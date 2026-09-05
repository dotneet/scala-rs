use crate::prelude::{class, fn1, iface, method, type_param};
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::{SymbolId, Type};

/// `class WithFilter[+A, +CC[_]]`, as 2.13 declares it.
///
/// `CC` is a type *constructor*: `map[B](f: A => B): CC[B]` is what makes
/// `for (x <- xs if p) yield x.toString` a `List[String]`. Holding the
/// filtered collection whole (`CC = List[A]`, `map: CC`) made every guarded
/// comprehension keep the element type it started with.
pub(crate) fn add_with_filter(st: &mut SymbolTable) -> SymbolId {
    let wf = class(
        st,
        st.scala_pkg,
        "WithFilter",
        "scala/collection/WithFilter",
        &[Type::AnyRef],
    );
    let a = type_param(st, wf, "A");
    let cc = type_param(st, wf, "CC");
    let cc_x = type_param(st, cc, "X");
    st.get_mut(cc).tparams = vec![cc_x];
    st.get_mut(wf).tparams = vec![a, cc];
    let ta = Type::TypeParam(a);
    let tcc = Type::TypeParam(cc);
    let applied = |arg: Type| Type::Applied {
        ctor: Box::new(tcc.clone()),
        args: vec![arg],
    };
    let m = method(
        st,
        wf,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        Type::Any,
        Intrinsic::None,
    );
    let b = type_param(st, m, "B");
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), Type::TypeParam(b))]],
        ret: Box::new(applied(Type::TypeParam(b))),
    };
    let fm = method(
        st,
        wf,
        "flatMap",
        vec![fn1(ta.clone(), Type::Any)],
        Type::Any,
        Intrinsic::None,
    );
    let fb = type_param(st, fm, "B");
    st.get_mut(fm).tparams = vec![fb];
    st.get_mut(fm).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), applied(Type::TypeParam(fb)))]],
        ret: Box::new(applied(Type::TypeParam(fb))),
    };
    method(
        st,
        wf,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "withFilter",
        vec![fn1(ta, Type::Boolean)],
        Type::Class {
            sym: wf,
            args: vec![Type::TypeParam(a), tcc],
        },
        Intrinsic::None,
    );
    wf
}
pub(crate) fn add_option_with_filter(st: &mut SymbolTable) -> SymbolId {
    let wf = class(
        st,
        st.scala_pkg,
        "Option$WithFilter",
        "scala/Option$WithFilter",
        &[Type::AnyRef],
    );
    let a = type_param(st, wf, "A");
    st.get_mut(wf).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let opt = Type::Class {
        sym: st.option_sym,
        args: vec![ta.clone()],
    };
    // `def map[B](f: A => B): Option[B]` -- the element type is what `f`
    // returns, not what the filter was applied to.
    let m = method(
        st,
        wf,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        Type::Any,
        Intrinsic::None,
    );
    let mb = type_param(st, m, "B");
    st.get_mut(m).tparams = vec![mb];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), Type::TypeParam(mb))]],
        ret: Box::new(Type::Class {
            sym: st.option_sym,
            args: vec![Type::TypeParam(mb)],
        }),
    };
    let fm = method(
        st,
        wf,
        "flatMap",
        vec![fn1(ta.clone(), opt.clone())],
        Type::Any,
        Intrinsic::None,
    );
    let fb = type_param(st, fm, "B");
    st.get_mut(fm).tparams = vec![fb];
    let opt_b = Type::Class {
        sym: st.option_sym,
        args: vec![Type::TypeParam(fb)],
    };
    st.get_mut(fm).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), opt_b.clone())]],
        ret: Box::new(opt_b),
    };
    let _ = opt;
    method(
        st,
        wf,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "withFilter",
        vec![fn1(ta, Type::Boolean)],
        Type::Class {
            sym: wf,
            args: vec![Type::TypeParam(a)],
        },
        Intrinsic::None,
    );
    wf
}
pub(crate) fn add_iterator(st: &mut SymbolTable) -> SymbolId {
    let it = iface(st, st.scala_pkg, "Iterator", "scala/collection/Iterator");
    let a = type_param(st, it, "A");
    st.get_mut(it).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let it_t = Type::Class {
        sym: it,
        args: vec![ta.clone()],
    };
    method(st, it, "hasNext", vec![], Type::Boolean, Intrinsic::None);
    method(st, it, "next", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        it,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        it,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        it_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        it,
        "filter",
        vec![fn1(ta.clone(), Type::Boolean)],
        it_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        it,
        "withFilter",
        vec![fn1(ta, Type::Boolean)],
        it_t,
        Intrinsic::None,
    );
    it
}
