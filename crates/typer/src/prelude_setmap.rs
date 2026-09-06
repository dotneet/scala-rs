//! The `Predef` wrapping methods that let an `Array` be passed as a `Seq`, and the
//! read members of `scala.collection.Map` (agent/setmap).
//!
//! # `Array` to collection
//!
//! The actual behaviour (2.13.16), confirmed with nsc's `-Xprint:typer`:
//!
//! ```text
//! def v(a: Array[Any]): Seq[Any]          = scala.Predef.copyArrayToImmutableIndexedSeq[Any](a)
//! def w(a: Array[Int]): Seq[Int]          = scala.Predef.copyArrayToImmutableIndexedSeq[Int](a)
//! def y(a: Array[Any]): Iterable[Any]     = scala.Predef.genericWrapArray[Any](a)
//! ```
//!
//! `scala.Seq` / `scala.IndexedSeq` are aliases of the **`immutable`** ones, so the
//! `scala.collection.mutable.ArraySeq` that `genericWrapArray` returns does not reach
//! them. The lowest-priority `copyArrayToImmutableIndexedSeq`
//! (`LowPriorityImplicits2`) is picked instead. `scala.Iterable` is
//! `scala.collection.Iterable`, which `genericWrapArray` does reach, so by priority
//! that one is picked.
//!
//! The brief's hypothesis ("`genericWrapArray` is unusable because the descriptor
//! does not match; add `wrapRefArray`") was **wrong**. What did not match was the
//! `([Ljava/lang/Object;)` you get from writing `Array[Any]`; declared as `Array[T]`
//! with a real type parameter, `array_elem_is_abstract` in `erasure.rs` collapses it
//! to `Ljava/lang/Object;` just as nsc does. javap:
//!
//! ```text
//! scala.LowPriorityImplicits:
//!   public <T> scala.collection.mutable.ArraySeq<T> genericWrapArray(java.lang.Object);
//! scala.LowPriorityImplicits2:
//!   public <T> scala.collection.immutable.IndexedSeq<T> copyArrayToImmutableIndexedSeq(java.lang.Object);
//! ```
//!
//! `wrapRefArray` is constrained to `T <: AnyRef` and does not apply to `Array[Any]`
//! (nor does nsc pick it, as above), so it is not added.
//!
//! For the same reason as `wrapBooleanArray` (`prelude_seqfn.rs`), these are **not**
//! `implicit`: as implicits they would compete with `refArrayOps` in ordinary member
//! selection on an `Array`. `seqfn_view.rs` looks them up by name.
//!
//! # `scala.collection.Map`
//!
//! The `scala/collection/Map` that `prelude_hier.rs`'s `LINKS` builds is a link with
//! nothing but type parameters, carrying no members at all. Prelude classes with a
//! `scala/` name are not touched by `pickle_supply::adopt_binary_class`
//! (`class_sym.0 < st.prelude_end`), so nothing is supplied from the jar either.
//! For slick's `expansions: collection.Map[TableIdentitySymbol, (TermSymbol, Node)]`,
//! `expansions contains tsym` came out `not a member`, and `expansions(tsym)` fell to
//! the companion's varargs `apply` and gave `no matching overload`.
//! Only the three read members of `collection.MapOps` are declared here.

use crate::prelude::{method, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// All of this is `library_abi` only. The private runtime (`--no-scala-library`) has
/// no implementation of `scala/collection/Map`, `IterableOps.++` or
/// `Predef.genericWrapArray`, and rather than passing the types and having codegen
/// call a method that does not exist, it is right to emit a diagnostic as before
/// (`.agent-brief.md`, "no stubs").
pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    add_collection_map_members(st);
    add_option_is_iterable_once(st);
    add_set_widening_concat(st);
    add_array_wraps(st);
}

/// `Set.++[B >: A](that: IterableOnce[B]): Set[B]`.
///
/// In 2.13 `++` is **two overloads** (`javap`):
///
/// ```text
/// scala.collection.SetOps:      public default C   $plus$plus(scala.collection.IterableOnce<A>);
/// scala.collection.IterableOps: public default <B> CC $plus$plus(scala.collection.IterableOnce<B>);
/// ```
///
/// The prelude side had only the one corresponding to the former (built by
/// `prelude_coll` as `++(Set[A]): Set[A]` and widened to `++(IterableOnce[A]): Set[A]`
/// by `prelude_buildfrom::widen_set_concat`), and since `lookup_member` finds it the
/// pickle side is never asked for `++` at all (confirmed with
/// `SCALA_RS_PICKLE_DEBUG=1`: `concat` is asked for, `++` is not).
/// That is why `s ++ anOptionOfSomethingElse` -- slick's
/// `Set() ++ dbType.map(…) ++ (if(…) Some(…) else None) ++ …` -- came out as
/// `no matching overload`.
///
/// SetOps is a subclass of IterableOps, which resolves equal-domain ties.
/// Code generation must preserve the selected declaration: SetOps returns C,
/// whereas IterableOps can widen to an ordinary Set.
fn add_set_widening_concat(st: &mut SymbolTable) {
    let (Some(set), Some(ioc)) = (
        crate::classpath::find_by_jvm(st, "scala/collection/immutable/Set"),
        crate::classpath::find_by_jvm(st, "scala/collection/IterableOnce"),
    ) else {
        return;
    };
    let Some(&a) = st.get(set).tparams.first() else {
        return;
    };
    let poly_already = st
        .get(set)
        .members
        .iter()
        .copied()
        .any(|m| st.get(m).name == "++" && !st.get(m).tparams.is_empty());
    if poly_already {
        return;
    }
    // These alternatives originate in different traits. Keeping both only
    // under Set loses the declaring-owner specificity relation.
    for mono in st.get(set).members.clone() {
        if st.get(mono).name == "++" && st.get(mono).tparams.is_empty() {
            st.get_mut(mono).declaring_class = "scala/collection/SetOps".into();
            st.get_mut(mono).declaring_is_interface = true;
        }
    }
    let m = st.alloc("++", set, SymKind::Method, Flags::EMPTY, "");
    st.get_mut(m).declaring_class = "scala/collection/IterableOps".into();
    st.get_mut(m).declaring_is_interface = true;
    let b = type_param(st, m, "B");
    st.get_mut(b).bound_lo = Some(Type::TypeParam(a));
    let tb = Type::TypeParam(b);
    let param_ty = Type::Class {
        sym: ioc,
        args: vec![tb.clone()],
    };
    let p = st.alloc("that", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(p).ty = param_ty.clone();
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![p];
    st.get_mut(m).paramss = vec![vec![p]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![param_ty]],
        ret: Box::new(Type::Class {
            sym: set,
            args: vec![tb],
        }),
    };
    st.get_mut(m).intrinsic = Intrinsic::None;
}

