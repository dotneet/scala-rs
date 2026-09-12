//! `map` on `Option`, `Try` and `Either` with its real type parameter.
//!
//! The prelude declared these as `map(f: A => Any): Option[A]` (likewise
//! `Try[T]`, `Either[A, B]`) and let `check_apply`'s `map` rule rebuild the
//! result from whatever the lambda returned. The lambda body was therefore
//! typed against an expected `Any` instead of an undetermined `B`, which is not
//! what nsc does:
//!
//! ```scala
//! final def map[B](f: A => B): Option[B]            // scala.Option
//! def map[U](f: T => U): Try[U]                     // scala.util.Try
//! def map[B1](f: B => B1): Either[A, B1]            // scala.util.Either
//! ```
//!
//! This rewrites the existing symbols in place (their JVM identity and every
//! other member stay as they are), the way `prelude_sgap::fix_option_flat_map`
//! does for `Option.flatMap`. A `map` that already has a type parameter --
//! another slice may have declared it properly at its own declaration site --
//! is left alone.

use crate::prelude::{fn1, type_param};
use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable) {
    let option = st.option_sym;
    retype_map(st, option, 0, "B");
    if let Some(try_c) = crate::classpath::find_by_jvm(st, "scala/util/Try") {
        retype_map(st, try_c, 0, "U");
        retype_try_apply(st, try_c);
    }
    if let Some(either) = crate::classpath::find_by_jvm(st, "scala/util/Either") {
        retype_map(st, either, 1, "B1");
    }
}

/// `object Try { def apply[T](r: => T): Try[T] }`. The prelude declared
/// `apply(r: => Any): Try[T]` with the *class's* `T`, so an explicit
/// `Try[Int](throw e)` had no type parameter to take the `Int` and came out
/// `Try[Nothing]` (`check_apply` reads the element off the argument).
fn retype_try_apply(st: &mut SymbolTable, try_c: SymbolId) {
    let Some(module) = crate::classpath::find_by_jvm(st, "scala/util/Try$") else {
        return;
    };
    let mcls = st.module_class_of(module);
    let byname_any = Type::ByName(Box::new(Type::Any));
    let apps: Vec<SymbolId> = [module, mcls]
        .iter()
        .flat_map(|&o| st.get(o).members.clone())
        .filter(|&m| st.get(m).name == "apply" && st.get(m).kind == SymKind::Method)
        .filter(|&m| st.get(m).tparams.is_empty())
        .filter(|&m| {
            matches!(&st.get(m).ty, Type::Method { paramss, .. }
                if paramss.len() == 1 && paramss[0] == [byname_any.clone()])
        })
        .collect();
    let Some(&m) = apps.first() else {
        return;
    };
    let t = type_param(st, m, "T");
    let r_ty = Type::ByName(Box::new(Type::TypeParam(t)));
    let r = st.alloc("r", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(r).ty = r_ty.clone();
    st.get_mut(m).tparams = vec![t];
    st.get_mut(m).params = vec![r];
    st.get_mut(m).paramss = vec![vec![r]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![r_ty]],
        ret: Box::new(Type::Class {
            sym: try_c,
            args: vec![Type::TypeParam(t)],
        }),
    };
}

/// Give `cls`'s own monomorphic `map(f: T => Any)` the signature
/// `map[X](f: T => X): C[.., X, ..]`, where `T` is `cls`'s `elem`-th type
/// parameter and `X` takes its place in the result.
fn retype_map(st: &mut SymbolTable, cls: SymbolId, elem: usize, tparam: &str) {
    let tps = st.get(cls).tparams.clone();
    let Some(&elem_tp) = tps.get(elem) else {
        return;
    };
    let Some(m) = st
        .get(cls)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).name == "map" && st.get(m).kind == SymKind::Method)
    else {
        return;
    };
    if !st.get(m).tparams.is_empty() {
        return;
    }
    // Only the prelude's own `(f: T => Any)` shape.
    let declared = fn1(Type::TypeParam(elem_tp), Type::Any);
    if !matches!(&st.get(m).ty, Type::Method { paramss, .. }
        if paramss.len() == 1 && paramss[0].len() == 1 && paramss[0][0] == declared)
    {
        return;
    }
    let x = type_param(st, m, tparam);
    let mut args: Vec<Type> = tps.iter().map(|t| Type::TypeParam(*t)).collect();
    args[elem] = Type::TypeParam(x);
    let ret = Type::Class { sym: cls, args };
    let f_ty = fn1(Type::TypeParam(elem_tp), Type::TypeParam(x));
    let f = st.alloc("f", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = f_ty.clone();
    st.get_mut(m).tparams = vec![x];
    st.get_mut(m).params = vec![f];
    st.get_mut(m).paramss = vec![vec![f]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![f_ty]],
        ret: Box::new(ret),
    };
}
