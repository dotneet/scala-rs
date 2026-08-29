//! `scala.util.matching.Regex` as a pattern extractor.
//!
//! `val NumericPattern = "…".r` used in a `case NumericPattern(v)` is how
//! Scala matches with a regular expression, and the prelude's `Regex` had
//! `findFirstIn` and `matches` but no `unapplySeq`, so every such pattern was
//! reported as an unknown extractor.

use crate::prelude::prelude_method;
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::Type;

pub fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        // The private runtime has no `Regex`; leave it as it was so a use is
        // still diagnosed.
        return;
    }
    let Some(regex) = crate::classpath::find_by_jvm(st, "scala/util/matching/Regex") else {
        return;
    };
    if !st.lookup_member(regex, "unapplySeq").is_empty() {
        return;
    }
    let Some(list) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/List") else {
        return;
    };
    // The library's parameter is `CharSequence`, and the descriptor has to
    // say so or the call does not link.
    let cs = crate::classpath::find_or_stub_java_class(st, "java/lang/CharSequence");
    let cs_ty = Type::Class {
        sym: cs,
        args: vec![],
    };
    prelude_method(
        st,
        regex,
        "unapplySeq",
        vec![cs_ty],
        Type::Class {
            sym: st.option_sym,
            args: vec![Type::Class {
                sym: list,
                args: vec![Type::String],
            }],
        },
        Intrinsic::None,
    );
    for (name, params, ret) in [
        ("findAllIn", vec![Type::String], Type::Any),
        ("findFirstMatchIn", vec![Type::String], Type::Any),
        (
            "replaceAllIn",
            vec![Type::String, Type::String],
            Type::String,
        ),
        (
            "replaceFirstIn",
            vec![Type::String, Type::String],
            Type::String,
        ),
        (
            "split",
            vec![Type::String],
            Type::Array(Box::new(Type::String)),
        ),
    ] {
        if st.lookup_member(regex, name).is_empty() {
            prelude_method(st, regex, name, params, ret, Intrinsic::None);
        }
    }
}
