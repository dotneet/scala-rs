//! `IterableOnceOps.toMap` -- `def toMap[K, V](implicit ev: A <:< (K, V)): Map[K, V]`.
//!
//! `prelude.rs` calls [`install`] on a single line.
//!
//! `toMap` is the member that needs "recursive derivation of a polymorphic implicit"
//! outright: the only witness for `ev` is `<:<.refl` (`prelude_conform.rs`), and `K`
//! and `V` appear nowhere on the calling side, so implicit search itself has to decide
//! them (nsc's undetermined type parameters). That is what the undet path of
//! `implicits.rs::search_implicit_undet` and `check.rs::adapt_implicit_apply` does.
//!
//! JVM: `scala/collection/IterableOnceOps.toMap:(Lscala/$less$colon$less;)`
//! `Lscala/collection/immutable/Map;` (confirmed with `javap -s`). The invoke is
//! emitted by `crates/backend/src/gen.rs`.
//!
//! Under the **private runtime (`--no-scala-library`)** `<:<` does not exist at all
//! (`prelude_conform.rs` gates it on `library_abi`), so nothing is declared here
//! either and the diagnostic `value toMap is not a member of List[A]` comes out.

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
