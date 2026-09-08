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
    install_ordering_family(st, list, elem);
}

/// `sorted` / `min` / `max` / `sum` / `product` on `List`.
///
/// nsc declares all five with a lower-bounded type parameter that occurs only
/// in the implicit clause:
///
/// ```text
/// def sorted[B >: A](implicit ord: Ordering[B]): C
/// def max[B >: A](implicit ord: Ordering[B]): A
/// def sum[B >: A](implicit num: Numeric[B]): B
/// ```
///
/// `prelude_seq`'s hand-written versions dropped the parameter and typed the
/// evidence as `Ordering[A]` / `Numeric[A]`, so `xs.sorted(AA.toOrdering)` with
/// `AA >: A` — cats writes it six times — was rejected with `found:
/// Ordering[AA] required: Ordering[A]`. Every other collection reaches these
/// through the pickle, which now keeps the parameter; `List` is the one class
/// whose declaration is ours.
///
/// Erasure is unchanged. `Ordering[B]` and `Ordering[A]` both erase to
/// `Lscala/math/Ordering;`, and `sum`'s result goes from `A` to `B`, which are
/// both `Ljava/lang/Object;`.
///
/// Under `--no-scala-library` none of the five is declared at all (see
/// `prelude_seq`'s `add_list_core_private`), so this is a no-op there.
fn install_ordering_family(st: &mut SymbolTable, list: SymbolId, elem: SymbolId) {
    let ordering = crate::classpath::find_by_jvm(st, "scala/math/Ordering");
    let numeric = crate::classpath::find_by_jvm(st, "scala/math/Numeric");
    let list_a = Type::Class {
        sym: list,
        args: vec![Type::TypeParam(elem)],
    };
    // (name, evidence class, result)
    let mut jobs: Vec<(&str, SymbolId, Ret)> = Vec::new();
    if let Some(ord) = ordering {
        jobs.push(("sorted", ord, Ret::Fixed(list_a)));
        jobs.push(("min", ord, Ret::Elem));
        jobs.push(("max", ord, Ret::Elem));
    }
    if let Some(num) = numeric {
        jobs.push(("sum", num, Ret::Widened));
        jobs.push(("product", num, Ret::Widened));
    }
    for (name, evidence, ret) in jobs {
        for m in members_named(st, list, name) {
            // Only the nullary-plus-one-implicit-clause shape is ours to
            // rewrite; anything else under this name was declared elsewhere.
            let Type::Method { paramss, .. } = st.get(m).ty.clone() else {
                continue;
            };
            if paramss.len() != 1 || paramss[0].len() != 1 {
                continue;
            }
            let b = add_lower_bounded_tparam(st, m, "B", Type::TypeParam(elem));
            let ev = Type::Class {
                sym: evidence,
                args: vec![Type::TypeParam(b)],
            };
            let result = match &ret {
                Ret::Fixed(t) => t.clone(),
                Ret::Elem => Type::TypeParam(elem),
                Ret::Widened => Type::TypeParam(b),
            };
            set_single_implicit_param(st, m, ev.clone(), result.clone());
        }
    }
}

/// What the rewritten member returns: the receiver's own collection, its
/// element type, or the widened `B`.
enum Ret {
    Fixed(Type),
    Elem,
    Widened,
}

/// Replace `method`'s parameters with one implicit clause holding a single
/// parameter of type `ev`, and its result with `ret`.
///
/// The parameter *symbols* carry `Flags::IMPLICIT`; implicit search reads them
/// rather than the `Type::Method`, so rewriting only the type would leave the
/// evidence looking like an ordinary argument.
fn set_single_implicit_param(st: &mut SymbolTable, method: SymbolId, ev: Type, ret: Type) {
    let p = st.alloc(
        "evidence$1",
        method,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(p).ty = ev.clone();
    st.get_mut(method).params = vec![p];
    st.get_mut(method).paramss = vec![vec![p]];
    st.get_mut(method).ty = Type::Method {
        paramss: vec![vec![ev]],
        ret: Box::new(ret),
    };
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
