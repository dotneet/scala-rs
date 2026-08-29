//! `scala.math.BigDecimal.apply(java.math.BigDecimal)` (agent/overloadshadow).
//!
//! The real companion declares
//!
//! ```text
//! def apply(bd: java.math.BigDecimal): BigDecimal
//! ```
//!
//! and it is how JDBC results become Scala values: slick's
//! `PositionedResult.nextBigDecimal` is `BigDecimal(rs.getBigDecimal(i))`.
//!
//! `PickleSupply` can supply it, but only once *something else* has put
//! `java.math.BigDecimal` into the symbol table -- a pickled parameter type it
//! cannot name makes the whole overload ineligible. That is true for every
//! program that actually calls it (naming the argument's type loads the
//! class), but it left the companion's shape depending on load order, which is
//! precisely what this branch is about. Declaring it in the prelude pins it.
//!
//! `library_abi` only: the private runtime (`crates/backend/src/runtime.rs`)
//! emits no `scala/math/BigDecimal$`, so in `--no-scala-library` mode there is
//! no method to call and the caller must keep getting a diagnostic.

use crate::prelude::method;
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::Type;

/// Add `BigDecimal.apply(java.math.BigDecimal)` to the companion, if the
/// prelude built the companion and it is not there yet.
pub fn install(st: &mut SymbolTable) {
    let Some(cls) = crate::classpath::find_by_jvm(st, "scala/math/BigDecimal") else {
        return;
    };
    let Some(mcls) = crate::classpath::find_by_jvm(st, "scala/math/BigDecimal$") else {
        return;
    };
    let jbd = crate::classpath::find_or_stub_java_class(st, "java/math/BigDecimal");
    let param = Type::Class {
        sym: jbd,
        args: vec![],
    };
    let already = st.lookup_member(mcls, "apply").into_iter().any(|m| {
        matches!(&st.get(m).ty, Type::Method { paramss, .. }
            if paramss.len() == 1 && paramss[0] == vec![param.clone()])
    });
    if already {
        return;
    }
    let ret = Type::Class {
        sym: cls,
        args: vec![],
    };
    let id = method(st, mcls, "apply", vec![param], ret, Intrinsic::None);
    // `add_big_decimal` mirrors the module class's members onto the module
    // symbol so `BigDecimal.apply` resolves through either; keep that in step.
    let module = st
        .lookup_member(st.get(mcls).owner, "BigDecimal")
        .into_iter()
        .find(|&m| st.get(m).kind == SymKind::Module);
    if let Some(m) = module {
        if !st.get(m).members.contains(&id) {
            st.get_mut(m).members.push(id);
        }
    }
}
