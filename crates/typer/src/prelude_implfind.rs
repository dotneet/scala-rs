//! The `MapOps` members of `scala.collection.Map`.
//!
//! `prelude.rs` calls [`install`] on a single line.
//!
//! The "linking" trait `prelude_hier.rs` builds (`scala/collection/Map`) carries no
//! members at all. `get` / `contains` / `getOrElse` / `apply` grew only on the
//! concrete `immutable.Map` / `mutable.Map`, so
//!
//! ```scala
//! def createResult(expansions: collection.Map[TableIdentitySymbol, (TermSymbol, Node)], …) =
//!   … if expansions contains tsym then expansions(tsym) …   // slick ExpandTables
//! ```
//!
//! turned into `value contains is not a member of Map[…]` the moment it was **taken
//! at the abstract type**, and `expansions(tsym)` picked up the `Map` companion's
//! `apply` and gave `no matching overload for ((K, V)*)Map[K, V]`. In 2.13 all four
//! are declarations on `scala.collection.MapOps`, so the abstract side is where they
//! belong (they are all listed in `javap scala/collection/Map`).
//!
//! The signatures line up with the `immutable.Map` side (`prelude.rs` /
//! `prelude_coll.rs`). The key parameter of `get`/`contains`/`getOrElse`/`apply` is
//! `Any` because that is how the prelude already wrote it, where 2.13's `MapOps`
//! takes `K`; tightening it only here would disagree with the inheriting side.
//!
//! No gate is needed for the **private runtime (`--no-scala-library`)** either, since
//! the prelude provides `scala/collection/Map`. If it is absent, do nothing.

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
