//! `scala.util.Either` / `scala.util.Try` / `scala.Option` member signatures.
//!
//! Everything here is checked against `scala-library-2.13.16.jar` with
//! `javap -s`; the erased JVM descriptors live in `crates/backend/src/gen.rs`.
//! Members that only exist in the real library ABI are installed from
//! [`install_library_abi`]; the ones the private runtime
//! (`crates/backend/src/runtime.rs`) can back are installed from
//! [`install_option_core`] in both modes.

use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

// ---------------------------------------------------------------------------
// local copies of the prelude helpers (kept private to this module so that
// `prelude.rs` stays untouched apart from the two call sites)
// ---------------------------------------------------------------------------

fn class(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    jvm: &str,
    parents: &[Type],
) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Class, Flags::FINAL, jvm);
    st.get_mut(id).parents = parents.to_vec();
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    id
}

fn method(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    params: Vec<Type>,
    ret: Type,
    intrinsic: Intrinsic,
) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Method, Flags::FINAL, "");
    let paramss = if params.is_empty() {
        Vec::new()
    } else {
        vec![params]
    };
    st.get_mut(id).ty = Type::Method {
        paramss,
        ret: Box::new(ret),
    };
    st.get_mut(id).intrinsic = intrinsic;
    id
}

fn type_param(st: &mut SymbolTable, owner: SymbolId, name: &str) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(id).ty = Type::TypeParam(id);
    id
}

fn fn1(arg: Type, ret: Type) -> Type {
    Type::Function {
        params: vec![arg],
        ret: Box::new(ret),
    }
}

/// Prelude installation runs before any scope is pushed, so class symbols are
/// only reachable through the `scala` package's member list.
fn cls_named(st: &SymbolTable, name: &str) -> SymbolId {
    st.get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == name && st.get(*id).kind == SymKind::Class)
        .unwrap_or(SymbolId::NONE)
}

/// `java.lang.Throwable` is not a `scala` package member; `prelude::add_try`
/// stored it as the type of `Failure`'s single constructor field.
fn throwable_type(st: &SymbolTable) -> Type {
    let failure = cls_named(st, "Failure");
    if failure.is_none() {
        return Type::AnyRef;
    }
    match st.get(failure).ctor_fields.first() {
        Some(f) => st.get(*f).ty.clone(),
        None => Type::AnyRef,
    }
}

fn ty_of(sym: SymbolId, args: Vec<Type>) -> Type {
    Type::Class { sym, args }
}

fn by_name(t: Type) -> Type {
    Type::ByName(Box::new(t))
}

// ---------------------------------------------------------------------------
// Option
// ---------------------------------------------------------------------------

/// Members of `scala.Option` that the private runtime can also back.
///
/// `map` / `flatMap` / `foreach` / `withFilter` / `get` / `isEmpty` are already
/// installed by `prelude::add_option_members`; this fills in the rest.
pub fn install_option_core(st: &mut SymbolTable) {
    let o = st.option_sym;
    let a = st.get(o).tparams.first().copied().unwrap_or(SymbolId::NONE);
    if a.is_none() {
        return;
    }
    let ta = Type::TypeParam(a);
    let opt = ty_of(o, vec![ta.clone()]);

    method(st, o, "isDefined", vec![], Type::Boolean, Intrinsic::None);
    method(st, o, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        o,
        "getOrElse",
        vec![by_name(ta.clone())],
        ta.clone(),
        Intrinsic::None,
    );
    method(
        st,
        o,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        o,
        "exists",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        o,
        "forall",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        o,
        "filter",
        vec![fn1(ta.clone(), Type::Boolean)],
        opt.clone(),
        Intrinsic::None,
    );
    method(
        st,
        o,
        "filterNot",
        vec![fn1(ta.clone(), Type::Boolean)],
        opt.clone(),
        Intrinsic::None,
    );
    method(
        st,
        o,
        "orElse",
        vec![by_name(opt.clone())],
        opt,
        Intrinsic::None,
    );
    // nsc: `def fold[B](ifEmpty: => B)(f: A => B): B`
    let fold = method(st, o, "fold", vec![], Type::Unit, Intrinsic::None);
    let fb = type_param(st, fold, "B");
    let tb = Type::TypeParam(fb);
    let p0 = st.alloc("ifEmpty", fold, SymKind::Term, Flags::PARAM, "");
    st.get_mut(p0).ty = by_name(tb.clone());
    let p1 = st.alloc("f", fold, SymKind::Term, Flags::PARAM, "");
    st.get_mut(p1).ty = fn1(ta.clone(), tb.clone());
    st.get_mut(fold).tparams = vec![fb];
    st.get_mut(fold).params = vec![p0, p1];
    st.get_mut(fold).paramss = vec![vec![p0], vec![p1]];
    st.get_mut(fold).ty = Type::Method {
        paramss: vec![vec![by_name(tb.clone())], vec![fn1(ta, tb.clone())]],
        ret: Box::new(tb),
    };
}

