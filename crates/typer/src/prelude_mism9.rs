//! `foreach` is polymorphic in its function's *result* in 2.13, and the
//! prelude declared it as `A => Unit`.
//!
//! `javap -s scala.collection.IterableOnceOps` (scala-library 2.13.16):
//!
//! ```text
//! public abstract <U> void foreach(scala.actor.Function1<A, U>);
//! //   ^ generic signature: <U:Ljava/lang/Object;>(Lscala/Function1<-TA;+TU;>;)V
//! ```
//!
//! Every collection's `foreach` in the prelude was written `foreach(f: A =>
//! Unit): Unit`, which is *not* what `Function1[Int, R] <: Function1[Int,
//! Unit]` needs to be true -- it is not. A lambda literal adapted to it
//! (`xs.foreach(i => i + 1)`, whose body is discarded), but a function *value*
//! did not: slick's
//!
//! ```scala
//! def foreach[R](f: Int => R): Unit = r.foreach(f)
//! ```
//!
//! was `type mismatch; found: (Int) => R  required: (Int) => Unit`, and so is
//! every `xs.foreach(f)` whose `f` was declared elsewhere.
//!
//! Rewriting each declaration in place rather than at its twenty-odd
//! definition sites keeps one statement of the rule. Only the shape the
//! prelude wrote is touched: a `foreach` that already has a type parameter, or
//! whose parameter is not a one-argument `Unit`-returning function, is left
//! alone. The erased descriptor does not change -- `U` erases to `Object` and
//! the parameter is a `Function1` either way -- so codegen is unaffected.

use scala_rs_parser::{Flags, SymbolId, Type};

use crate::symbol::{SymKind, SymbolTable};

pub(crate) fn install(st: &mut SymbolTable) {
    let ids: Vec<SymbolId> = (0..st.symbols.len())
        .map(|i| SymbolId(i as u32))
        .filter(|id| is_unit_foreach(st, *id))
        .collect();
    for id in ids {
        generalize(st, id);
    }
}

/// `def foreach(f: A => Unit): Unit`, with no type parameters of its own and
/// exactly one parameter in one clause.
fn is_unit_foreach(st: &SymbolTable, id: SymbolId) -> bool {
    let s = st.get(id);
    if s.name != "foreach" || s.kind != SymKind::Method || !s.tparams.is_empty() {
        return false;
    }
    let Type::Method { paramss, ret } = &s.ty else {
        return false;
    };
    if !matches!(ret.as_ref(), Type::Unit) {
        return false;
    }
    let [clause] = paramss.as_slice() else {
        return false;
    };
    let [Type::Function { params, ret }] = clause.as_slice() else {
        return false;
    };
    params.len() == 1 && matches!(ret.as_ref(), Type::Unit)
}

fn generalize(st: &mut SymbolTable, id: SymbolId) {
    let Type::Method { paramss, .. } = st.get(id).ty.clone() else {
        return;
    };
    let Some(Type::Function { params, .. }) = paramss.first().and_then(|c| c.first()).cloned()
    else {
        return;
    };
    let u = st.alloc("U", id, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(u).ty = Type::TypeParam(u);
    let fn_ty = Type::Function {
        params,
        ret: Box::new(Type::TypeParam(u)),
    };
    if let Some(&p) = st.get(id).params.first() {
        st.get_mut(p).ty = fn_ty.clone();
    }
    st.get_mut(id).tparams = vec![u];
    st.get_mut(id).ty = Type::Method {
        paramss: vec![vec![fn_ty]],
        ret: Box::new(Type::Unit),
    };
}
