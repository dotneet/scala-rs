use crate::prelude::{class, fn1, method, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn add_rich_int_and_range(st: &mut SymbolTable) -> SymbolId {
    let range = class(
        st,
        st.scala_pkg,
        "Range",
        "scala/collection/immutable/Range",
        &[Type::AnyRef],
    );
    method(st, range, "length", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        range,
        "apply",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        range,
        "foreach",
        vec![fn1(Type::Int, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, range, "toString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        range,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    add_numeric_range(st);
    let ri = class(
        st,
        st.scala_pkg,
        "RichInt",
        "scala/runtime/RichInt",
        &[Type::AnyVal],
    );
    let f = st.alloc("self", ri, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = Type::Int;
    st.get_mut(ri).ctor_fields = vec![f];
    method(st, ri, "abs", vec![], Type::Int, Intrinsic::None);
    method(st, ri, "max", vec![Type::Int], Type::Int, Intrinsic::None);
    method(st, ri, "min", vec![Type::Int], Type::Int, Intrinsic::None);
    let range_t = Type::Class {
        sym: range,
        args: vec![],
    };
    method(
        st,
        ri,
        "to",
        vec![Type::Int],
        range_t.clone(),
        Intrinsic::None,
    );
    method(st, ri, "until", vec![Type::Int], range_t, Intrinsic::None);
    ri
}
fn add_rich_value(st: &mut SymbolTable, name: &str, jvm: &str, under: Type) -> SymbolId {
    let c = class(st, st.scala_pkg, name, jvm, &[Type::AnyVal]);
    let f = st.alloc("self", c, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = under.clone();
    st.get_mut(c).ctor_fields = vec![f];
    c
}
pub(crate) fn add_rich_long_double_char(st: &mut SymbolTable) -> (SymbolId, SymbolId, SymbolId) {
    let rl = add_rich_value(st, "RichLong", "scala/runtime/RichLong", Type::Long);
    method(st, rl, "abs", vec![], Type::Long, Intrinsic::None);
    method(st, rl, "max", vec![Type::Long], Type::Long, Intrinsic::None);
    method(st, rl, "min", vec![Type::Long], Type::Long, Intrinsic::None);
    let nr = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|&id| st.get(id).name == "NumericRange")
        .expect("NumericRange");
    let nr_l = Type::Class {
        sym: nr,
        args: vec![Type::Long],
    };
    method(
        st,
        rl,
        "to",
        vec![Type::Long],
        nr_l.clone(),
        Intrinsic::None,
    );
    method(st, rl, "until", vec![Type::Long], nr_l, Intrinsic::None);
    let rd = add_rich_value(st, "RichDouble", "scala/runtime/RichDouble", Type::Double);
    method(st, rd, "abs", vec![], Type::Double, Intrinsic::None);
    method(
        st,
        rd,
        "max",
        vec![Type::Double],
        Type::Double,
        Intrinsic::None,
    );
    method(
        st,
        rd,
        "min",
        vec![Type::Double],
        Type::Double,
        Intrinsic::None,
    );
    let rc = add_rich_value(st, "RichChar", "scala/runtime/RichChar", Type::Char);
    method(st, rc, "isDigit", vec![], Type::Boolean, Intrinsic::None);
    method(st, rc, "toInt", vec![], Type::Int, Intrinsic::None);
    let nr_c = Type::Class {
        sym: nr,
        args: vec![Type::Char],
    };
    method(
        st,
        rc,
        "to",
        vec![Type::Char],
        nr_c.clone(),
        Intrinsic::None,
    );
    method(st, rc, "until", vec![Type::Char], nr_c, Intrinsic::None);
    (rl, rd, rc)
}
pub(crate) fn add_rich_float(st: &mut SymbolTable) -> SymbolId {
    let rf = add_rich_value(st, "RichFloat", "scala/runtime/RichFloat", Type::Float);
    method(st, rf, "abs", vec![], Type::Float, Intrinsic::None);
    method(
        st,
        rf,
        "max",
        vec![Type::Float],
        Type::Float,
        Intrinsic::None,
    );
    method(
        st,
        rf,
        "min",
        vec![Type::Float],
        Type::Float,
        Intrinsic::None,
    );
    rf
}
fn add_numeric_range(st: &mut SymbolTable) -> SymbolId {
    let nr = class(
        st,
        st.scala_pkg,
        "NumericRange",
        "scala/collection/immutable/NumericRange",
        &[Type::AnyRef],
    );
    let ta = type_param(st, nr, "T");
    st.get_mut(nr).tparams = vec![ta];
    let tt = Type::TypeParam(ta);
    method(
        st,
        nr,
        "foreach",
        vec![fn1(tt.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, nr, "toString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        nr,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(st, nr, "apply", vec![Type::Int], tt, Intrinsic::None);
    nr
}
pub(crate) fn add_rich_byte_short_boolean(st: &mut SymbolTable) -> (SymbolId, SymbolId, SymbolId) {
    let rb = add_rich_value(st, "RichByte", "scala/runtime/RichByte", Type::Byte);
    method(st, rb, "abs", vec![], Type::Byte, Intrinsic::None);
    method(st, rb, "max", vec![Type::Byte], Type::Byte, Intrinsic::None);
    method(st, rb, "min", vec![Type::Byte], Type::Byte, Intrinsic::None);
    let nr = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|&id| st.get(id).name == "NumericRange")
        .expect("NumericRange");
    let nr_t = Type::Class {
        sym: nr,
        args: vec![Type::Byte],
    };
    method(
        st,
        rb,
        "to",
        vec![Type::Byte],
        nr_t.clone(),
        Intrinsic::None,
    );
    method(st, rb, "until", vec![Type::Byte], nr_t, Intrinsic::None);
    let rs = add_rich_value(st, "RichShort", "scala/runtime/RichShort", Type::Short);
    method(st, rs, "abs", vec![], Type::Short, Intrinsic::None);
    method(
        st,
        rs,
        "max",
        vec![Type::Short],
        Type::Short,
        Intrinsic::None,
    );
    method(
        st,
        rs,
        "min",
        vec![Type::Short],
        Type::Short,
        Intrinsic::None,
    );
    let nr_s = Type::Class {
        sym: nr,
        args: vec![Type::Short],
    };
    method(
        st,
        rs,
        "to",
        vec![Type::Short],
        nr_s.clone(),
        Intrinsic::None,
    );
    method(st, rs, "until", vec![Type::Short], nr_s, Intrinsic::None);
    let rbool = add_rich_value(
        st,
        "RichBoolean",
        "scala/runtime/RichBoolean",
        Type::Boolean,
    );
    method(
        st,
        rbool,
        "compare",
        vec![Type::Boolean],
        Type::Int,
        Intrinsic::None,
    );
    (rb, rs, rbool)
}
