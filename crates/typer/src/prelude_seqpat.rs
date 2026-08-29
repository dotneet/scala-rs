//! シーケンスパターン（`case Seq(a, b)` / `case Vector(a, b, rest @ _*)` /
//! `case Array(a, b)`）のための `unapplySeq`。
//!
//! `prelude.rs` からは [`install`] を 1 行呼ぶだけにしてある。
//!
//! これまで `unapplySeq` を持つのは `List` のコンパニオンだけだったので、
//! `case Seq((s, _))` は「クラスパターン」枝に落ちて要素型が付かず、
//! `s` が `Any` になっていた（slick `JdbcStatementBuilderComponent` ほか）。
//!
//! 実 scalac 2.13.16 が出すのは
//!
//! ```text
//! Seq$.unapplySeq:(Lscala/collection/SeqOps;)Lscala/collection/SeqOps;
//! SeqFactory$UnapplySeqWrapper$.lengthCompare$extension:(Lscala/collection/SeqOps;I)I
//! SeqFactory$UnapplySeqWrapper$.apply$extension:(Lscala/collection/SeqOps;I)Ljava/lang/Object;
//! SeqFactory$UnapplySeqWrapper$.drop$extension:(Lscala/collection/SeqOps;I)Lscala/collection/immutable/Seq;
//! ```
//!
//! で、`Array` だけは `scala/Array$UnapplySeqWrapper$` の同名 extension を
//! `Ljava/lang/Object;` 受けで使う。対応する invoke は
//! `crates/backend/src/gen.rs` の `gen_unapply_wrapper_bind` が出す
//! （`SeqPatShape` で 2 つの wrapper を切り替える）。
//!
//! ここで宣言する結果型は `Option[Seq[A]]` で、実際の descriptor とは違う。
//! `List$.unapplySeq` を `Option[List[A]]` と宣言してあるのと同じ扱いで、
//! codegen 側が「組み込みのシーケンス factory」を見て Option を経由しない
//! コードを出す。`_*` に付く型は**この結果型の要素コンテナ**から取るので、
//! `List(a, rest @ _*)` の `rest` は `List[A]`（従来どおり）、
//! `Seq(a, rest @ _*)` / `Array(a, rest @ _*)` の `rest` は `Seq[A]` になる
//! （後者は実 scalac の `drop$extension` の戻り型と一致する）。
//!
//! **私有ランタイム（`--no-scala-library`）** には `scala/collection/SeqOps`
//! も `SeqFactory$UnapplySeqWrapper$` も無い。宣言自体は両モードで入れて
//! おき、`check.rs` の `type_pattern` が jar 無しのときに
//! 「`--scala-library` が要る」と**診断を出す**（黙って壊れたコードを出さない）。

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// `unapplySeq` を持たせる組み込みコンパニオンの JVM 名。
/// `check.rs` と `gen.rs` が同じ表を見る。
pub(crate) const SEQ_FACTORY_MODULES: &[&str] = &[
    "scala/collection/immutable/Seq$",
    "scala/collection/immutable/Vector$",
    "scala/collection/immutable/IndexedSeq$",
];

/// `scala.Array` のコンパニオン。`SeqOps` ではなく生の配列を受ける。
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

/// `Seq` の型シンボル（`_*` に付ける戻り型のコンテナ）。
fn seq_class(st: &SymbolTable) -> Option<SymbolId> {
    st.lookup_member(st.scala_pkg, "Seq").into_iter().find(|s| {
        st.get(*s).kind == SymKind::Class && st.get(*s).jvm_name == "scala/collection/immutable/Seq"
    })
}

/// `def unapplySeq[A](x: CC[A]): Option[Seq[A]]`。
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

/// `def unapplySeq[A](x: Array[A]): Option[Seq[A]]`。
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
