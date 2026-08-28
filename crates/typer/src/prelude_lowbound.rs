//! Lower-bounded (`[B >: A]`) signatures for prelude members.
//!
//! Scala 2.13's immutable collections widen their element type on the "add one
//! element" operations: `def ::[B >: A](elem: B): List[B]`. Declaring the bound
//! here (rather than typing the parameter as `Any`) lets
//! `Typer::infer_method_tparams_in` join the argument type with the receiver's
//! element type, so `Circle(1) :: Rect(2, 3) :: Nil` is a `List[Shape]` instead
//! of a `List[Circle]`.
//!
//! Erasure is unchanged: `(B)List[B]` and the previous `(Any)List[A]` both
//! erase to `(Ljava/lang/Object;)Lscala/collection/immutable/List;`, so neither
//! the private runtime nor the real scala-library ABI is affected.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable) {
    let list = st.list_sym;
    let Some(elem) = st.get(list).tparams.first().copied() else {
        return;
    };
    let list_of = |b: SymbolId| Type::Class {
        sym: list,
        args: vec![Type::TypeParam(b)],
    };
    for m in members_named(st, list, "::") {
        let b = add_lower_bounded_tparam(st, m, "B", Type::TypeParam(elem));
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![Type::TypeParam(b)]],
            ret: Box::new(list_of(b)),
        };
    }
}

fn members_named(st: &SymbolTable, owner: SymbolId, name: &str) -> Vec<SymbolId> {
    st.get(owner)
        .members
        .iter()
        .copied()
        .filter(|&m| st.get(m).kind == SymKind::Method && st.get(m).name == name)
        .collect()
}

/// Give `method` a single type parameter `name` with `>: lo`, replacing any it
/// already had.
fn add_lower_bounded_tparam(
    st: &mut SymbolTable,
    method: SymbolId,
    name: &str,
    lo: Type,
) -> SymbolId {
    let b = st.alloc(name, method, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(b).ty = Type::TypeParam(b);
    st.get_mut(b).bound_lo = Some(lo);
    st.get_mut(method).tparams = vec![b];
    b
}
