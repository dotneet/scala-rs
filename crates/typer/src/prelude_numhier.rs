//! `scala.math` の型クラス階層（`Numeric` / `Integral` / `Fractional` /
//! `Ordering`）の親子関係を張る。
//!
//! `crates/typer/src/prelude.rs` の `install_prelude` から 1 行だけ呼ばれる。
//! マージ衝突を避けるため新規モジュールに分けた（`.agent-brief.md` の方針）。
//!
//! prelude は `scala.math.Numeric` を「`sum` / `product` の implicit 引数を
//! 解決するための入れ物」として合成しているだけで、実 ABI の
//! `interface scala.math.Numeric<T> extends scala.math.Ordering<T>`
//! （`javap` で確認）という継承を写していなかった。そのため
//!
//! ```scala
//! class B[T](implicit ct: ClassTag[T], ord: Ordering[T])
//! class N[T](implicit tag: ClassTag[T], num: Numeric[T]) extends B[T]()(tag, num)
//! ```
//!
//! （slick `ScalaNumericType`）の `Numeric[T]` → `Ordering[T]` が通らず、
//! `no matching overload for constructor` になっていた。メンバ解決だけは
//! `lookup_member` が別経路で当たっていたので、症状は「サブタイプだけ落ちる」
//! という分かりにくい形で出る。
//!
//! `Integral` / `Fractional` は prelude の時点では symbol table にいない
//! （ソースが名前を出したときに jar から読まれる）ので、ここでは触らない。
//! それらの `<: Numeric[T]` はまだ通らない（既知の残件）。

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable) {
    let Some(ordering) = crate::classpath::find_by_jvm(st, "scala/math/Ordering") else {
        return;
    };
    if let Some(numeric) = crate::classpath::find_by_jvm(st, "scala/math/Numeric") {
        add_parent(st, numeric, ordering);
    }
}

/// `child[T] extends parent[T]`。`child` の最初の型パラメータをそのまま
/// 渡す（`Numeric[T] <: Ordering[T]`）。すでに同じ親を持つなら何もしない。
fn add_parent(st: &mut SymbolTable, child: SymbolId, parent: SymbolId) {
    if st.get(child).kind != SymKind::Class {
        return;
    }
    if st
        .get(child)
        .parents
        .iter()
        .any(|p| matches!(p, Type::Class { sym, .. } if *sym == parent))
    {
        return;
    }
    let args = match st.get(child).tparams.first().copied() {
        Some(tp) if !st.get(parent).tparams.is_empty() => vec![Type::TypeParam(tp)],
        _ => Vec::new(),
    };
    st.get_mut(child)
        .parents
        .push(Type::Class { sym: parent, args });
}
