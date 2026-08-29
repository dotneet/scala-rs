//! `StringOps.map[B](f: Char => B): IndexedSeq[B]`。
//!
//! `prelude.rs` からは [`install`] を 1 行呼ぶだけにしてある。
//!
//! 2.13 の `StringOps` は `map` を 2 つ持つ。javap 2.13.16:
//!
//! ```text
//! public static java.lang.String map$extension(java.lang.String, scala.Function1);
//! public static <B> scala.collection.immutable.IndexedSeq<B>
//!     map$extension(java.lang.String, scala.Function1);
//! ```
//!
//! 戻り型だけが違う 2 本で、JVM の descriptor としては別物。prelude 側も
//! **2 つのシンボル**として持つのが正しい（1 つに畳むと `value_extension_desc`
//! がシンボルの結果型から descriptor を作るので、`Char => Char` のときにも
//! `IndexedSeq` を返す方を呼んで `ClassCastException` になる）。
//!
//! 2 つ並べたときのオーバーロード解決は nsc と同じ「より specific な方を採る」
//! で決まる: `Char => Char` は `Char => B` に適用できるが逆は成り立たないので、
//! ラムダが `Char` を返すときだけ `String` 版が勝つ。関数リテラルの結果型が
//! まだ決まっていない状態で両方が applicable になる件は `check.rs` の
//! `is_as_specific_method` が処理する。
//!
//! **私有ランタイム（`--no-scala-library`）** には `StringOps` 自体が無いので
//! `library_abi` のときだけ入れる。

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
    // `map(Char => Char): String` は prelude.rs が入れる。同じ名前で 2 本目。
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
