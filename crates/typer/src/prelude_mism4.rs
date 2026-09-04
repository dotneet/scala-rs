//! A `Map` is a `K => V`.
//!
//! `prelude.rs` calls [`install`] on a single line.
//!
//! In 2.13 the declaration of `scala.collection.Map[K, +V]` itself extends
//! `PartialFunction[K, V]` (hence `K => V`) -- `scala/Function1` is right there in
//! the interface list of `javap scala/collection/Map`. The prelude's hierarchy table
//! (`prelude_hier.rs`) wired up only the `Iterable` edge, so
//!
//! ```scala
//! val symbolToIndex: TermSymbol => Int = someMap   // slick QueryInterpreter
//! ```
//!
//! came out as `type mismatch; found: Map[TermSymbol, Int]  required: (TermSymbol) => Int`.
//! One extra edge solves it. `Seq[A] <: Int => A` / `Set[A] <: A => Boolean` are real
//! too, but are not wired up here (they reach further into overload resolution and
//! implicit search, and slick never needs them).
//!
//! `Function1` became usable as a parent once `symbol.rs`'s `function_class_shape`
//! went in. The class type `scala.FunctionN[T1, …, R]` and the structural
//! `(T1, …) => R` are the same type; the prelude writes the former as a parent and
//! everywhere else uses the latter.
//!
//! No gate is needed for the **private runtime (`--no-scala-library`)** either, since
//! the prelude provides both `Map` and `Function1`. If they are not found, do nothing.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable) {
    map_is_a_function(st);
}

/// `Map[K, V] <: Function1[K, V]`.
///
/// The edge goes on every `Map` the prelude built. `scala/collection/Map` is
/// not one of them (`prelude_hier`'s edge to it is skipped for want of a
/// class), so the immutable and mutable ones each get their own.
fn map_is_a_function(st: &mut SymbolTable) {
    let Some(f1) = scala_class(st, "Function1") else {
        return;
    };
    for jvm in [
        "scala/collection/Map",
        "scala/collection/immutable/Map",
        "scala/collection/mutable/Map",
    ] {
        let Some(map) = crate::classpath::find_by_jvm(st, jvm) else {
            continue;
        };
        let tps = st.get(map).tparams.clone();
        if tps.len() != 2 {
            continue;
        }
        let parent = Type::Class {
            sym: f1,
            args: vec![Type::TypeParam(tps[0]), Type::TypeParam(tps[1])],
        };
        if st.get(map).parents.contains(&parent) {
            continue;
        }
        st.get_mut(map).parents.push(parent);
    }
}

fn scala_class(st: &SymbolTable, name: &str) -> Option<SymbolId> {
    st.get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == name && st.get(*id).kind == SymKind::Class)
}
