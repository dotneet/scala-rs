//! `FunctionN.tupled` / `FunctionN.curried` and `scala.Function.untupled`.
//!
//! nsc declares
//! `trait FunctionN[-T1, …, -Tn, +R] { def tupled: ((T1, …, Tn)) => R;
//!  def curried: T1 => … => R }` for `n = 2…22`, and
//! `object Function { def untupled[T1, …, Tn, R](f: ((T1, …, Tn)) => R): (T1, …, Tn) => R }`
//! for `n = 2…5`. slick's `lifted/CompilableFunctions.scala` builds every
//! arity's `CompiledFunction` out of `f.tupled`, so all 21 arities are needed.
//!
//! JVM (scala-library 2.13.16):
//! * `scala/FunctionN.tupled()Lscala/Function1;` — an interface default method,
//! * `scala/FunctionN.curried()Lscala/Function1;` — likewise,
//! * `scala/Function$.untupled(Lscala/Function1;)Lscala/FunctionN;` — the four
//!   overloads differ only in their return type, which the erased signature of
//!   each symbol already carries.
//!
//! Everything here is `library_abi`-only. The private runtime
//! (`crates/backend/src/runtime.rs`) emits `scala/Function0` and
//! `scala/Function1` with nothing but `apply`, and no `scala/Function$` at all,
//! so under `--no-scala-library` these members stay undeclared and `f.tupled`
//! is reported as "value tupled is not a member of (Int, Int) => Int" rather
//! than compiled into a call that does not exist.

use crate::prelude::{iface, method, module, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// Highest arity scala-library defines.
const MAX_ARITY: usize = 22;

/// Highest arity `scala.Function.untupled` covers in nsc.
const MAX_UNTUPLED: usize = 5;

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    for n in 0..=MAX_ARITY {
        let fun = function_class(st, n);
        if n >= 2 {
            add_tupled_and_curried(st, fun, n);
        }
    }
    add_function_module(st);
}

/// `scala.FunctionN`, reusing the one `prelude.rs` already made for
/// `n <= 4`. Its type parameters are `T1 … Tn, R` in that order: member types
/// are substituted positionally from the receiver's parameter types followed
/// by its result type (`Typer::type_select`).
fn function_class(st: &mut SymbolTable, n: usize) -> SymbolId {
    let jvm = format!("scala/Function{n}");
    let id = match crate::classpath::find_by_jvm(st, &jvm) {
        Some(id) => id,
        None => iface(st, st.scala_pkg, &format!("Function{n}"), &jvm),
    };
    if st.get(id).tparams.is_empty() {
        let mut tps = Vec::with_capacity(n + 1);
        for i in 1..=n {
            tps.push(type_param(st, id, &format!("T{i}")));
        }
        tps.push(type_param(st, id, "R"));
        st.get_mut(id).tparams = tps;
    }
    id
}

/// The `T1 … Tn` and `R` of `fun`, as types.
fn tparam_types(st: &SymbolTable, fun: SymbolId) -> (Vec<Type>, Type) {
    let tps = st.get(fun).tparams.clone();
    let (args, ret) = tps.split_at(tps.len() - 1);
    (
        args.iter().map(|&t| Type::TypeParam(t)).collect(),
        Type::TypeParam(ret[0]),
    )
}

fn add_tupled_and_curried(st: &mut SymbolTable, fun: SymbolId, n: usize) {
    let (args, ret) = tparam_types(st, fun);
    if args.len() != n {
        // A `FunctionN` completed from the classpath with an unexpected shape:
        // adding members whose substitution would be wrong is worse than
        // leaving `tupled` undeclared.
        return;
    }
    if !members_named(st, fun, "tupled").is_empty() {
        return;
    }
    let tupled_ty = Type::Function {
        params: vec![Type::Tuple(args.clone())],
        ret: Box::new(ret.clone()),
    };
    let t = method(st, fun, "tupled", Vec::new(), tupled_ty, Intrinsic::None);
    st.get_mut(t).flags = Flags::EMPTY;

    // `curried` nests right-to-left: `T1 => (T2 => (… => R))`.
    let mut curried_ty = ret;
    for a in args.into_iter().rev() {
        curried_ty = Type::Function {
            params: vec![a],
            ret: Box::new(curried_ty),
        };
    }
    let c = method(st, fun, "curried", Vec::new(), curried_ty, Intrinsic::None);
    st.get_mut(c).flags = Flags::EMPTY;
}

/// `object Function` with nsc's four `untupled` overloads. They share a name
/// and an erased parameter list (`(Lscala/Function1;)`) and differ only in the
/// return type, which `method_desc` derives from each symbol's own result.
fn add_function_module(st: &mut SymbolTable) {
    if st
        .get(st.scala_pkg)
        .members
        .iter()
        .any(|&m| st.get(m).name == "Function" && st.get(m).kind == SymKind::Module)
    {
        return;
    }
    let f_mod = module(st, st.scala_pkg, "Function", "scala/Function$");
    let cls = st.module_class_of(f_mod);
    for n in 2..=MAX_UNTUPLED {
        let m = st.alloc("untupled", cls, SymKind::Method, Flags::EMPTY, "");
        let mut tps = Vec::with_capacity(n + 1);
        for i in 1..=n {
            tps.push(type_param(st, m, &format!("T{i}")));
        }
        tps.push(type_param(st, m, "R"));
        st.get_mut(m).tparams = tps.clone();
        let args: Vec<Type> = tps[..n].iter().map(|&t| Type::TypeParam(t)).collect();
        let ret = Type::TypeParam(tps[n]);
        let param = Type::Function {
            params: vec![Type::Tuple(args.clone())],
            ret: Box::new(ret.clone()),
        };
        // The result is an n-ary function type, so it erases to
        // `scala/FunctionN` -- which is the only thing that tells this
        // overload's JVM signature from its siblings'.
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![param]],
            ret: Box::new(Type::Function {
                params: args,
                ret: Box::new(ret),
            }),
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
