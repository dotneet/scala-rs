//! `Map` は `K => V` である。
//!
//! `prelude.rs` からは [`install`] を 1 行呼ぶだけにしてある。
//!
//! 2.13 の `scala.collection.Map[K, +V]` は宣言そのものが
//! `PartialFunction[K, V]`（したがって `K => V`）を継承している
//! （`javap scala/collection/Map` の interface 一覧に `scala/Function1` が
//! 並ぶ）。prelude の階層表（`prelude_hier.rs`）は `Iterable` 側の辺しか
//! 張っていなかったので、
//!
//! ```scala
//! val symbolToIndex: TermSymbol => Int = someMap   // slick QueryInterpreter
//! ```
//!
//! が `type mismatch; found: Map[TermSymbol, Int]  required: (TermSymbol) => Int`
//! になっていた。辺を 1 本足すだけで解ける。`Seq[A] <: Int => A` /
//! `Set[A] <: A => Boolean` も実在するが、ここでは張らない（オーバーロード
//! 解決と implicit 探索に効く範囲が広く、slick では必要にならない）。
//!
//! `Function1` を親に持てるようになったのは `symbol.rs` の
//! `function_class_shape` を入れたため。`scala.FunctionN[T1, …, R]` という
//! クラス型と構造的な `(T1, …) => R` は同じ型で、prelude は前者を親として
//! 書き、それ以外の場所では後者を使う。
//!
//! **私有ランタイム（`--no-scala-library`）** でも `Map` も `Function1` も
//! prelude が用意しているのでゲート不要。見つからないときは黙って何もしない。

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
