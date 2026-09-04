//! A `Seq[+A]` is a `PartialFunction[Int, A]` (hence an `Int => A`).
//!
//! In 2.13 the declaration of `scala.collection.Seq[A]` itself extends
//! `PartialFunction[Int, A]` (`javap scala/collection/Seq`):
//!
//! ```text
//! public interface scala.collection.Seq<A> extends scala.collection.Iterable<A>,
//!   scala.PartialFunction<java.lang.Object, A>, scala.collection.SeqOps<...>, scala.Equals
//! ```
//!
//! `prelude_hier.rs` wired up only the edge to `Iterable`, so
//!
//! ```scala
//! val f: Int => Int = List(10, 20, 30)   // scalac accepts this
//! List(0, 2).map(f)                       // pass a Seq as an Int => A
//! List(1, 2).isDefinedAt(5)                // grows through PartialFunction
//! ```
//!
//! came out as `type mismatch; found: List[Int]  required: (Int) => Int`. One edge is
//! added, the same shape as `Map` (`prelude_mism4.rs`). `Set[A] <: A => Boolean` is
//! real too, but is not wired up here (out of scope for this slice).
//!
//! The edge goes in one place only: `scala/collection/Seq` (assembled by
//! `prelude_hier`, the common ancestor of `List` / `Vector` / `ArraySeq` / `Range` /
//! `LazyList` / `Queue` / `mutable.Seq` (including `Buffer` / `ArrayBuffer` /
//! `ListBuffer`) and the rest). `base_type_seq` walks parents transitively, so that
//! alone propagates to every concrete collection below it (`Vector[Int] <: Int => Int`
//! and friends were confirmed by dual runs).
//!
//! Unlike `Map` (see the comment in `prelude_mism4.rs`), there is no known member that
//! breaks when `PartialFunction` is made a direct parent. `Seq.apply(Int): A` sits in
//! `List`'s own `members` as a concrete member, and `lookup_member` collects an
//! owner's own members before its parents'. The case where `Seq[A]` has two `apply`s
//! that can only be told apart after instantiation -- `SeqOps.apply(Int): A` and
//! `PartialFunction[Int, A].apply(Int): A` -- is already handled in general by
//! `overload_member_types` in `check.rs` (the typechecking context), which has a doc
//! comment saying exactly that.
//!
//! `PartialFunction` also grows `lift` / `orElse` (nsc: the default methods in
//! `javap scala.PartialFunction`). `prelude.rs::add_partial_function` only added
//! `apply` / `isDefinedAt` / `applyOrElse`.
//!
//! **`library_abi` only**: the private runtime's `scala/PartialFunction`
//! (`--no-scala-library`, `crates/backend/src/runtime.rs`) is an abstract interface
//! with nothing but `isDefinedAt` / `applyOrElse`, with no default implementation of
//! `lift` / `orElse`, and the private classfiles for `List` / `Vector` and the rest do
//! not implement `scala/PartialFunction` / `scala/Function1` either. Adding the edge
//! or the members there would make a broken link -- "the types go through but the
//! invokeinterface lands on something with no implementation" (the same reason as
//! `tupled`/`curried` in `prelude_fntuple.rs`). Under `--no-scala-library` the
//! diagnostics stay as they were: `type mismatch` / `value lift is not a member of ...`.

use crate::prelude::fn1;
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::{SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    seq_is_a_partial_function(st);
    add_lift_and_or_else(st);
    mutable_array_seq_is_an_indexed_seq(st);
    add_wrap_boolean_array(st);
}

/// `Seq[A] <: PartialFunction[Int, A]`.
fn seq_is_a_partial_function(st: &mut SymbolTable) {
    let Some(pf) = crate::classpath::find_by_jvm(st, "scala/PartialFunction") else {
        return;
    };
    let Some(seq) = crate::classpath::find_by_jvm(st, "scala/collection/Seq") else {
        return;
    };
    let tps = st.get(seq).tparams.clone();
    if tps.len() != 1 {
        return;
    }
    let parent = Type::Class {
        sym: pf,
        args: vec![Type::Int, Type::TypeParam(tps[0])],
    };
    if st.get(seq).parents.contains(&parent) {
        return;
    }
    st.get_mut(seq).parents.push(parent);
}

/// `PartialFunction[A, B].lift: A => Option[B]` and
/// `.orElse(that: PartialFunction[A, B]): PartialFunction[A, B]`.
///
/// nsc's real signatures carry their own bounded type parameters
/// (`orElse[A1 <: A, B1 >: B](that: PartialFunction[A1, B1]): PartialFunction[A1, B1]`);
/// `add_partial_function`'s `applyOrElse` already simplifies the same way to
/// `ta`/`tb` directly, so this follows suit.
fn add_lift_and_or_else(st: &mut SymbolTable) {
    let Some(pf) = crate::classpath::find_by_jvm(st, "scala/PartialFunction") else {
        return;
    };
    let tps = st.get(pf).tparams.clone();
    let [a, b] = tps.as_slice() else { return };
    let ta = Type::TypeParam(*a);
    let tb = Type::TypeParam(*b);
    if !members_named(st, pf, "lift").is_empty() {
        return;
    }
    let opt_b = Type::Class {
        sym: st.option_sym,
        args: vec![tb.clone()],
    };
    crate::prelude::prelude_method(
        st,
        pf,
        "lift",
        vec![],
        fn1(ta.clone(), opt_b),
        Intrinsic::None,
    );
    let pf_ty = Type::Class {
        sym: pf,
        args: vec![ta, tb],
    };
    crate::prelude::prelude_method(
        st,
        pf,
        "orElse",
        vec![pf_ty.clone()],
        pf_ty,
        Intrinsic::None,
    );
}

