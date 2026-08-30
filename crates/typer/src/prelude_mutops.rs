//! `++=`, `-=` and `--=` on the mutable collections.
//!
//! The prelude declared `+=` on each of them but not the rest of `Growable`
//! and `Shrinkable`, so `replace ++= xs` reported that `++=` is not a member
//! and then, because `++=` looks like an assignment, that the receiver is not
//! assignable.

use crate::prelude::prelude_method;
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::{SymbolId, Type};

/// Every mutable collection the prelude declares `+=` on.
const GROWABLE: &[&str] = &[
    "scala/collection/mutable/HashMap",
    "scala/collection/mutable/HashSet",
    "scala/collection/mutable/LinkedHashMap",
    "scala/collection/mutable/LinkedHashSet",
    "scala/collection/mutable/ArrayBuffer",
    "scala/collection/mutable/ListBuffer",
    "scala/collection/mutable/ArrayDeque",
    "scala/collection/mutable/Set",
    "scala/collection/mutable/Map",
    // Declared by `prelude_mutcoll`; they reach `Growable` / `Shrinkable` the
    // same way, and the generic `scala/collection/mutable/` arm in
    // `gen.rs` already emits the interface call for all three operators.
    "scala/collection/mutable/Queue",
    "scala/collection/mutable/Stack",
    "scala/collection/mutable/TreeMap",
    "scala/collection/mutable/TreeSet",
    "scala/collection/mutable/PriorityQueue",
];

pub fn install(st: &mut SymbolTable) {
    for jvm in GROWABLE {
        let Some(cls) = crate::classpath::find_by_jvm(st, jvm) else {
            continue;
        };
        let self_ty = self_type_of(st, cls);
        // `++=` and `--=` take any `IterableOnce`; the element type is checked
        // by the collection's own `+=`, which the prelude already declares.
        for name in ["++=", "--="] {
            if has_member(st, cls, name) {
                continue;
            }
            prelude_method(
                st,
                cls,
                name,
                vec![Type::Any],
                self_ty.clone(),
                Intrinsic::None,
            );
        }
        if !has_member(st, cls, "-=") {
            prelude_method(
                st,
                cls,
                "-=",
                vec![Type::Any],
                self_ty.clone(),
                Intrinsic::None,
            );
        }
    }
}

/// The collection applied to its own type parameters, which is what these
/// methods return (`this.type` in the library).
fn self_type_of(st: &SymbolTable, cls: SymbolId) -> Type {
    Type::Class {
        sym: cls,
        args: st
            .get(cls)
            .tparams
            .iter()
            .map(|t| Type::TypeParam(*t))
            .collect(),
    }
}

fn has_member(st: &SymbolTable, owner: SymbolId, name: &str) -> bool {
    !st.lookup_member(owner, name).is_empty()
}
