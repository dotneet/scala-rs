//! `scala.util.matching.Regex` as a pattern extractor.
//!
//! `val NumericPattern = "…".r` used in a `case NumericPattern(v)` is how
//! Scala matches with a regular expression, and the prelude's `Regex` had
//! `findFirstIn` and `matches` but no `unapplySeq`, so every such pattern was
//! reported as an unknown extractor.
//!
//! `unapplySeq` is all this installs. `findAllIn`, `findFirstMatchIn`,
//! `replaceAllIn`, `replaceFirstIn` and `split` were declared here too, as a
//! fallback -- and the fallback was what every call actually got, because
//! `lookup_member` only sees a jar member that something has already asked
//! for, so the guard `is_empty()` was always true at install time. Two things
//! followed: `findAllIn` / `findFirstMatchIn` answered `Any`
//! (`value map is not a member of Any` on slick's
//! `MysqlCustomProperties.findFirstMatchIn(url).map(…)`), and the ones that
//! did have a usable result type took `String` where the library takes
//! `CharSequence`, so the call compiled to a descriptor that does not link
//! (`NoSuchMethodError: Regex.replaceAllIn(String, String)`). The pickle
//! supplies every one of them with the real signature on demand, so the
//! honest thing is to declare none of them; a name the pickle cannot supply
//! is then reported as not a member of `Regex` rather than silently given a
//! wrong type.

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
}