/// `Option[A] <: IterableOnce[A]`.
///
/// In 2.13 `Option` became an `IterableOnce` -- a real parent, not 2.12's
/// `option2Iterable` implicit conversion:
///
/// ```text
/// sealed abstract class Option[+A] extends IterableOnce[A] with Product with Serializable
/// ```
///
/// Without it, slick's
/// `Set() ++ dbType.map(...) ++ (if(...) Some(...) else None) ++ …`
/// comes out as `no matching overload for (IterableOnce[A])Set[A] with arguments
/// (Option[SqlType])`. Real scalac's `-Xprint:typer` shows
/// `Set.apply[String]().++(o)`, passing it straight through **with no conversion**.
///
/// Only the edge is added. In this prelude `IterableOnce` has nothing but `foreach`,
/// and `Option` has a `foreach` of its own, so inheritance adds no members.
fn add_option_is_iterable_once(st: &mut SymbolTable) {
    let Some(ioc) = crate::classpath::find_by_jvm(st, "scala/collection/IterableOnce") else {
        return;
    };
    let opt = st.option_sym;
    if st.get(ioc).tparams.len() != 1 || st.get(opt).tparams.len() != 1 {
        return;
    }
    if st
        .get(opt)
        .parents
        .iter()
        .any(|p| matches!(p, Type::Class { sym, .. } if *sym == ioc))
    {
        return;
    }
    let a = Type::TypeParam(st.get(opt).tparams[0]);
    st.get_mut(opt).parents.push(Type::Class {
        sym: ioc,
        args: vec![a],
    });
}

/// `Predef.genericWrapArray[T](xs: Array[T]): mutable.ArraySeq[T]` and
/// `Predef.copyArrayToImmutableIndexedSeq[T](xs: Array[T]): immutable.IndexedSeq[T]`.
fn add_array_wraps(st: &mut SymbolTable) {
    let predef = st.predef;
    let owner = match st.get(predef).ty.clone() {
        Type::ModuleRef(id) => id,
        _ => predef,
    };
    if let Some(arrseq) = crate::classpath::find_by_jvm(st, "scala/collection/mutable/ArraySeq") {
        add_wrap(st, owner, "genericWrapArray", arrseq);
    }
    if let Some(ixs) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/IndexedSeq") {
        add_wrap(st, owner, "copyArrayToImmutableIndexedSeq", ixs);
    }
}

/// Add an `Array[T] => Cls[T]` with a single type parameter `T` to `owner`.
///
/// The erasure of `Array[T]` (with `T` abstract) is `Ljava/lang/Object;`, so it
/// matches the real ABI descriptor as it stands. The result is `Cls[T]`, i.e.
/// `Lscala/collection/...;`.
fn add_wrap(st: &mut SymbolTable, owner: SymbolId, name: &str, cls: SymbolId) {
    if !st.lookup(name).is_empty() {
        return;
    }
    let m = st.alloc(name, owner, SymKind::Method, Flags::EMPTY, "");
    let t = type_param(st, m, "T");
    let tt = Type::TypeParam(t);
    let param = st.alloc("xs", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(param).ty = Type::Array(Box::new(tt.clone()));
    st.get_mut(m).tparams = vec![t];
    st.get_mut(m).params = vec![param];
    st.get_mut(m).paramss = vec![vec![param]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Array(Box::new(tt.clone()))]],
        ret: Box::new(Type::Class {
            sym: cls,
            args: vec![tt],
        }),
    };
    st.get_mut(m).intrinsic = Intrinsic::None;
    st.get_mut(owner).members.push(m);
    st.enter_in_current(name, m);
}

/// The three read members of `collection.MapOps`.
///
/// javap (`scala.collection.MapOps`):
/// ```text
/// public abstract scala.Option<V> get(K);
/// public V apply(K);
/// public boolean contains(K);
/// ```
fn add_collection_map_members(st: &mut SymbolTable) {
    let Some(map) = crate::classpath::find_by_jvm(st, "scala/collection/Map") else {
        return;
    };
    if st.get(map).tparams.len() != 2 {
        return;
    }
    let k = Type::TypeParam(st.get(map).tparams[0]);
    let v = Type::TypeParam(st.get(map).tparams[1]);
    if st
        .get(map)
        .members
        .iter()
        .any(|&m| st.get(m).name == "contains")
    {
        return;
    }
    method(
        st,
        map,
        "contains",
        vec![k.clone()],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        map,
        "apply",
        vec![k.clone()],
        v.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "get",
        vec![k],
        Type::Class {
            sym: st.option_sym,
            args: vec![v],
        },
        Intrinsic::None,
    );
}
