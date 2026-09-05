use crate::prelude::{class, fn1, iface, method, module, module_extending, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn add_either(st: &mut SymbolTable) {
    let either = class(
        st,
        st.scala_pkg,
        "Either",
        "scala/util/Either",
        &[Type::AnyRef],
    );
    let ea = type_param(st, either, "A");
    let eb = type_param(st, either, "B");
    st.get_mut(either).tparams = vec![ea, eb];
    let tb = Type::TypeParam(eb);
    let either_t = Type::Class {
        sym: either,
        args: vec![Type::TypeParam(ea), tb.clone()],
    };
    method(st, either, "isLeft", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        either,
        "getOrElse",
        vec![Type::ByName(Box::new(Type::Any))],
        Type::Any,
        Intrinsic::None,
    );
    method(
        st,
        either,
        "map",
        vec![fn1(tb, Type::Any)],
        either_t.clone(),
        Intrinsic::None,
    );

    // nsc: `class Left[+A, +B](value: A) extends Either[A, B]`
    let left = class(
        st,
        st.scala_pkg,
        "Left",
        "scala/util/Left",
        &[either_t.clone()],
    );
    let la = type_param(st, left, "A");
    let lb = type_param(st, left, "B");
    st.get_mut(left).tparams = vec![la, lb];
    st.get_mut(left).parents = vec![Type::Class {
        sym: either,
        args: vec![Type::TypeParam(la), Type::TypeParam(lb)],
    }];
    let lf = st.alloc("value", left, SymKind::Term, Flags::FINAL, "");
    st.get_mut(lf).ty = Type::TypeParam(la);
    st.get_mut(left).ctor_fields = vec![lf];
    // The field is private in the library, so `case Left(s)` has to read it
    // through the accessor -- without this the pattern emitted a `getfield`
    // and threw `IllegalAccessError`. Same reason as `Success.value`.
    method(
        st,
        left,
        "value",
        vec![],
        Type::TypeParam(la),
        Intrinsic::None,
    );
    let left_mod = module(st, st.scala_pkg, "Left", "scala/util/Left$");
    let left_cls = st.module_class_of(left_mod);
    let left_apply = method(
        st,
        left_cls,
        "apply",
        vec![Type::Any],
        Type::Class {
            sym: left,
            args: vec![Type::TypeParam(la), Type::TypeParam(lb)],
        },
        Intrinsic::None,
    );
    st.get_mut(left_apply).tparams = vec![la, lb];
    let mems = st.get(left_cls).members.clone();
    st.get_mut(left_mod).members.extend(mems);

    // nsc: `class Right[+A, +B](value: B) extends Either[A, B]`
    let right = class(st, st.scala_pkg, "Right", "scala/util/Right", &[either_t]);
    let ra = type_param(st, right, "A");
    let rb = type_param(st, right, "B");
    st.get_mut(right).tparams = vec![ra, rb];
    st.get_mut(right).parents = vec![Type::Class {
        sym: either,
        args: vec![Type::TypeParam(ra), Type::TypeParam(rb)],
    }];
    let rf = st.alloc("value", right, SymKind::Term, Flags::FINAL, "");
    st.get_mut(rf).ty = Type::TypeParam(rb);
    st.get_mut(right).ctor_fields = vec![rf];
    method(
        st,
        right,
        "value",
        vec![],
        Type::TypeParam(rb),
        Intrinsic::None,
    );
    let right_mod = module(st, st.scala_pkg, "Right", "scala/util/Right$");
    let right_cls = st.module_class_of(right_mod);
    let right_apply = method(
        st,
        right_cls,
        "apply",
        vec![Type::Any],
        Type::Class {
            sym: right,
            args: vec![Type::TypeParam(ra), Type::TypeParam(rb)],
        },
        Intrinsic::None,
    );
    st.get_mut(right_apply).tparams = vec![ra, rb];
    let mems = st.get(right_cls).members.clone();
    st.get_mut(right_mod).members.extend(mems);
}
pub(crate) fn add_try(st: &mut SymbolTable, throwable: SymbolId) {
    let try_c = class(st, st.scala_pkg, "Try", "scala/util/Try", &[Type::AnyRef]);
    let tt = type_param(st, try_c, "T");
    st.get_mut(try_c).tparams = vec![tt];
    let t_ty = Type::TypeParam(tt);
    let try_t = Type::Class {
        sym: try_c,
        args: vec![t_ty.clone()],
    };
    method(
        st,
        try_c,
        "getOrElse",
        vec![Type::ByName(Box::new(Type::Any))],
        Type::Any,
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "map",
        vec![fn1(t_ty, Type::Any)],
        try_t.clone(),
        Intrinsic::None,
    );

    let try_mod = module(st, st.scala_pkg, "Try", "scala/util/Try$");
    let try_cls = st.module_class_of(try_mod);
    method(
        st,
        try_cls,
        "apply",
        vec![Type::ByName(Box::new(Type::Any))],
        try_t.clone(),
        Intrinsic::None,
    );
    let mems = st.get(try_cls).members.clone();
    st.get_mut(try_mod).members.extend(mems);

    let success = class(
        st,
        st.scala_pkg,
        "Success",
        "scala/util/Success",
        &[try_t.clone()],
    );
    let sa = type_param(st, success, "T");
    st.get_mut(success).tparams = vec![sa];
    let sf = st.alloc("value", success, SymKind::Term, Flags::FINAL, "");
    st.get_mut(sf).ty = Type::TypeParam(sa);
    st.get_mut(success).ctor_fields = vec![sf];
    // The field is private in the library; a pattern reads it through this.
    method(
        st,
        success,
        "value",
        vec![],
        Type::TypeParam(sa),
        Intrinsic::None,
    );
    let success_mod = module(st, st.scala_pkg, "Success", "scala/util/Success$");
    let success_cls = st.module_class_of(success_mod);
    // `def apply[T](value: T): Success[T]`. A raw `Success` conformed to
    // nothing: `def a[R](…): Try[R] = Success(f)` reported
    // `found: Success required: Try[R]`.
    let sm = method(
        st,
        success_cls,
        "apply",
        vec![Type::Any],
        Type::Any,
        Intrinsic::None,
    );
    let smt = type_param(st, sm, "T");
    st.get_mut(sm).tparams = vec![smt];
    st.get_mut(sm).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(smt)]],
        ret: Box::new(Type::Class {
            sym: success,
            args: vec![Type::TypeParam(smt)],
        }),
    };
    let mems = st.get(success_cls).members.clone();
    st.get_mut(success_mod).members.extend(mems);

    let throwable_ty = Type::Class {
        sym: throwable,
        args: vec![],
    };
    let throwable_ty2 = throwable_ty.clone();
    let failure = class(st, st.scala_pkg, "Failure", "scala/util/Failure", &[try_t]);
    let fa = type_param(st, failure, "T");
    st.get_mut(failure).tparams = vec![fa];
    let ff = st.alloc("exception", failure, SymKind::Term, Flags::FINAL, "");
    st.get_mut(ff).ty = throwable_ty.clone();
    st.get_mut(failure).ctor_fields = vec![ff];
    // The field is private in the library; a pattern reads it through this.
    method(
        st,
        failure,
        "exception",
        vec![],
        throwable_ty.clone(),
        Intrinsic::None,
    );
    let failure_mod = module(st, st.scala_pkg, "Failure", "scala/util/Failure$");
    let failure_cls = st.module_class_of(failure_mod);
    // `def apply[T](exception: Throwable): Failure[T]`. `T` appears in no
    // parameter, so only the expected type (or `Nothing`, which `Try`'s
    // covariance makes harmless) can pin it -- but a *raw* `Failure` could not
    // be pinned at all.
    let fm = method(
        st,
        failure_cls,
        "apply",
        vec![throwable_ty],
        Type::Any,
        Intrinsic::None,
    );
    let fmt = type_param(st, fm, "T");
    st.get_mut(fm).tparams = vec![fmt];
    st.get_mut(fm).ty = Type::Method {
        paramss: vec![vec![throwable_ty2]],
        ret: Box::new(Type::Class {
            sym: failure,
            args: vec![Type::TypeParam(fmt)],
        }),
    };
    let mems = st.get(failure_cls).members.clone();
    st.get_mut(failure_mod).members.extend(mems);
}
/// `scala.util.control.Breaks` / `Breaks$` against scala-library 2.13.16.
/// nsc accepts `import Breaks._`, `Breaks.breakable { ... }`, and `new Breaks`.
pub(crate) fn add_breaks(st: &mut SymbolTable) {
    let control = crate::classpath::ensure_package(st, "scala/util/control");
    let breaks = st.alloc(
        "Breaks",
        control,
        crate::symbol::SymKind::Class,
        Flags::EMPTY,
        "scala/util/control/Breaks",
    );
    st.get_mut(breaks).parents = vec![Type::AnyRef];
    st.get_mut(breaks).ty = Type::Class {
        sym: breaks,
        args: vec![],
    };
    let try_block = iface(st, breaks, "TryBlock", "scala/util/control/Breaks$TryBlock");
    let tt = type_param(st, try_block, "T");
    st.get_mut(try_block).tparams = vec![tt];
    let cb = method(
        st,
        try_block,
        "catchBreak",
        vec![Type::ByName(Box::new(Type::TypeParam(tt)))],
        Type::TypeParam(tt),
        Intrinsic::None,
    );
    st.set_jvm_name(cb, "(Lscala/Function0;)Ljava/lang/Object;");
    add_breaks_members(st, breaks, try_block);
    method(
        st,
        breaks,
        "<init>",
        vec![],
        Type::Class {
            sym: breaks,
            args: vec![],
        },
        Intrinsic::None,
    );
    let breaks_mod = module_extending(
        st,
        control,
        "Breaks",
        "scala/util/control/Breaks$",
        Type::Class {
            sym: breaks,
            args: vec![],
        },
    );
    let mcls = st.module_class_of(breaks_mod);
    add_breaks_members(st, mcls, try_block);
    let mems = st.get(mcls).members.clone();
    st.get_mut(breaks_mod).members.extend(mems);
}
fn add_breaks_members(st: &mut SymbolTable, owner: SymbolId, try_block: SymbolId) {
    method(
        st,
        owner,
        "breakable",
        vec![Type::ByName(Box::new(Type::Unit))],
        Type::Unit,
        Intrinsic::None,
    );
    let br = method(st, owner, "break", vec![], Type::Nothing, Intrinsic::None);
    st.set_jvm_name(br, "()Lscala/runtime/Nothing$;");
    // nsc 2.13.16: `def tryBreakable[T](op: => T): Breaks.TryBlock[T]`
    let tb = method(
        st,
        owner,
        "tryBreakable",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let t = type_param(st, tb, "T");
    let op = st.alloc("op", tb, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = Type::ByName(Box::new(Type::TypeParam(t)));
    st.get_mut(tb).tparams = vec![t];
    st.get_mut(tb).params = vec![op];
    st.get_mut(tb).paramss = vec![vec![op]];
    st.get_mut(tb).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::TypeParam(t)))]],
        ret: Box::new(Type::Class {
            sym: try_block,
            args: vec![Type::TypeParam(t)],
        }),
    };
    st.set_jvm_name(
        tb,
        "(Lscala/Function0;)Lscala/util/control/Breaks$TryBlock;",
    );
}
