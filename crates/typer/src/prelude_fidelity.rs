//! Attributes the hand-written prelude was dropping that the library's own
//! pickle carries.
//!
//! A member supplied from a `ScalaSignature` arrives with its pickled flags,
//! its parameter names and its constructor fields. A member the prelude
//! writes by hand arrives with whatever `prelude::class` / `prelude::method`
//! happened to set, and those two helpers set `Flags::FINAL` and nothing else.
//! Where the prelude declares a class the library also declares, the
//! hand-written symbol is the one member lookup finds -- `PickleSupply` is
//! consulted only when nothing matched or when the classfile declares a
//! signature none of the candidates has -- so every attribute the prelude
//! leaves off is an attribute the compiler does not have, even in
//! `--scala-library` mode.
//!
//! This module closes the gaps that make the compiler reject a program scalac
//! accepts. Each entry is pinned by `javap -p` against
//! scala-library 2.13.16, not from memory.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId};

/// The library classes the prelude hand-writes that are `case class`es.
///
/// `javap -p scala.Some` (and the four beside it) declares `copy` and
/// `copy$default$1`, which is what a `case class` and only a `case class`
/// gets. `Flags::CASE` is what `Check::try_rewrite_case_copy` keys on, so
/// without it `Some(1).copy(value = 2)`, `Left(1).copy(value = 2)`,
/// `Right(x).copy(value = y)`, `Success(1).copy(value = 2)` and
/// `Failure(e).copy(exception = e2)` were all `value copy is not a member`.
/// This is the same defect `prelude_tuple::mark_case` fixed for `TupleN`,
/// found by comparing every prelude class against its pickled flags.
///
/// `scala.StringContext` is deliberately absent: it is a case class too, but
/// its single parameter is repeated (`case class StringContext(parts:
/// String*)`) and `javap` shows it has no `copy` at all, so claiming one
/// would invent a member the library does not have.
const CASE_CLASSES: &[&str] = &[
    "scala/Some",
    "scala/util/Left",
    "scala/util/Right",
    "scala/util/Success",
    "scala/util/Failure",
];

/// Set the flags the prelude left off. Called once from `install_prelude`,
/// after every prelude module has run.
pub(crate) fn install(st: &mut SymbolTable) {
    for jvm in CASE_CLASSES {
        for id in classes_named(st, jvm) {
            // A case class's fields are what `copy` rebuilds from; a class
            // with none of them would give `try_rewrite_case_copy` nothing to
            // work with, and marking it would be a claim we cannot back.
            if st.get(id).ctor_fields.is_empty() {
                continue;
            }
            let f = st.get(id).flags.with(Flags::CASE);
            st.get_mut(id).flags = f;
        }
    }
}

/// Every class-like prelude symbol with this JVM internal name.
///
/// By name rather than through `SymbolTable::find_class_by_jvm`, which picks
/// one symbol: the prelude declares more than one symbol for a few of these
/// names (`::` has both `$colon$colon` and an alias), and an attribute has to
/// land on all of them or the answer depends on which one lookup returns.
fn classes_named(st: &SymbolTable, jvm: &str) -> Vec<SymbolId> {
    st.symbols
        .iter()
        .filter(|s| s.kind == SymKind::Class && s.jvm_name == jvm)
        .map(|s| s.id)
        .collect()
}
