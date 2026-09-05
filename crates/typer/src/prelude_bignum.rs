use crate::prelude::{class, method, module};
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::{Flags, Type};

/// `scala.math.BigInt` + companion `BigInt$` against scala-library 2.13.16.
///
/// JVM: `BigInt$.apply(I)` / `apply(Ljava/lang/String;)` /
/// `int2bigInt(I)` (IMPLICIT) / instance `$plus` / `$times`.
pub(crate) fn add_big_int(st: &mut SymbolTable) {
    let math = crate::classpath::ensure_package(st, "scala/math");
    let cls = class(st, math, "BigInt", "scala/math/BigInt", &[Type::AnyRef]);
    let this_t = Type::Class {
        sym: cls,
        args: vec![],
    };
    method(
        st,
        cls,
        "+",
        vec![this_t.clone()],
        this_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        cls,
        "*",
        vec![this_t.clone()],
        this_t.clone(),
        Intrinsic::None,
    );
    let big_mod = module(st, math, "BigInt", "scala/math/BigInt$");
    let mcls = st.module_class_of(big_mod);
    method(
        st,
        mcls,
        "apply",
        vec![Type::Int],
        this_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        mcls,
        "apply",
        vec![Type::String],
        this_t.clone(),
        Intrinsic::None,
    );
    let conv = method(
        st,
        mcls,
        "int2bigInt",
        vec![Type::Int],
        this_t,
        Intrinsic::None,
    );
    st.get_mut(conv).flags = st.get(conv).flags.with(Flags::IMPLICIT);
    let mems = st.get(mcls).members.clone();
    st.get_mut(big_mod).members.extend(mems);
}
/// `scala.math.BigDecimal` plus its companion. Small extra.
pub(crate) fn add_big_decimal(st: &mut SymbolTable) {
    let math = crate::classpath::ensure_package(st, "scala/math");
    let cls = class(
        st,
        math,
        "BigDecimal",
        "scala/math/BigDecimal",
        &[Type::AnyRef],
    );
    let this_t = Type::Class {
        sym: cls,
        args: vec![],
    };
    method(
        st,
        cls,
        "+",
        vec![this_t.clone()],
        this_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        cls,
        "*",
        vec![this_t.clone()],
        this_t.clone(),
        Intrinsic::None,
    );
    let big_mod = module(st, math, "BigDecimal", "scala/math/BigDecimal$");
    let mcls = st.module_class_of(big_mod);
    method(
        st,
        mcls,
        "apply",
        vec![Type::Int],
        this_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        mcls,
        "apply",
        vec![Type::String],
        this_t.clone(),
        Intrinsic::None,
    );
    let mems = st.get(mcls).members.clone();
    st.get_mut(big_mod).members.extend(mems);
}