/// `mutable.ArraySeq[A] <: mutable.IndexedSeq[A]`.
///
/// `prelude_mutcoll.rs`'s `add_factory(st, mutp, "ArraySeq", class_tag)`
/// builds `scala/collection/mutable/ArraySeq` by hand with `AnyRef` as its
/// only parent -- it only needed a constructible `CC` for the companion's
/// `apply`/`empty`. `prelude_hier.rs`'s edge table wires `immutable.ArraySeq`
/// into the `Seq` chain but never mentions the mutable one, so
/// `wrapBooleanArray` below would hand back a value with no path to `Seq` /
/// `PartialFunction` (nor, for that matter, to plain `Iterable`) without this.
///
/// nsc's real `mutable.ArraySeq[T]` is `AbstractSeq[T] with IndexedSeq[T]
/// with ...`; going straight to `mutable.IndexedSeq` (already wired to `Seq`
/// by `prelude_hier`) gets the same conformances without re-deriving the
/// whole chain here.
fn mutable_array_seq_is_an_indexed_seq(st: &mut SymbolTable) {
    let Some(arrseq) = crate::classpath::find_by_jvm(st, "scala/collection/mutable/ArraySeq")
    else {
        return;
    };
    let Some(idx) = crate::classpath::find_by_jvm(st, "scala/collection/mutable/IndexedSeq") else {
        return;
    };
    let tps = st.get(arrseq).tparams.clone();
    if tps.len() != 1 {
        return;
    }
    let parent = Type::Class {
        sym: idx,
        args: vec![Type::TypeParam(tps[0])],
    };
    if st
        .get(arrseq)
        .parents
        .iter()
        .any(|p| matches!(p, Type::Class { sym, .. } if *sym == idx))
    {
        return;
    }
    st.get_mut(arrseq).parents.push(parent);
}

/// `LowPriorityImplicits.wrapBooleanArray(xs: Array[Boolean]):
/// mutable.ArraySeq$ofBoolean` (`Predef$` extends `LowPriorityImplicits` by
/// class inheritance, so `Predef.wrapBooleanArray(...)` resolves through it).
///
/// nsc JVM (`javap -p scala.LowPriorityImplicits` -- `javap -p
/// scala.Predef$` does not list it at all, since `javap` only lists members a
/// class *declares*, not ones a superclass does; `Predef$` inherits it
/// unmodified):
/// `public scala.collection.mutable.ArraySeq$ofBoolean wrapBooleanArray(boolean[]);`
///
/// The return type has to be the exact `$ofBoolean` subtype, not the
/// `mutable.ArraySeq` trait itself -- a first attempt using the trait typed
/// fine but failed at the JVM with `NoSuchMethodError:
/// 'scala.collection.mutable.ArraySeq scala.Predef$.wrapBooleanArray(...)'`,
/// since `invokevirtual` matches on the exact descriptor. `$ofBoolean` is
/// declared here as a small stub (the same move `wrapIntArray`
/// (`prelude.rs::add_predef_members`) makes for `ArraySeq$ofInt`), but with a
/// parent that actually reaches `Seq`/`PartialFunction`:
/// `mutable.ArraySeq[Boolean]` -- via `mutable_array_seq_is_an_indexed_seq`
/// above -- rather than `ArraySeq$ofInt`'s bare `Iterable[Int]` (that one only
/// ever had to satisfy an `asIterable` witness type and is never actually
/// linked against, so the gap never showed up).
///
/// Deliberately **not** `IMPLICIT`: nsc itself keeps `wrapXArray` at low
/// priority precisely so it does not out-compete `xArrayOps` for ordinary
/// `Array[Boolean]` method calls (`prelude.rs`'s comment on `wrapIntArray`
/// says the same). This prelude has no priority tiers, so the method simply
/// stays out of general implicit search; `seqfn_view.rs` reaches it by name,
/// the same way `array_wrap_view` (`check.rs`) already reaches `wrapIntArray`.
fn add_wrap_boolean_array(st: &mut SymbolTable) {
    let Some(arrseq) = crate::classpath::find_by_jvm(st, "scala/collection/mutable/ArraySeq")
    else {
        return;
    };
    if !st.lookup("wrapBooleanArray").is_empty() {
        return;
    }
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let of_bool = crate::prelude::class(
        st,
        mutp,
        "ArraySeq$ofBoolean",
        "scala/collection/mutable/ArraySeq$ofBoolean",
        &[Type::Class {
            sym: arrseq,
            args: vec![Type::Boolean],
        }],
    );
    let p = st.predef;
    let owner = match st.get(p).ty.clone() {
        Type::ModuleRef(id) => id,
        _ => p,
    };
    let ret = Type::Class {
        sym: of_bool,
        args: vec![],
    };
    let m = crate::prelude::prelude_method(
        st,
        owner,
        "wrapBooleanArray",
        vec![Type::Array(Box::new(Type::Boolean))],
        ret,
        Intrinsic::None,
    );
    st.enter_in_current("wrapBooleanArray", m);
}

fn members_named(st: &SymbolTable, owner: SymbolId, name: &str) -> Vec<SymbolId> {
    st.get(owner)
        .members
        .iter()
        .copied()
        .filter(|m| st.get(*m).name == name)
        .collect()
}
