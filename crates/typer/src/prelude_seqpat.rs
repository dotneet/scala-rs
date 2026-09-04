//! `unapplySeq` for sequence patterns (`case Seq(a, b)` /
//! `case Vector(a, b, rest @ _*)` / `case Array(a, b)`).
//!
//! `prelude.rs` calls [`install`] on a single line.
//!
//! Until now only `List`'s companion had an `unapplySeq`, so `case Seq((s, _))` fell
//! into the "class pattern" branch, got no element type, and left `s` as `Any`
//! (slick `JdbcStatementBuilderComponent` among others).
//!
//! What real scalac 2.13.16 emits is
//!
//! ```text
//! Seq$.unapplySeq:(Lscala/collection/SeqOps;)Lscala/collection/SeqOps;
//! SeqFactory$UnapplySeqWrapper$.lengthCompare$extension:(Lscala/collection/SeqOps;I)I
//! SeqFactory$UnapplySeqWrapper$.apply$extension:(Lscala/collection/SeqOps;I)Ljava/lang/Object;
//! SeqFactory$UnapplySeqWrapper$.drop$extension:(Lscala/collection/SeqOps;I)Lscala/collection/immutable/Seq;
//! ```
//!
//! where only `Array` uses the same-named extension on
//! `scala/Array$UnapplySeqWrapper$`, taking `Ljava/lang/Object;`. The matching
//! invoke is emitted by `gen_unapply_wrapper_bind` in `crates/backend/src/gen.rs`
//! (`SeqPatShape` switches between the two wrappers).
//!
//! The result type declared here is `Option[Seq[A]]`, which differs from the real
//! descriptor. This is the same treatment as declaring `List$.unapplySeq` as
//! `Option[List[A]]`: codegen sees a "built-in sequence factory" and emits code that
//! does not go through the Option. The type attached to `_*` comes from **the element
//! container of this result type**, so the `rest` of `List(a, rest @ _*)` is `List[A]`
//! (as before) and the `rest` of `Seq(a, rest @ _*)` / `Array(a, rest @ _*)` is
//! `Seq[A]` (which matches real scalac's `drop$extension` return type).
//!
//! The **private runtime (`--no-scala-library`)** has neither `scala/collection/SeqOps`
//! nor `SeqFactory$UnapplySeqWrapper$`. The declarations themselves go in under both
//! modes, and `check.rs`'s `type_pattern` **emits a diagnostic** saying
//! `--scala-library` is required when the jar is absent (rather than quietly emitting
//! broken code).

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// The JVM names of the built-in companions that get an `unapplySeq`.
/// `check.rs` and `gen.rs` read the same table.
pub(crate) const SEQ_FACTORY_MODULES: &[&str] = &[
    "scala/collection/immutable/Seq$",
    "scala/collection/immutable/Vector$",
    "scala/collection/immutable/IndexedSeq$",
];

/// `scala.Array`'s companion. It takes a raw array rather than `SeqOps`.
pub(crate) const ARRAY_FACTORY_MODULE: &str = "scala/Array$";

pub(crate) fn install(st: &mut SymbolTable) {
    for jvm in SEQ_FACTORY_MODULES {
        let name = simple_name(jvm);
        let Some(m) = find_module(st, name, jvm) else {
            continue;
        };
        let Some(cls) = find_class(st, name) else {
            continue;
        };
        add_seq_unapply_seq(st, m, cls);
    }
    if let Some(m) = find_module(st, "Array", ARRAY_FACTORY_MODULE) {
        add_array_unapply_seq(st, m);
    }
}

fn simple_name(jvm: &str) -> &str {
    jvm.rsplit('/')
        .next()
        .map(|s| s.trim_end_matches('$'))
        .unwrap_or(jvm)
}

fn find_module(st: &SymbolTable, name: &str, jvm: &str) -> Option<SymbolId> {
    st.lookup_member(st.scala_pkg, name)
        .into_iter()
        .find(|s| st.get(*s).kind == SymKind::Module && st.get(*s).jvm_name == jvm)
}

fn find_class(st: &SymbolTable, name: &str) -> Option<SymbolId> {
    st.lookup_member(st.scala_pkg, name)
        .into_iter()
        .find(|s| st.get(*s).kind == SymKind::Class && st.get(*s).tparams.len() == 1)
}

/// The `Seq` type symbol (the container of the return type attached to `_*`).
fn seq_class(st: &SymbolTable) -> Option<SymbolId> {
    st.lookup_member(st.scala_pkg, "Seq").into_iter().find(|s| {
        st.get(*s).kind == SymKind::Class && st.get(*s).jvm_name == "scala/collection/immutable/Seq"
    })
}

/// `def unapplySeq[A](x: CC[A]): Option[Seq[A]]`.
fn add_seq_unapply_seq(st: &mut SymbolTable, module: SymbolId, coll: SymbolId) {
    let mcls = st.module_class_of(module);
    if mcls.is_none() {
        return;
    }
    if !st.lookup_member(mcls, "unapplySeq").is_empty() {
        return;
    }
    let Some(seq) = seq_class(st) else {
        return;
    };
    let id = st.alloc("unapplySeq", mcls, SymKind::Method, Flags::FINAL, "");
    let a = st.alloc("A", id, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(a).ty = Type::TypeParam(a);
    st.get_mut(id).tparams = vec![a];
    let ta = Type::TypeParam(a);
    st.get_mut(id).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: coll,
            args: vec![ta.clone()],
        }]],
        ret: Box::new(Type::Class {
            sym: st.option_sym,
            args: vec![Type::Class {
                sym: seq,
                args: vec![ta],
            }],
        }),
    };
}

/// `def unapplySeq[A](x: Array[A]): Option[Seq[A]]`.
fn add_array_unapply_seq(st: &mut SymbolTable, module: SymbolId) {
    let mcls = st.module_class_of(module);
    if mcls.is_none() {
        return;
    }
    if !st.lookup_member(mcls, "unapplySeq").is_empty() {
        return;
    }
    let Some(seq) = seq_class(st) else {
        return;
    };
    let id = st.alloc("unapplySeq", mcls, SymKind::Method, Flags::FINAL, "");
    let a = st.alloc("A", id, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(a).ty = Type::TypeParam(a);
    st.get_mut(id).tparams = vec![a];
    let ta = Type::TypeParam(a);
    st.get_mut(id).ty = Type::Method {
        paramss: vec![vec![Type::Array(Box::new(ta.clone()))]],
        ret: Box::new(Type::Class {
            sym: st.option_sym,
            args: vec![Type::Class {
                sym: seq,
                args: vec![ta],
            }],
        }),
    };
}
