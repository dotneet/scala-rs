//! Small, independently-fixable prelude corrections (agent/smallgaps).
//!
//! `Option[A].flatMap` was declared in `prelude.rs` reusing the class's own
//! type parameter `A` for both the lambda's return type and the method's own
//! return type:
//!
//! ```text
//! def flatMap(f: A => Option[A]): Option[A]   // wrong: not polymorphic
//! ```
//!
//! instead of a fresh method type parameter `B`:
//!
//! ```text
//! def flatMap[B](f: A => Option[B]): Option[B]  // real scalac signature
//! ```
//!
//! Because `ret` was a concrete `Option[A]` (not a bare type-parameter
//! referencing one of the *method's own* tparams), none of the inference
//! fallbacks in `check.rs::type_apply` kicked in: the array-ops `flatMap`
//! special case is gated on `is_array_ops_ty`, and the generic "substitute a
//! bare method tparam from the lambda's actual result" pass only fires when
//! `ret` itself is `Type::TypeParam(id)` with `id` among the method's own
//! tparams. The upshot: `Option(x).flatMap(y => Some(y.toString))` type
//! checked the lambda body against `Option[A]` (the *receiver's* element
//! type) and then reported the overall call as `Option[A]` too, e.g.
//! `type mismatch; found: Option[String]  required: Option[Int]`.
//!
//! `List.flatMap` (see `prelude_seq.rs`'s `poly_in`) already uses the correct
//! fresh-tparam-per-call shape and does not have this bug; this module makes
//! `Option.flatMap` match it. Confirmed against real scalac 2.13.16 (no
//! error) and reproduced against scala-rs pre-fix in both `--scala-library`
//! and `--no-scala-library` modes. See `crates/cli/tests/smallgaps.rs`
//! (`sgap` fixture) for the dual-run regression test.
//!
//! This is also the root cause of a chunk of slick's cascaded
//! `value length/varying is not a member of FieldSymbol` errors: slick's
//! `sym.flatMap(_.findColumnOption[RelationalProfile.ColumnOption.Length])`
//! (`Option[FieldSymbol] => Option[ColumnOption.Length]`) hit exactly this
//! bug, so the extracted `Length` value downstream was mistyped back to
//! `FieldSymbol`.

use crate::prelude::{fn1, method, module, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, Type};

/// Rebuild `Option.flatMap` with a fresh method type parameter, in place of
/// the non-polymorphic signature `prelude.rs::add_option_members` installs.
pub fn fix_option_flat_map(st: &mut SymbolTable) {
    let option_sym = st.option_sym;
    let Some(&a) = st.get(option_sym).tparams.first() else {
        return;
    };
    let ta = Type::TypeParam(a);
    let Some(flat_map) = st
        .lookup_member(option_sym, "flatMap")
        .into_iter()
        .find(|&m| st.get(m).kind == SymKind::Method)
    else {
        return;
    };
    let b = type_param(st, flat_map, "B");
    let opt_b = Type::Class {
        sym: option_sym,
        args: vec![Type::TypeParam(b)],
    };
    let f = st.alloc("f", flat_map, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), opt_b.clone());
    st.get_mut(flat_map).tparams = vec![b];
    st.get_mut(flat_map).params = vec![f];
    st.get_mut(flat_map).paramss = vec![vec![f]];
    st.get_mut(flat_map).ty = Type::Method {
        paramss: vec![vec![fn1(ta, opt_b.clone())]],
        ret: Box::new(opt_b),
    };
}

/// `scala.collection.Iterable`'s companion `apply[A](elems: A*): Iterable[A]`
/// (real scalac 2.13.16 signature, `javap`-verified as
/// `IterableFactory$Delegate.apply(Seq): Object`, *inherited* onto `Iterable$`
/// -- not declared directly on it).
///
/// `crates/typer/src/prelude_coll.rs::find_or_create_iterable` already
/// installs `scala.collection.Iterable[A]` itself (a bare trait, no
/// companion) for `library_abi` mode, modeled after the members needed to
/// *consume* an `Iterable` (`foreach`, `mkString`, ...). It has no `apply`
/// because nothing ever created an `Iterable` *module* to hang one on: an
/// unqualified `Iterable` reference falls through to the generic real-jar
/// pickle-supply path, which only sees members `scala.collection.Iterable$`
/// declares *directly* in its own pickle -- not `apply`, which it inherits
/// from `IterableFactory$Delegate` (a plain Java-visible superclass, invisible
/// to the pickle reader). `List` / `Seq` hit the exact same
/// inherited-from-a-Delegate shape and dodge it by being eagerly declared as
/// their own module in `prelude.rs` (`add_list_members` / `add_seq_and_lazylist`)
/// so ordinary scope lookup finds them before pickle-supply ever runs; this
/// function gives `Iterable` the same eager companion, mirroring `Seq`'s
/// `apply` declaration (`prelude.rs::add_seq_and_lazylist`) exactly.
///
/// `library_abi`-gated only: the private runtime (`crates/backend/src/runtime.rs`)
/// has no backing classfile for a bare `Iterable` factory (there is no single
/// concrete collection a private-runtime `Iterable(...)` could build), so
/// `--no-scala-library` deliberately leaves `Iterable` without this member --
/// the existing "value apply is not a member of Iterable" diagnostic is the
/// intended, honest behavior there (see `.agent-brief.md`'s rule against
/// silently accepting members the private runtime cannot back).
pub fn add_iterable_apply(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    let Some(iterable) = st
        .get(coll)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).kind == SymKind::Class && st.get(m).name == "Iterable")
    else {
        return;
    };
    if st.get(iterable).tparams.is_empty() {
        return;
    }
    // Placed directly under the `scala` package, exactly like `List`/`Seq`'s
    // own companions -- `scala.Iterable` is a package-object alias for
    // `scala.collection.Iterable` in the real library, and modeling that
    // alias mechanism is out of scope here; an eagerly-declared symbol at the
    // same lookup point real code resolves it from is enough.
    let iterable_mod = module(st, st.scala_pkg, "Iterable", "scala/collection/Iterable$");
    let mcls = st.module_class_of(iterable_mod);
    let apply = method(st, mcls, "apply", vec![], Type::Unit, Intrinsic::None);
    let a = type_param(st, apply, "A");
    st.get_mut(apply).tparams = vec![a];
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(a)))]],
        ret: Box::new(Type::Class {
            sym: iterable,
            args: vec![Type::TypeParam(a)],
        }),
    };
    let mems = st.get(mcls).members.clone();
    st.get_mut(iterable_mod).members.extend(mems);
}
