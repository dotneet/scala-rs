//! The rest of `scala.math.BigDecimal`'s companion `apply` overloads
//! (`type mismatch` slice 12).
//!
//! `add_big_decimal` declares `apply(Int)` and `apply(String)`, and
//! `prelude_oshadow` adds `apply(java.math.BigDecimal)`. A hand-written
//! prelude member declines the pickled copy of the same name (see
//! `agent/setapply`), so the *whole* set has to be written here or the missing
//! alternatives simply do not exist: slick's
//!
//! ```text
//! new ScalaNumericType[BigDecimal](BigDecimal.apply)   // Type.scala:388
//! ```
//!
//! eta-expands the companion at `Double => BigDecimal` and reported
//! `found: <overload (Int)BigDecimal | (String)BigDecimal | (BigDecimal)BigDecimal>`.
//!
//! Every signature below is `javap`'s, off the real
//! `scala-library-2.13.16.jar`. `library_abi` only: the private runtime
//! (`crates/backend/src/runtime.rs`) emits no `scala/math/BigDecimal$`, so in
//! `--no-scala-library` mode the caller must keep getting a diagnostic --
//! `prelude.rs` calls this from inside the `library_abi` branch.

use crate::prelude::method;
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::Type;

/// Add the `BigDecimal.apply` overloads the prelude does not already declare.
pub fn install(st: &mut SymbolTable) {
    let Some(cls) = crate::classpath::find_by_jvm(st, "scala/math/BigDecimal") else {
        return;
    };
    let Some(mcls) = crate::classpath::find_by_jvm(st, "scala/math/BigDecimal$") else {
        return;
    };
    let ret = Type::Class {
        sym: cls,
        args: vec![],
    };
    let mc = Type::Class {
        sym: crate::classpath::find_or_stub_java_class(st, "java/math/MathContext"),
        args: vec![],
    };
    let big_int = crate::classpath::find_by_jvm(st, "scala/math/BigInt").map(|s| Type::Class {
        sym: s,
        args: vec![],
    });
    let chars = Type::Array(Box::new(Type::Char));
    let mut sigs: Vec<Vec<Type>> = vec![
        vec![Type::Int, mc.clone()],
        vec![Type::Long],
        vec![Type::Long, mc.clone()],
        vec![Type::Long, Type::Int],
        vec![Type::Long, Type::Int, mc.clone()],
        vec![Type::Double],
        vec![Type::Double, mc.clone()],
        vec![chars.clone()],
        vec![chars, mc.clone()],
        vec![Type::String, mc.clone()],
    ];
    if let Some(bi) = big_int {
        sigs.push(vec![bi.clone()]);
        sigs.push(vec![bi.clone(), mc.clone()]);
        sigs.push(vec![bi.clone(), Type::Int]);
        sigs.push(vec![bi, Type::Int, mc]);
    }
    let module = st
        .lookup_member(st.get(mcls).owner, "BigDecimal")
        .into_iter()
        .find(|&m| st.get(m).kind == SymKind::Module);
    for params in sigs {
        let already = st.lookup_member(mcls, "apply").into_iter().any(|m| {
            matches!(&st.get(m).ty, Type::Method { paramss, .. }
                if paramss.len() == 1 && paramss[0] == params)
        });
        if already {
            continue;
        }
        let id = method(st, mcls, "apply", params, ret.clone(), Intrinsic::None);
        // `add_big_decimal` mirrors the module class's members onto the module
        // symbol so `BigDecimal.apply` resolves through either; keep that in
        // step, exactly as `prelude_oshadow` does.
        if let Some(m) = module {
            if !st.get(m).members.contains(&id) {
                st.get_mut(m).members.push(id);
            }
        }
    }
}
