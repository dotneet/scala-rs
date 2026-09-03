//! `scala.collection.Map` の `MapOps` メンバ。
//!
//! `prelude.rs` からは [`install`] を 1 行呼ぶだけにしてある。
//!
//! `prelude_hier.rs` の作る「リンク用」トレイト
//! （`scala/collection/Map`）はメンバを一切持たない。具体的な
//! `immutable.Map` / `mutable.Map` の側にだけ `get` / `contains` /
//! `getOrElse` / `apply` が生えていたので、
//!
//! ```scala
//! def createResult(expansions: collection.Map[TableIdentitySymbol, (TermSymbol, Node)], …) =
//!   … if expansions contains tsym then expansions(tsym) …   // slick ExpandTables
//! ```
//!
//! のように**抽象側の型で受けた**とたん `value contains is not a member of
//! Map[…]` になり、`expansions(tsym)` は `Map` コンパニオンの `apply` を
//! 拾って `no matching overload for ((K, V)*)Map[K, V]` になっていた。
//! 2.13 では 4 つとも `scala.collection.MapOps` の宣言なので、抽象側に置く
//! のが正しい（`javap scala/collection/Map` にすべて並ぶ）。
//!
//! シグネチャは `immutable.Map` 側（`prelude.rs` / `prelude_coll.rs`）と
//! そろえてある。`get`/`contains`/`getOrElse`/`apply` の鍵引数が `Any` なのは
//! 2.13 の `MapOps` が `K` を受けるのに対し prelude が既にそう書いていたため
//! で、ここだけ厳しくすると継承側と食い違う。
//!
//! **私有ランタイム（`--no-scala-library`）** でも `scala/collection/Map` は
//! prelude が用意しているのでゲート不要。無ければ黙って何もしない。

use crate::symbol::SymbolTable;
use scala_rs_parser::Type;

pub(crate) fn install(st: &mut SymbolTable) {
    let Some(map) = crate::classpath::find_by_jvm(st, "scala/collection/Map") else {
        return;
    };
    let tps = st.get(map).tparams.clone();
    if tps.len() != 2 {
        return;
    }
    let tv = Type::TypeParam(tps[1]);
    let option = st.option_sym;
    if option.is_none() {
        return;
    }
    let decls: Vec<(&str, Vec<Type>, Type)> = vec![
        ("apply", vec![Type::Any], tv.clone()),
        (
            "get",
            vec![Type::Any],
            Type::Class {
                sym: option,
                args: vec![tv.clone()],
            },
        ),
        ("contains", vec![Type::Any], Type::Boolean),
        (
            "getOrElse",
            vec![Type::Any, Type::ByName(Box::new(tv.clone()))],
            tv.clone(),
        ),
    ];
    for (name, params, ret) in decls {
        if st
            .lookup_member(map, name)
            .iter()
            .any(|&m| st.get(m).owner == map)
        {
            continue;
        }
        crate::prelude::prelude_method(st, map, name, params, ret, crate::symbol::Intrinsic::None);
    }
}
