use crate::prelude::{method, type_param};
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::Type;

pub(crate) fn add_any_members(st: &mut SymbolTable) {
    let any = st.any_sym;
    method(
        st,
        any,
        "==",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::AnyEq,
    );
    method(
        st,
        any,
        "!=",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::AnyNe,
    );
    method(
        st,
        any,
        "equals",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, any, "hashCode", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        any,
        "toString",
        vec![],
        Type::String,
        Intrinsic::AnyToString,
    );
    // nsc `Any.asInstanceOf[T0]: T0` / `Any.isInstanceOf[T0]: Boolean` are
    // generic over the explicit type argument: `x.asInstanceOf[String]` must
    // type as `String`, not `Any`.
    let as_instance_of = method(
        st,
        any,
        "asInstanceOf",
        vec![],
        Type::Any,
        Intrinsic::AsInstanceOf,
    );
    let aio_t = type_param(st, as_instance_of, "T0");
    st.get_mut(as_instance_of).tparams = vec![aio_t];
    st.get_mut(as_instance_of).ty = Type::Method {
        paramss: Vec::new(),
        ret: Box::new(Type::TypeParam(aio_t)),
    };
    let is_instance_of = method(
        st,
        any,
        "isInstanceOf",
        vec![],
        Type::Boolean,
        Intrinsic::IsInstanceOf,
    );
    let iio_t = type_param(st, is_instance_of, "T0");
    st.get_mut(is_instance_of).tparams = vec![iio_t];
    // nsc `Any.synchronized[T0](body: => T0): T0`
    let sync = method(
        st,
        any,
        "synchronized",
        vec![Type::ByName(Box::new(Type::Any))],
        Type::Any,
        Intrinsic::Synchronized,
    );
    let t0 = type_param(st, sync, "T0");
    st.get_mut(sync).tparams = vec![t0];
    st.get_mut(sync).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::TypeParam(t0)))]],
        ret: Box::new(Type::TypeParam(t0)),
    };
    let anyref = st.anyref_sym;
    method(
        st,
        anyref,
        "eq",
        vec![Type::AnyRef],
        Type::Boolean,
        Intrinsic::Eq,
    );
    method(
        st,
        anyref,
        "ne",
        vec![Type::AnyRef],
        Type::Boolean,
        Intrinsic::Ne,
    );
}
pub(crate) fn add_int_members(st: &mut SymbolTable) {
    let c = st.int_sym;
    for (op, ic) in [
        ("+", Intrinsic::IntBin("+")),
        ("-", Intrinsic::IntBin("-")),
        ("*", Intrinsic::IntBin("*")),
        ("/", Intrinsic::IntBin("/")),
        ("%", Intrinsic::IntBin("%")),
        ("&", Intrinsic::IntBin("&")),
        ("|", Intrinsic::IntBin("|")),
        ("^", Intrinsic::IntBin("^")),
        ("<<", Intrinsic::IntBin("<<")),
        (">>", Intrinsic::IntBin(">>")),
        (">>>", Intrinsic::IntBin(">>>")),
    ] {
        method(st, c, op, vec![Type::Int], Type::Int, ic);
    }
    for (op, ic) in [
        ("==", Intrinsic::IntBin("==")),
        ("!=", Intrinsic::IntBin("!=")),
        ("<", Intrinsic::IntBin("<")),
        ("<=", Intrinsic::IntBin("<=")),
        (">", Intrinsic::IntBin(">")),
        (">=", Intrinsic::IntBin(">=")),
    ] {
        method(st, c, op, vec![Type::Int], Type::Boolean, ic);
    }
    method(st, c, "unary_-", vec![], Type::Int, Intrinsic::IntUn("-"));
    method(st, c, "unary_~", vec![], Type::Int, Intrinsic::IntUn("~"));
    method(st, c, "toLong", vec![], Type::Long, Intrinsic::IntToLong);
    method(st, c, "toFloat", vec![], Type::Float, Intrinsic::IntToFloat);
    method(
        st,
        c,
        "toDouble",
        vec![],
        Type::Double,
        Intrinsic::IntToDouble,
    );
    method(st, c, "toByte", vec![], Type::Byte, Intrinsic::IntToByte);
    method(st, c, "toShort", vec![], Type::Short, Intrinsic::IntToShort);
    method(
        st,
        c,
        "toString",
        vec![],
        Type::String,
        Intrinsic::AnyToString,
    );
    method(
        st,
        c,
        "+",
        vec![Type::Long],
        Type::Long,
        Intrinsic::LongBin("+"),
    );
    method(
        st,
        c,
        "+",
        vec![Type::Double],
        Type::Double,
        Intrinsic::DoubleBin("+"),
    );
}
pub(crate) fn add_long_members(st: &mut SymbolTable) {
    let c = st.long_sym;
    for (op, ic) in [
        ("+", Intrinsic::LongBin("+")),
        ("-", Intrinsic::LongBin("-")),
        ("*", Intrinsic::LongBin("*")),
        ("/", Intrinsic::LongBin("/")),
        ("%", Intrinsic::LongBin("%")),
    ] {
        method(st, c, op, vec![Type::Long], Type::Long, ic);
    }
    for op in ["==", "!=", "<", "<=", ">", ">="] {
        method(
            st,
            c,
            op,
            vec![Type::Long],
            Type::Boolean,
            Intrinsic::LongBin(op),
        );
    }
    method(st, c, "unary_-", vec![], Type::Long, Intrinsic::LongUn("-"));
    method(st, c, "unary_~", vec![], Type::Long, Intrinsic::LongUn("~"));
    // `Intrinsic::None` here boxed the receiver and called a `java.lang.Long`
    // method that does not exist; `l2i` is the whole of `Long.toInt`.
    method(st, c, "toInt", vec![], Type::Int, Intrinsic::NumConv("JI"));
    method(
        st,
        c,
        "toDouble",
        vec![],
        Type::Double,
        Intrinsic::LongToDouble,
    );
    method(
        st,
        c,
        "toFloat",
        vec![],
        Type::Float,
        Intrinsic::LongToFloat,
    );
}
pub(crate) fn add_double_members(st: &mut SymbolTable) {
    let c = st.double_sym;
    for (op, ic) in [
        ("+", Intrinsic::DoubleBin("+")),
        ("-", Intrinsic::DoubleBin("-")),
        ("*", Intrinsic::DoubleBin("*")),
        ("/", Intrinsic::DoubleBin("/")),
        ("%", Intrinsic::DoubleBin("%")),
    ] {
        method(st, c, op, vec![Type::Double], Type::Double, ic);
    }
    for op in ["==", "!=", "<", "<=", ">", ">="] {
        method(
            st,
            c,
            op,
            vec![Type::Double],
            Type::Boolean,
            Intrinsic::DoubleBin(op),
        );
    }
    method(
        st,
        c,
        "unary_-",
        vec![],
        Type::Double,
        Intrinsic::DoubleUn("-"),
    );
}
pub(crate) fn add_float_members(st: &mut SymbolTable) {
    let c = st.float_sym;
    for op in ["+", "-", "*", "/", "%"] {
        method(
            st,
            c,
            op,
            vec![Type::Float],
            Type::Float,
            Intrinsic::FloatBin(op),
        );
    }
    for op in ["==", "!=", "<", "<=", ">", ">="] {
        method(
            st,
            c,
            op,
            vec![Type::Float],
            Type::Boolean,
            Intrinsic::FloatBin(op),
        );
    }
    method(
        st,
        c,
        "toDouble",
        vec![],
        Type::Double,
        Intrinsic::FloatToDouble,
    );
    method(
        st,
        c,
        "unary_-",
        vec![],
        Type::Float,
        Intrinsic::FloatUn("-"),
    );
}
pub(crate) fn add_bool_members(st: &mut SymbolTable) {
    let c = st.boolean_sym;
    method(
        st,
        c,
        "&&",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("&&"),
    );
    method(
        st,
        c,
        "||",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("||"),
    );
    method(
        st,
        c,
        "unary_!",
        vec![],
        Type::Boolean,
        Intrinsic::BoolUn("!"),
    );
    method(
        st,
        c,
        "==",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("=="),
    );
    method(
        st,
        c,
        "!=",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("!="),
    );
}
pub(crate) fn add_string_members(st: &mut SymbolTable, library_abi: bool) {
    let c = st.string_sym;
    method(
        st,
        c,
        "+",
        vec![Type::Any],
        Type::String,
        Intrinsic::StringConcat,
    );
    method(
        st,
        c,
        "charAt",
        vec![Type::Int],
        Type::Char,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "concat",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    if !library_abi {
        method(st, c, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    }
    method(
        st,
        c,
        "equals",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, c, "toString", vec![], Type::String, Intrinsic::Identity);
    // nsc calls java.lang.String for these; StringOps has no $extension.
    method(
        st,
        c,
        "startsWith",
        vec![Type::String],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "endsWith",
        vec![Type::String],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "indexOf",
        vec![Type::String],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "split",
        vec![Type::String],
        Type::Array(Box::new(Type::String)),
        Intrinsic::None,
    );
    if !library_abi {
        method(st, c, "length", vec![], Type::Int, Intrinsic::None);
        // Private runtime: parseInt on String. Library mode uses StringOps via augmentString.
        method(st, c, "toInt", vec![], Type::Int, Intrinsic::StringToInt);
        method(st, c, "toLong", vec![], Type::Long, Intrinsic::StringToLong);
        method(
            st,
            c,
            "toDouble",
            vec![],
            Type::Double,
            Intrinsic::StringToDouble,
        );
    }
}
pub(crate) fn add_array_members(st: &mut SymbolTable) {
    let c = st.array_sym;
    method(st, c, "length", vec![], Type::Int, Intrinsic::None);
    method(st, c, "apply", vec![Type::Int], Type::Any, Intrinsic::None);
    method(
        st,
        c,
        "update",
        vec![Type::Int, Type::Any],
        Type::Unit,
        Intrinsic::None,
    );
    // `arr.clone()`. Every JVM array has a public `clone()`; nsc gives it the
    // receiver's own type (`def clone(): Array[T]`) and emits
    // `invokevirtual "[I".clone:()Ljava/lang/Object;` plus a `checkcast`.
    // Declared with an `Any` result like `apply`, and rewritten to the
    // receiver's type where `apply` and `update` are (`Check::type_select`).
    // An empty parameter list, not a parameterless method: `clone()` is how
    // it is written, here as in Java.
    let cl = method(st, c, "clone", vec![], Type::Any, Intrinsic::None);
    st.get_mut(cl).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Any),
    };
}
