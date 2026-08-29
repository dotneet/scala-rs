//! `Coll.empty` は本来 `def empty[A]: Coll[A]`（`Map` 系は `[K, V]`）である。
//!
//! prelude の多くのコンパニオンはこれを単相の `Coll[Any]` として宣言していた
//! ため、`val c: Vector[Int] = Vector.empty` が `found: Vector[Any]` で落ちて
//! いた（scalac は通す）。個々の宣言を書き換えるとファイル全体が衝突するので、
//! prelude を組み立て終わったあとに一括で多相化する。
//!
//! 対象は「コンパニオンのモジュールクラスに属し、型パラメータも引数も持たず、
//! 結果型が `Coll[Any, …]`（`Coll` の型パラメータ数と一致）である `empty`」
//! だけ。`ArrayDeque` / `HashMap` などすでに多相なものは型パラメータを持つので
//! 触らない。

use crate::prelude::type_param;
use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable) {
    let mut work: Vec<(SymbolId, SymbolId, usize)> = Vec::new();
    for i in 0..st.symbols.len() {
        let id = SymbolId(i as u32);
        if st.get(id).kind != SymKind::Method || st.get(id).name != "empty" {
            continue;
        }
        let s = st.get(id);
        if !s.tparams.is_empty() || !s.params.is_empty() {
            continue;
        }
        // `def empty: Coll[Any]` も `def empty(): Coll[Any]` も対象。
        let ret = match &s.ty {
            Type::Method { paramss, ret } if paramss.iter().all(|c| c.is_empty()) => {
                (**ret).clone()
            }
            Type::Class { .. } => s.ty.clone(),
            _ => continue,
        };
        let Type::Class { sym: coll, args } = &ret else {
            continue;
        };
        if args.is_empty() || !args.iter().all(|a| matches!(a, Type::Any)) {
            continue;
        }
        if st.get(*coll).tparams.len() != args.len() {
            continue;
        }
        // オーナーが `Coll` のコンパニオン（モジュールクラス）であること。
        let Some(comp) = st.companion_module(*coll) else {
            continue;
        };
        if st.module_class_of(comp) != st.get(id).owner {
            continue;
        }
        work.push((id, *coll, args.len()));
    }
    for (id, coll, arity) in work {
        let names: &[&str] = if arity == 2 { &["K", "V"] } else { &["A"] };
        let tps: Vec<SymbolId> = names.iter().map(|n| type_param(st, id, n)).collect();
        let targs: Vec<Type> = tps.iter().map(|t| Type::TypeParam(*t)).collect();
        st.get_mut(id).tparams = tps;
        st.get_mut(id).ty = Type::Method {
            paramss: vec![vec![]],
            ret: Box::new(Type::Class {
                sym: coll,
                args: targs,
            }),
        };
    }
}
