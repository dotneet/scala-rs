//! `compare` on the numeric `Rich*` wrappers.
//!
//! `3.compare(4)` is `Predef.intWrapper(3).compare(4)` in nsc: `RichInt` is a
//! `ScalaWholeNumberProxy[Int]`, which is an `OrderedProxy[Int]`, which is an
//! `Ordered[Int]`. Without a `compare` on `RichInt` the view search fell
//! through to `Ordered.orderingToOrdered`, whose conversion the backend never
//! materialised: slick's `def lengthCompare(n: Int) = length.compare(n)`
//! (`slick/util/ConstArray.scala`) came out as `checkcast scala/math/Ordered`
//! applied to an `int`, and the JVM rejected the method ("Bad type on operand
//! stack"). `RichBoolean.compare` was already declared here for the same
//! reason.
//!
//! The backend emits these as the matching `java.lang.X.compare(x, y)` static,
//! which is what `OrderedProxy.compare` computes anyway (`invoke_value_extension`
//! in `crates/backend/src/gen.rs`).

use scala_rs_parser::Type;

use crate::symbol::SymbolTable;

/// Declare `compare(that: T): Int` on `RichByte` / `RichShort` / `RichInt` /
/// `RichLong` / `RichFloat` / `RichDouble` / `RichChar`.
pub(crate) fn install_rich_compare(st: &mut SymbolTable) {
    for (jvm, under) in [
        ("scala/runtime/RichByte", Type::Byte),
        ("scala/runtime/RichShort", Type::Short),
        ("scala/runtime/RichInt", Type::Int),
        ("scala/runtime/RichLong", Type::Long),
        ("scala/runtime/RichFloat", Type::Float),
        ("scala/runtime/RichDouble", Type::Double),
        ("scala/runtime/RichChar", Type::Char),
    ] {
        let Some(cls) = crate::classpath::find_by_jvm(st, jvm) else {
            continue;
        };
        if st
            .get(cls)
            .members
            .iter()
            .any(|&m| st.get(m).name == "compare")
        {
            continue;
        }
        crate::prelude::method(
            st,
            cls,
            "compare",
            vec![under],
            Type::Int,
            crate::symbol::Intrinsic::None,
        );
    }
}
