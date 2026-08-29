//! `IterableOnceOps.toMap` — `def toMap[K, V](implicit ev: A <:< (K, V)): Map[K, V]`。
//!
//! `prelude.rs` からは [`install`] を 1 行呼ぶだけにしてある。
//!
//! `toMap` は「多相な implicit の再帰的な導出」がそのまま必要になるメンバで、
//! `ev` の witness は `<:<.refl`（`prelude_conform.rs`）ただひとつ。しかも `K`
//! と `V` は呼び出し側のどこにも現れないので、implicit 探索そのものが
//! 決めなければならない（nsc の undetermined type parameter）。
//! それを行うのが `implicits.rs::search_implicit_undet` と
//! `check.rs::adapt_implicit_apply` の undet 経路。
//!
//! JVM: `scala/collection/IterableOnceOps.toMap:(Lscala/$less$colon$less;)`
//! `Lscala/collection/immutable/Map;`（`javap -s` で確認）。invoke は
//! `crates/backend/src/gen.rs` が出す。
//!
//! **私有ランタイム（`--no-scala-library`）** では `<:<` 自体が存在しない
//! （`prelude_conform.rs` が `library_abi` でゲートしている）ので、ここも
//! 何も宣言しない。`value toMap is not a member of List[A]` の診断が出る。

use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    let Some(less) = crate::classpath::find_by_jvm(st, "scala/$less$colon$less") else {
        return;
    };
    let Some(map) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/Map") else {
        return;
    };
    let list = st.list_sym;
    add_to_map(st, list, less, map);
    if let Some(it) = crate::classpath::find_by_jvm(st, "scala/collection/Iterator") {
        add_to_map(st, it, less, map);
    }
    // Deliberately *not* on `scala.collection.Iterable`: `pickle_supply` hands
    // concrete collections (`HashMap`, `ConstArray`, …) their own pickled
    // `toMap`, and a second inherited one would make every `.toMap` an
    // unresolvable overload.
}

/// `owner[A].toMap[K, V](implicit ev: A <:< (K, V)): Map[K, V]`.
fn add_to_map(st: &mut SymbolTable, owner: SymbolId, less: SymbolId, map: SymbolId) {
    if owner.is_none() {
        return;
    }
    let Some(a) = st.get(owner).tparams.first().copied() else {
        return;
    };
    let already = st
        .get(owner)
        .members
        .iter()
        .any(|m| st.get(*m).name == "toMap");
    if already {
        return;
    }
    let m = st.alloc("toMap", owner, SymKind::Method, Flags::FINAL, "");
    let k = type_param(st, m, "K");
    let v = type_param(st, m, "V");
    let ev_ty = Type::Class {
        sym: less,
        args: vec![
            Type::TypeParam(a),
            Type::Tuple(vec![Type::TypeParam(k), Type::TypeParam(v)]),
        ],
    };
    let ev = st.alloc(
        "ev",
        m,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = ev_ty.clone();
    st.get_mut(m).tparams = vec![k, v];
    st.get_mut(m).params = vec![ev];
    st.get_mut(m).paramss = vec![vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![ev_ty]],
        ret: Box::new(Type::Class {
            sym: map,
            args: vec![Type::TypeParam(k), Type::TypeParam(v)],
        }),
    };
    st.get_mut(m).intrinsic = Intrinsic::None;
}

fn type_param(st: &mut SymbolTable, owner: SymbolId, name: &str) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(id).ty = Type::TypeParam(id);
    id
}