/// `scala.Option` members that need the real library ABI: `collect` needs
/// `PartialFunction`, `toRight`/`toLeft` need `scala.util.Either`, `toList`
/// needs `scala.collection.immutable.List`, `zip` needs `Tuple2`.
fn install_option_library(st: &mut SymbolTable) {
    let o = st.option_sym;
    let a = st.get(o).tparams.first().copied().unwrap_or(SymbolId::NONE);
    if a.is_none() {
        return;
    }
    let ta = Type::TypeParam(a);
    let opt = ty_of(o, vec![ta.clone()]);
    let either = cls_named(st, "Either");
    let pf = cls_named(st, "PartialFunction");
    let tuple2 = cls_named(st, "Tuple2");
    let list = st.list_sym;

    method(
        st,
        o,
        "toList",
        vec![],
        ty_of(list, vec![ta.clone()]),
        Intrinsic::None,
    );
    if !pf.is_none() {
        method(
            st,
            o,
            "collect",
            vec![ty_of(pf, vec![ta.clone(), Type::Any])],
            opt.clone(),
            Intrinsic::None,
        );
    }
    if !tuple2.is_none() {
        method(
            st,
            o,
            "zip",
            vec![ty_of(o, vec![Type::Any])],
            ty_of(o, vec![ty_of(tuple2, vec![ta.clone(), Type::Any])]),
            Intrinsic::None,
        );
    }
    // `def flatten[B](implicit ev: A <:< Option[B]): Option[B]`. We only have
    // the erased shape, so the element type is refined in `check.rs`.
    method(st, o, "flatten", vec![], opt, Intrinsic::None);
    if either.is_none() {
        return;
    }
    // nsc: `def toRight[X](left: => X): Either[X, A]`
    let to_right = method(st, o, "toRight", vec![], Type::Unit, Intrinsic::None);
    let x = type_param(st, to_right, "X");
    let tx = Type::TypeParam(x);
    let lp = st.alloc("left", to_right, SymKind::Term, Flags::PARAM, "");
    st.get_mut(lp).ty = by_name(tx.clone());
    st.get_mut(to_right).tparams = vec![x];
    st.get_mut(to_right).params = vec![lp];
    st.get_mut(to_right).paramss = vec![vec![lp]];
    st.get_mut(to_right).ty = Type::Method {
        paramss: vec![vec![by_name(tx.clone())]],
        ret: Box::new(ty_of(either, vec![tx, ta.clone()])),
    };
    // nsc: `def toLeft[X](right: => X): Either[A, X]`
    let to_left = method(st, o, "toLeft", vec![], Type::Unit, Intrinsic::None);
    let y = type_param(st, to_left, "X");
    let tyy = Type::TypeParam(y);
    let rp = st.alloc("right", to_left, SymKind::Term, Flags::PARAM, "");
    st.get_mut(rp).ty = by_name(tyy.clone());
    st.get_mut(to_left).tparams = vec![y];
    st.get_mut(to_left).params = vec![rp];
    st.get_mut(to_left).paramss = vec![vec![rp]];
    st.get_mut(to_left).ty = Type::Method {
        paramss: vec![vec![by_name(tyy.clone())]],
        ret: Box::new(ty_of(either, vec![ta, tyy])),
    };
}

