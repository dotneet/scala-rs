//! `scala.runtime.LazyRef` and its unboxed siblings: the cells a *method-local*
//! `lazy val` is compiled into.
//!
//! nsc's `lazyvals` phase gives every local `lazy val` a one-slot cell instead
//! of the `bitmap$0` + field pair a class member gets — there is no instance to
//! hang a field on. The cell is `scala.runtime.LazyRef` for reference results
//! and a monomorphic `LazyInt` / `LazyLong` / … for the primitives, so the
//! value is not boxed on every read:
//!
//! ```text
//! new LazyInt()                         // at the declaration
//! expensive$1(cell)                     // at every use
//! ```
//!
//! Only the *classes* are modelled here. Their three members
//! (`initialized()` / `value()` / `initialize(v)`) are never named by user
//! code and never resolved by the typer: `crates/backend/src/gen.rs` emits the
//! `invokevirtual`s directly, the way it already does for the `*Ref` boxes a
//! captured `var` lives in. `crates/backend/src/runtime.rs` emits stand-ins for
//! the private-runtime (`--no-scala-library`) mode.

use scala_rs_parser::{Flags, Type};

use crate::prelude::class;
use crate::symbol::{SymKind, SymbolTable};

/// The cell classes, in `SymbolTable::lazy_cells` order. `LazyRef` first so
/// index 0 is always a usable fallback.
pub(crate) const CELL_NAMES: &[&str] = &[
    "LazyRef",
    "LazyBoolean",
    "LazyByte",
    "LazyChar",
    "LazyShort",
    "LazyInt",
    "LazyLong",
    "LazyFloat",
    "LazyDouble",
    "LazyUnit",
];

pub(crate) fn install(st: &mut SymbolTable) {
    let runtime = st.alloc(
        "runtime",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/runtime",
    );
    let mut cells = Vec::with_capacity(CELL_NAMES.len());
    for n in CELL_NAMES {
        let jvm = format!("scala/runtime/{n}");
        cells.push(class(st, runtime, n, &jvm, &[Type::AnyRef]));
    }
    st.lazy_cells = cells;
}

/// Index into `SymbolTable::lazy_cells` for a local `lazy val` of type `ty`.
///
/// A value class is deliberately *not* unboxed here: erasure has not run when
/// the local-lazy pass builds the cell, so `lazy val m: Meters` would pick
/// `LazyInt` from a type the backend later sees as `int`-or-`Meters` depending
/// on the use. `LazyRef` is always sound — the backend boxes on the way in and
/// unboxes on the way out when the cell and the result disagree.
fn cell_index(ty: &Type) -> usize {
    match ty.widen_constant() {
        Type::Boolean => 1,
        Type::Byte => 2,
        Type::Char => 3,
        Type::Short => 4,
        Type::Int => 5,
        Type::Long => 6,
        Type::Float => 7,
        Type::Double => 8,
        Type::Unit => 9,
        _ => 0,
    }
}

/// The cell class a local `lazy val` of type `ty` stores its value in, or
/// `None` when the prelude was not installed (never in a real compilation).
pub(crate) fn cell_class(st: &SymbolTable, ty: &Type) -> Option<scala_rs_parser::SymbolId> {
    st.lazy_cells.get(cell_index(ty)).copied()
}
