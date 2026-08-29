//! `java.lang.String`'s real shape: the interfaces it implements, and the
//! search overloads the prelude was missing.
//!
//! `prelude.rs` builds `String` with `AnyRef` as its only parent, so
//! `String <: CharSequence` was false and every JDK overload that takes a
//! `CharSequence` was inapplicable: `Instant.parse(s)`,
//! `LocalDate.parse(s, fmt)` and `DateTimeFormatter.parse(s)` all reported
//! "no matching overload". `Comparable` and `Serializable` are the other two
//! interfaces `java.lang.String` declares.
//!
//! These are JDK classes, read from `jmods` in both library modes, so nothing
//! here depends on the scala-library jar: `java.lang.String` implements them
//! whichever runtime we link against. When a class cannot be found (no JDK on
//! the classpath at all) its edge is simply not added.

use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::Type;

/// The `indexOf` / `lastIndexOf` alternatives `java.lang.String` declares
/// beyond the single `(String)Int` `prelude.rs` and `prelude_text.rs` install.
/// Without `indexOf(int)` a `Char` argument (`s.indexOf(':')`, which nsc
/// resolves by widening the `Char` to `Int`) had no alternative to match.
///
/// Like `add_string_extra`, these back onto the JDK class, which is there in
/// both library modes, so they are registered unconditionally.
pub(crate) fn install_string_search(st: &mut SymbolTable) {
    let c = st.string_sym;
    if c.is_none() {
        return;
    }
    for name in ["indexOf", "lastIndexOf"] {
        for params in [
            vec![Type::Int],
            vec![Type::Int, Type::Int],
            vec![Type::String, Type::Int],
        ] {
            crate::prelude::prelude_method(st, c, name, params, Type::Int, Intrinsic::None);
        }
    }
}

/// The interfaces `java.lang.String` implements, in declaration order.
pub(crate) const STRING_PARENTS: [&str; 3] = [
    "java/lang/Comparable",
    "java/lang/CharSequence",
    "java/io/Serializable",
];

pub(crate) fn link_string_parents(st: &mut SymbolTable) {
    let string = st.string_sym;
    if string.is_none() {
        return;
    }
    for jvm in STRING_PARENTS {
        let Some(sym) = crate::classpath::find_by_jvm(st, jvm) else {
            continue;
        };
        // `String implements Comparable<String>`; the other two take no
        // arguments.
        let args = if st.get(sym).tparams.is_empty() {
            vec![]
        } else {
            vec![Type::String]
        };
        let p = Type::Class { sym, args };
        if !st.get(string).parents.contains(&p) {
            st.get_mut(string).parents.push(p);
        }
    }
}