// ---------------------------------------------------------------------------
// Either
// ---------------------------------------------------------------------------

/// 2.13's `Either` is right-biased: `map` / `flatMap` / `foreach` / `getOrElse`
/// act on the `Right` value. `left` gives the `LeftProjection`.
///
/// Note: `Either` has **no** `withFilter` in 2.13.16 (checked with `javap`), so
/// a `for` comprehension over `Either` cannot carry a guard. `filterOrElse` is
/// the supported spelling.
fn install_either(st: &mut SymbolTable) {
    let either = cls_named(st, "Either");
    if either.is_none() {
        return;
    }
    let tps = st.get(either).tparams.clone();
    if tps.len() != 2 {
        return;
    }
    let (ea, eb) = (tps[0], tps[1]);
    let ta = Type::TypeParam(ea);
    let tb = Type::TypeParam(eb);
    let either_t = ty_of(either, vec![ta.clone(), tb.clone()]);
    let option = st.option_sym;
    let seq = cls_named(st, "Seq");

    method(
        st,
        either,
        "isRight",
        vec![],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        either,
        "swap",
        vec![],
        ty_of(either, vec![tb.clone(), ta.clone()]),
        Intrinsic::None,
    );
    method(
        st,
        either,
        "toOption",
        vec![],
        ty_of(option, vec![tb.clone()]),
        Intrinsic::None,
    );
    if !seq.is_none() {
        method(
            st,
            either,
            "toSeq",
            vec![],
            ty_of(seq, vec![tb.clone()]),
            Intrinsic::None,
        );
    }
    method(
        st,
        either,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        either,
        "exists",
        vec![fn1(tb.clone(), Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        either,
        "forall",
        vec![fn1(tb.clone(), Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        either,
        "foreach",
        vec![fn1(tb.clone(), Type::Any)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        either,
        "flatMap",
        vec![fn1(tb.clone(), either_t.clone())],
        either_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        either,
        "orElse",
        vec![by_name(either_t.clone())],
        either_t.clone(),
        Intrinsic::None,
    );
    // nsc: `def fold[C](fa: A => C, fb: B => C): C`
    let fold = method(st, either, "fold", vec![], Type::Unit, Intrinsic::None);
    let c = type_param(st, fold, "C");
    let tc = Type::TypeParam(c);
    let fa = st.alloc("fa", fold, SymKind::Term, Flags::PARAM, "");
    st.get_mut(fa).ty = fn1(ta.clone(), tc.clone());
    let fb = st.alloc("fb", fold, SymKind::Term, Flags::PARAM, "");
    st.get_mut(fb).ty = fn1(tb.clone(), tc.clone());
    st.get_mut(fold).tparams = vec![c];
    st.get_mut(fold).params = vec![fa, fb];
    st.get_mut(fold).paramss = vec![vec![fa, fb]];
    st.get_mut(fold).ty = Type::Method {
        paramss: vec![vec![
            fn1(ta.clone(), tc.clone()),
            fn1(tb.clone(), tc.clone()),
        ]],
        ret: Box::new(tc),
    };
    // nsc: `def filterOrElse[A1 >: A](p: B => Boolean, zero: => A1): Either[A1, B]`
    let foe = method(
        st,
        either,
        "filterOrElse",
        vec![
            fn1(tb.clone(), Type::Boolean),
            Type::ByName(Box::new(Type::Any)),
        ],
        either_t.clone(),
        Intrinsic::None,
    );
    let _ = foe;

    let lp = install_left_projection(st, either, ea, eb);
    method(
        st,
        either,
        "left",
        vec![],
        ty_of(lp, vec![ta, tb]),
        Intrinsic::None,
    );
}

/// `scala.util.Either.LeftProjection`, the left-biased view of an `Either`.
fn install_left_projection(
    st: &mut SymbolTable,
    either: SymbolId,
    _ea: SymbolId,
    _eb: SymbolId,
) -> SymbolId {
    let lp = class(
        st,
        st.scala_pkg,
        "LeftProjection",
        "scala/util/Either$LeftProjection",
        &[Type::AnyRef],
    );
    let a = type_param(st, lp, "A");
    let b = type_param(st, lp, "B");
    st.get_mut(lp).tparams = vec![a, b];
    let ta = Type::TypeParam(a);
    let tb = Type::TypeParam(b);
    let either_t = ty_of(either, vec![ta.clone(), tb.clone()]);
    let option = st.option_sym;
    let seq = cls_named(st, "Seq");

    method(st, lp, "e", vec![], either_t.clone(), Intrinsic::None);
    method(st, lp, "get", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        lp,
        "getOrElse",
        vec![by_name(ta.clone())],
        ta.clone(),
        Intrinsic::None,
    );
    method(
        st,
        lp,
        "toOption",
        vec![],
        ty_of(option, vec![ta.clone()]),
        Intrinsic::None,
    );
    if !seq.is_none() {
        method(
            st,
            lp,
            "toSeq",
            vec![],
            ty_of(seq, vec![ta.clone()]),
            Intrinsic::None,
        );
    }
    method(
        st,
        lp,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        either_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        lp,
        "flatMap",
        vec![fn1(ta.clone(), either_t.clone())],
        either_t,
        Intrinsic::None,
    );
    method(
        st,
        lp,
        "foreach",
        vec![fn1(ta.clone(), Type::Any)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        lp,
        "exists",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        lp,
        "forall",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        lp,
        "filterToOption",
        vec![fn1(ta.clone(), Type::Boolean)],
        ty_of(option, vec![ty_of(either, vec![ta, tb])]),
        Intrinsic::None,
    );
    lp
}

// ---------------------------------------------------------------------------
// Try
// ---------------------------------------------------------------------------

/// `scala.util.Try` and its `WithFilter`. `recover` / `recoverWith` / `collect`
/// take a `PartialFunction`, so a `{ case ... }` literal is accepted.
fn install_try(st: &mut SymbolTable) {
    let try_c = cls_named(st, "Try");
    if try_c.is_none() {
        return;
    }
    let tps = st.get(try_c).tparams.clone();
    if tps.len() != 1 {
        return;
    }
    let tt = tps[0];
    let t_ty = Type::TypeParam(tt);
    let try_t = ty_of(try_c, vec![t_ty.clone()]);
    let option = st.option_sym;
    let either = cls_named(st, "Either");
    let pf = cls_named(st, "PartialFunction");
    let throwable_t = throwable_type(st);

    method(
        st,
        try_c,
        "isSuccess",
        vec![],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "isFailure",
        vec![],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, try_c, "get", vec![], t_ty.clone(), Intrinsic::None);
    method(
        st,
        try_c,
        "toOption",
        vec![],
        ty_of(option, vec![t_ty.clone()]),
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "failed",
        vec![],
        ty_of(try_c, vec![throwable_t.clone()]),
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "foreach",
        vec![fn1(t_ty.clone(), Type::Any)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "flatMap",
        vec![fn1(t_ty.clone(), try_t.clone())],
        try_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "filter",
        vec![fn1(t_ty.clone(), Type::Boolean)],
        try_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "orElse",
        vec![by_name(try_t.clone())],
        try_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "transform",
        vec![
            fn1(t_ty.clone(), try_t.clone()),
            fn1(throwable_t.clone(), try_t.clone()),
        ],
        try_t.clone(),
        Intrinsic::None,
    );
    if !either.is_none() {
        method(
            st,
            try_c,
            "toEither",
            vec![],
            ty_of(either, vec![throwable_t.clone(), t_ty.clone()]),
            Intrinsic::None,
        );
    }
    if !pf.is_none() {
        method(
            st,
            try_c,
            "recover",
            vec![ty_of(pf, vec![throwable_t.clone(), Type::Any])],
            try_t.clone(),
            Intrinsic::None,
        );
        method(
            st,
            try_c,
            "recoverWith",
            vec![ty_of(pf, vec![throwable_t.clone(), try_t.clone()])],
            try_t.clone(),
            Intrinsic::None,
        );
        method(
            st,
            try_c,
            "collect",
            vec![ty_of(pf, vec![t_ty.clone(), Type::Any])],
            try_t.clone(),
            Intrinsic::None,
        );
    }
    // nsc: `def fold[U](fa: Throwable => U, fb: T => U): U`
    let fold = method(st, try_c, "fold", vec![], Type::Unit, Intrinsic::None);
    let u = type_param(st, fold, "U");
    let tu = Type::TypeParam(u);
    let fa = st.alloc("fa", fold, SymKind::Term, Flags::PARAM, "");
    st.get_mut(fa).ty = fn1(throwable_t.clone(), tu.clone());
    let fb = st.alloc("fb", fold, SymKind::Term, Flags::PARAM, "");
    st.get_mut(fb).ty = fn1(t_ty.clone(), tu.clone());
    st.get_mut(fold).tparams = vec![u];
    st.get_mut(fold).params = vec![fa, fb];
    st.get_mut(fold).paramss = vec![vec![fa, fb]];
    st.get_mut(fold).ty = Type::Method {
        paramss: vec![vec![
            fn1(throwable_t, tu.clone()),
            fn1(t_ty.clone(), tu.clone()),
        ]],
        ret: Box::new(tu),
    };

    // `Try.WithFilter` — `withFilter` returns it so a `for` comprehension with
    // a guard works. nsc: `final class WithFilter(p: T => Boolean)`.
    let wf = class(
        st,
        st.scala_pkg,
        "Try$WithFilter",
        "scala/util/Try$WithFilter",
        &[Type::AnyRef],
    );
    let wa = type_param(st, wf, "T");
    st.get_mut(wf).tparams = vec![wa];
    let twa = Type::TypeParam(wa);
    let wf_try = ty_of(try_c, vec![twa.clone()]);
    method(
        st,
        wf,
        "map",
        vec![fn1(twa.clone(), Type::Any)],
        wf_try.clone(),
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "flatMap",
        vec![fn1(twa.clone(), wf_try.clone())],
        wf_try,
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "foreach",
        vec![fn1(twa.clone(), Type::Any)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "withFilter",
        vec![fn1(twa.clone(), Type::Boolean)],
        ty_of(wf, vec![twa.clone()]),
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "withFilter",
        vec![fn1(t_ty, Type::Boolean)],
        ty_of(wf, vec![twa]),
        Intrinsic::None,
    );

    fix_success_failure_parents(st, try_c);
}

/// `prelude::add_try` gives `Success`/`Failure` their own `T` but leaves the
/// `Try[T]` parent pointing at `Try`'s own type parameter, so a member selected
/// on a `Success[Int]` loses the element type. Re-link them.
fn fix_success_failure_parents(st: &mut SymbolTable, try_c: SymbolId) {
    for name in ["Success", "Failure"] {
        let c = cls_named(st, name);
        if c.is_none() {
            continue;
        }
        let Some(tp) = st.get(c).tparams.first().copied() else {
            continue;
        };
        st.get_mut(c).parents = vec![ty_of(try_c, vec![Type::TypeParam(tp)])];
    }
}

// ---------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------

/// Installed from `prelude::install_prelude` under `library_abi`, after
/// `add_either` / `add_try` / `add_seq_and_lazylist` have run.
pub fn install_library_abi(st: &mut SymbolTable) {
    install_option_library(st);
    install_either(st);
    install_try(st);
}
