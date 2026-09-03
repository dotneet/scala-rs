//! `Either.getOrElse` / `Try.getOrElse` widened to the `[B1 >: B]` shape nsc
//! declares.
//!
//! `prelude::add_either` and `prelude::add_try` both wrote the parameter and
//! the result as plain `Any`:
//!
//! ```text
//! method(st, either, "getOrElse", vec![ByName(Any)], Any, …)
//! ```
//!
//! which is not a widening -- it *erases* the result. Every use of the value
//! then fails on its own member: slick's
//!
//! ```scala
//! val prit = inv.results(…)(ctx.session).getOrElse(throw new NoSuchElementException)
//! prit.map(v => new Mutator(v, prit.pr, inv))
//! ```
//!
//! (`slick/jdbc/JdbcActionComponent.scala`) reported three separate `… is not
//! a member of Any` -- one per use -- from one wrong signature. nsc has
//!
//! ```scala
//! def getOrElse[B1 >: B](or: => B1): B1   // scala.util.Either
//! def getOrElse[U >: T](default: => U): U // scala.util.Try
//! ```
//!
//! and `Infer::infer_method_tparams_in` already joins an argument type with a
//! declared lower bound (`prelude_ovl3` widened `Option.getOrElse` the same
//! way), so stating the bound is the whole fix. Erasure is unchanged: a type
//! parameter bounded by nothing still erases to `Object`, which is what `Any`
//! erased to.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable) {
    // `Either[A, B]` widens on its *second* parameter: 2.13's `Either` is
    // right-biased.
    widen_get_or_else(st, "Either", 1);
    widen_get_or_else(st, "Try", 0);
}

/// Rewrite `owner.getOrElse` from `(=> Any): Any` to `[B1 >: T](=> B1): B1`,
/// where `T` is the owner's `tparam_index`-th type parameter.
///
/// Only the monomorphic shape this module exists to fix is touched: a
/// signature already read from a pickle, or already widened, is left alone.
fn widen_get_or_else(st: &mut SymbolTable, class_name: &str, tparam_index: usize) {
    let owner = class_named(st, class_name);
    if owner.is_none() {
        return;
    }
    let Some(&tp) = st.get(owner).tparams.get(tparam_index) else {
        return;
    };
    let elem = Type::TypeParam(tp);
    for m in members_named(st, owner, "getOrElse") {
        let Type::Method { paramss, ret } = st.get(m).ty.clone() else {
            continue;
        };
        if paramss.len() != 1 || paramss[0].len() != 1 || *ret != Type::Any {
            continue;
        }
        if !matches!(&paramss[0][0], Type::ByName(t) if **t == Type::Any) {
            continue;
        }
        let b1 = add_lower_bounded_tparam(st, m, "B1", elem.clone());
        let tb1 = Type::TypeParam(b1);
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![Type::ByName(Box::new(tb1.clone()))]],
            ret: Box::new(tb1),
        };
    }
}

fn class_named(st: &SymbolTable, name: &str) -> SymbolId {
    st.get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).kind == SymKind::Class && st.get(m).name == name)
        .unwrap_or(SymbolId::NONE)
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
/// already had. (Same shape as `prelude_ovl3`'s helper; kept local so the two
/// slices do not have to share a file.)
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
