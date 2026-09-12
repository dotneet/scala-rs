//! `toSeq` on a collection that is not a sequence.
//!
//! `IterableOnceOps.toSeq` is declared `immutable.Seq[A]`, and an
//! `immutable.Seq` returns itself (`override final def toSeq: this.type`).
//! The prelude declared `Set`'s and `Map`'s as `List[A]` (`prelude_coll`), so
//! the call went out as
//! `invokeinterface Set.toSeq()Lscala/collection/immutable/List;` -- a
//! descriptor the library does not have -- and the *use* of the result failed
//! verification:
//!
//! ```scala
//! val setseq = Set(1, 2, 3, 4).toSeq
//! setseq.map(n => n * n).head   // VerifyError: Seq is not assignable to List
//! ```
//!
//! (corpus `run/t3563`, reported by agent/erascg). `Vector.toSeq` said `List`
//! for the same reason and is its own type.
//!
//! Only the result type changes, and only where the prelude itself wrote
//! `List`: a class whose `toSeq` came from the pickle already has the real
//! signature.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{SymbolId, Type};

/// Classes whose `toSeq` is `IterableOnceOps`' -- they are not sequences.
const NOT_SEQS: &[&str] = &[
    "scala/collection/immutable/Set",
    "scala/collection/immutable/Map",
    "scala/collection/mutable/Set",
    "scala/collection/mutable/Map",
    "scala/collection/Set",
    "scala/collection/Map",
    "scala/collection/Iterable",
];

/// Classes that *are* immutable sequences: `toSeq` is `this.type`.
const OWN_SEQS: &[&str] = &[
    "scala/collection/immutable/Vector",
    "scala/collection/immutable/IndexedSeq",
];

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        // The private runtime's only sequence is `List`; leave it alone.
        return;
    }
    let Some(seq) = crate::classpath::find_by_jvm(st, "scala/collection/immutable/Seq") else {
        return;
    };
    for jvm in NOT_SEQS {
        if let Some(cls) = crate::classpath::find_by_jvm(st, jvm) {
            retype_to_seq(st, cls, seq);
        }
    }
    for jvm in OWN_SEQS {
        if let Some(cls) = crate::classpath::find_by_jvm(st, jvm) {
            retype_to_seq(st, cls, cls);
        }
    }
}

/// Rewrite `cls`'s own `toSeq: List[X]` to `head[X]`, keeping the element
/// types the declaration already carries (a `Map`'s is its pair).
fn retype_to_seq(st: &mut SymbolTable, cls: SymbolId, head: SymbolId) {
    let members: Vec<SymbolId> = st
        .get(cls)
        .members
        .iter()
        .copied()
        .filter(|&m| st.get(m).name == "toSeq" && st.get(m).kind == SymKind::Method)
        .collect();
    for m in members {
        let Type::Method { paramss, ret } = st.get(m).ty.clone() else {
            continue;
        };
        if !paramss.iter().all(|c| c.is_empty()) {
            continue;
        }
        let Type::Class { sym, args } = ret.as_ref() else {
            continue;
        };
        if *sym != st.list_sym || *sym == head {
            continue;
        }
        st.get_mut(m).ty = Type::Method {
            paramss,
            ret: Box::new(Type::Class {
                sym: head,
                args: args.clone(),
            }),
        };
    }
}
