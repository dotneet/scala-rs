//! `StringOps.map[B](f: Char => B): IndexedSeq[B]`.
//!
//! `prelude.rs` calls [`install`] on a single line.
//!
//! 2.13's `StringOps` has two `map`s. javap 2.13.16:
//!
//! ```text
//! public static java.lang.String map$extension(java.lang.String, scala.Function1);
//! public static <B> scala.collection.immutable.IndexedSeq<B>
//!     map$extension(java.lang.String, scala.Function1);
//! ```
//!
//! Two methods differing only in return type, and distinct as JVM descriptors. The
//! prelude side is right to hold them as **two symbols** (folding them into one makes
//! `value_extension_desc`, which builds the descriptor from the symbol's result type,
//! call the `IndexedSeq`-returning one even for `Char => Char` and throw
//! `ClassCastException`).
//!
//! With both present, overload resolution settles it the same way nsc does, by taking
//! the more specific one: `Char => Char` is applicable to `Char => B` but not the
//! other way round, so the `String` version wins exactly when the lambda returns
//! `Char`. The case where both are applicable because the function literal's result
//! type is not yet known is handled by `is_as_specific_method` in `check.rs`.
//!
//! The **private runtime (`--no-scala-library`)** has no `StringOps` at all, so this
//! is installed only under `library_abi`.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    let Some(so) = find_string_ops(st) else {
        return;
    };
    let Some(idx) = find_indexed_seq(st) else {
        return;
    };
    // `map(Char => Char): String` is installed by prelude.rs. This is the second one under the same name.
    if st
        .lookup_member(so, "map")
        .into_iter()
        .any(|m| !st.get(m).tparams.is_empty())
    {
        return;
    }
    let id = st.alloc("map", so, SymKind::Method, Flags::FINAL, "");
    let b = st.alloc("B", id, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(b).ty = Type::TypeParam(b);
    st.get_mut(id).tparams = vec![b];
    let tb = Type::TypeParam(b);
    st.get_mut(id).ty = Type::Method {
        paramss: vec![vec![Type::Function {
            params: vec![Type::Char],
            ret: Box::new(tb.clone()),
        }]],
        ret: Box::new(Type::Class {
            sym: idx,
            args: vec![tb],
        }),
    };
}

fn find_string_ops(st: &SymbolTable) -> Option<SymbolId> {
    crate::classpath::find_by_jvm(st, "scala/collection/StringOps")
}

fn find_indexed_seq(st: &SymbolTable) -> Option<SymbolId> {
    st.lookup_member(st.scala_pkg, "IndexedSeq")
        .into_iter()
        .find(|s| st.get(*s).jvm_name == "scala/collection/immutable/IndexedSeq")
}
