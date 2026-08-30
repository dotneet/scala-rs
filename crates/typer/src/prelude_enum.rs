//! The three `Enumeration.Value` overloads the pickle path cannot reach.
//!
//! Everything else `object Color extends Enumeration` needs -- `values`,
//! `withName`, `apply`, `maxId`, and the whole `ValueSet` surface -- is read
//! out of `scala/Enumeration.class`'s own `ScalaSignature` by
//! `PickleSupply::complete`, now that it also asks a user class's *library
//! ancestors*. Nothing here duplicates that.
//!
//! `Value` is the one name it cannot serve. `scala.Enumeration` declares both
//! a class `Value` and four methods called `Value`:
//!
//! ```text
//! protected final def Value: Value
//! protected final def Value(i: Int): Value
//! protected final def Value(name: String): Value
//! protected final def Value(i: Int, name: String): Value
//! ```
//!
//! Completion runs only when member lookup found *nothing* (`prelude.rs`'s
//! declarations always win), and the inner class already answers to that name,
//! so the overloads are never asked for. Dropping the prelude's nullary
//! `Value` does not help either: `Value(10, "custom")` then resolves the bare
//! name to the *class* and reports `value apply is not a member of Value`.
//!
//! So the four are declared here, against `javap -p scala.Enumeration`:
//!
//! ```text
//! public final scala.Enumeration$Value Value();
//! public final scala.Enumeration$Value Value(int);
//! public final scala.Enumeration$Value Value(java.lang.String);
//! public final scala.Enumeration$Value Value(int, java.lang.String);
//! ```
//!
//! (`prelude.rs::add_enumeration` declares the nullary one and the `Value`
//! class itself; only the other three are added here.)
//!
//! `val Red, Green, Blue = Value` gives each name the next id because that is
//! what the *library* does at run time -- `Value()` reads and bumps
//! `Enumeration.nextId` -- so the multiple assignment needs no compiler magic
//! beyond evaluating the right-hand side once per name, which
//! `Check::multi_val_defs` already does.

use scala_rs_parser::Type;

use crate::symbol::{Intrinsic, SymKind, SymbolTable};

/// Add `Value(Int)`, `Value(String)` and `Value(Int, String)` to
/// `scala.Enumeration`. Library ABI only: the private runtime has no
/// `scala/Enumeration` class file at all, and `prelude.rs` only calls this
/// where `add_enumeration` ran.
pub(crate) fn install(st: &mut SymbolTable) {
    let Some(en) = find_scala_class(st, "scala/Enumeration") else {
        return;
    };
    let Some(val) = find_scala_class(st, "scala/Enumeration$Value") else {
        return;
    };
    let val_t = Type::Class {
        sym: val,
        args: vec![],
    };
    for params in [
        vec![Type::Int],
        vec![Type::String],
        vec![Type::Int, Type::String],
    ] {
        crate::prelude::prelude_method(st, en, "Value", params, val_t.clone(), Intrinsic::None);
    }
}

fn find_scala_class(st: &SymbolTable, jvm: &str) -> Option<scala_rs_parser::SymbolId> {
    st.symbols
        .iter()
        .find(|s| s.jvm_name == jvm && matches!(s.kind, SymKind::Class))
        .map(|s| s.id)
}
