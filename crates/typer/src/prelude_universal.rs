//! `Any.##` and the `java.lang.String` methods the prelude was missing.

use crate::prelude::prelude_method;
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::Type;

pub fn install(st: &mut SymbolTable) {
    let any = st.any_sym;
    if st.lookup_member(any, "##").is_empty() {
        prelude_method(st, any, "##", vec![], Type::Int, Intrinsic::AnyHash);
    }
    let string = st.string_sym;
    // `java.lang.String` members the classpath does not supply because the
    // prelude owns `String` itself.
    for (name, params, ret) in [
        (
            "replaceFirst",
            vec![Type::String, Type::String],
            Type::String,
        ),
        ("replaceAll", vec![Type::String, Type::String], Type::String),
        (
            "regionMatches",
            vec![Type::Int, Type::String, Type::Int, Type::Int],
            Type::Boolean,
        ),
        (
            "regionMatches",
            vec![Type::Boolean, Type::Int, Type::String, Type::Int, Type::Int],
            Type::Boolean,
        ),
    ] {
        let already = st.lookup_member(string, name).into_iter().any(|m| {
            matches!(&st.get(m).ty, Type::Method { paramss, .. }
                if paramss.first().map(|c| c.len()) == Some(params.len()))
        });
        if !already {
            prelude_method(st, string, name, params, ret, Intrinsic::None);
        }
    }
}
