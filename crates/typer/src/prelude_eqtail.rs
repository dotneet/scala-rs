//! `Equiv[Int]` の summon 失敗（`agent/ordsummon` 残件）。
//!
//! 実 ABI（`javap -p -s scala.math.Ordering` / `PartialOrdering` / `Equiv`）:
//!
//! ```text
//! interface scala.math.Ordering<T>        extends java.util.Comparator<T>, scala.math.PartialOrdering<T>
//! interface scala.math.PartialOrdering<T>  extends scala.math.Equiv<T>
//! interface scala.math.Equiv<T>            extends java.io.Serializable
//! ```
//!
//! `Equiv` は prelude のどこにも登場しなかった（`Ordering` は
//! `crates/typer/src/prelude.rs` の `add_ordering` が丸ごと手書きで作るが、
//! 対応するものが `Equiv` には無い）。`Equiv[Int]` という**型**自体は
//! `check.rs` の `expose_unqualified` が real `scala` package object の
//! pickle 別名（`type Equiv[T] = scala.math.Equiv[T]`）経由で見つけていたが、
//! そこで使う `PickleSupply::complete` は「pickle の断片から名前とシグネチャ
//! だけ」を読む軽量版で、継承関係を運ばない
//! （`crates/typer/src/classpath.rs` の `attach_classpath_parents` の
//! ドキュメント参照）。継承は「参照が一度失敗したときだけ」
//! `pickle_supply::ensure_parents` が補う作りだが、それは `import x._` の
//! prefix 解決からしか呼ばれない。結果:
//!
//! ```scala
//! val e: Equiv[Int] = Ordering.Int             // real scalac: OK（劣化代入）
//! val p: PartialOrdering[Int] = Ordering.Int    // real scalac: OK
//! implicitly[Equiv[Int]]                        // real scalac: OK
//! ```
//!
//! がすべて scala-rs では失敗していた: 前 2 つは `Ordering[Int]` に
//! `PartialOrdering` / `Equiv` が親として載っていないための `type
//! mismatch`。3 つ目は `object Equiv` が implicit instance を 1 つも
//! 持っていないための `could not find implicit value`
//! （real scalac は `Ordering.Int` ではなく `object Equiv` 自身の専用
//! instance `Equiv$Int$` を選ぶ -- `implicitly[Equiv[Int]].getClass.getName`
//! で確認した）。
//!
//! `Ordering` / `Numeric` / `Integral` / `Fractional`（`prelude_seq.rs` /
//! `prelude_numhier.rs`）は同じ穴を「jar のロードを待たず、prelude の時点で
//! 自前の class + companion module を作って現在スコープに入れる」ことで
//! 塞いでいる。`Equiv` と `PartialOrdering` にも同じ手を使う: ここで先に
//! 作って `enter_in_current` してしまえば、あとから jar 経由で解決しようと
//! する `expose_unqualified` は「もう スコープにある」ので通らず、
//! この prelude シンボルだけが使われる。メンバ（`equiv` / `fromComparator` /
//! `by` / `TupleN` 等）は `jvm_name` さえ実クラスと一致していれば
//! `pickle_supply` がオンデマンドで供給する（`Ordering` の `lt` / `gt` /
//! `lteq` / `gteq` / `max` / `min` が今も同じやり方で効いているのと同じ）。

use scala_rs_parser::{Flags, SymbolId, Type};

use crate::symbol::{SymKind, SymbolTable};

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        // 私有ランタイムには `scala/math/Equiv` / `PartialOrdering` の
        // classfile が無い。ここで何も作らなければ `Equiv[Int]` は
        // `not found: value Equiv` の診断のまま（スタブ禁止 --
        // `.agent-brief.md`）。
        return;
    }
    let Some(ordering) = crate::classpath::find_by_jvm(st, "scala/math/Ordering") else {
        return;
    };
    let math = crate::classpath::ensure_package(st, "scala/math");
    let equiv = ensure_equiv(st, math);
    let partial = ensure_partial_ordering(st, math, equiv);
    add_parent(st, ordering, partial);
    add_equiv_instances(st, equiv);
}

/// `trait Equiv[T]` と companion module を prelude 内に作り、現在スコープへ
/// 型・項の両方で入れる（既にあれば何もしない）。
fn ensure_equiv(st: &mut SymbolTable, math: SymbolId) -> SymbolId {
    if let Some(id) = crate::classpath::find_by_jvm(st, "scala/math/Equiv") {
        enter_type(st, "Equiv", id);
        if let Some(m) = st.companion_module(id) {
            enter_term(st, "Equiv", m);
        }
        return id;
    }
    let equiv = crate::prelude::iface(st, math, "Equiv", "scala/math/Equiv");
    let t = crate::prelude::type_param(st, equiv, "T");
    st.get_mut(equiv).tparams = vec![t];
    crate::prelude::method(
        st,
        equiv,
        "equiv",
        vec![Type::TypeParam(t), Type::TypeParam(t)],
        Type::Boolean,
        crate::symbol::Intrinsic::None,
    );
    let m = crate::prelude::module(st, math, "Equiv", "scala/math/Equiv$");
    enter_type(st, "Equiv", equiv);
    enter_term(st, "Equiv", m);
    equiv
}

/// `trait PartialOrdering[T] extends Equiv[T]` を prelude 内に作り、現在
/// スコープへ型で入れる（既にあれば `Equiv` を親として足すだけ）。
/// summon 可能な instance を real scalac も持たないので companion module は
/// 作らない（`implicitly[PartialOrdering[Int]]` は real scalac 同様
/// `could not find implicit value` のまま）。
fn ensure_partial_ordering(st: &mut SymbolTable, math: SymbolId, equiv: SymbolId) -> SymbolId {
    if let Some(id) = crate::classpath::find_by_jvm(st, "scala/math/PartialOrdering") {
        enter_type(st, "PartialOrdering", id);
        add_parent(st, id, equiv);
        return id;
    }
    let partial = crate::prelude::iface(st, math, "PartialOrdering", "scala/math/PartialOrdering");
    let t = crate::prelude::type_param(st, partial, "T");
    st.get_mut(partial).tparams = vec![t];
    add_parent(st, partial, equiv);
    enter_type(st, "PartialOrdering", partial);
    partial
}

fn enter_type(st: &mut SymbolTable, name: &str, id: SymbolId) {
    let already = st
        .lookup(name)
        .into_iter()
        .any(|s| s == id && st.get(s).kind == SymKind::Class);
    if !already {
        st.enter_in_current(name, id);
    }
}

fn enter_term(st: &mut SymbolTable, name: &str, id: SymbolId) {
    if st.lookup(name).into_iter().any(|s| s == id) {
        return;
    }
    st.enter_in_current(name, id);
}

/// `child[T] extends parent[T]`。`child` の最初の型パラメータをそのまま渡す。
/// すでに同じ親を持つなら何もしない（`prelude_numhier::add_parent` と同じ形
/// -- 新規モジュールに分けたので複製）。
fn add_parent(st: &mut SymbolTable, child: SymbolId, parent: SymbolId) {
    if st.get(child).kind != SymKind::Class {
        return;
    }
    if st
        .get(child)
        .parents
        .iter()
        .any(|p| matches!(p, Type::Class { sym, .. } if *sym == parent))
    {
        return;
    }
    let args = match st.get(child).tparams.first().copied() {
        Some(tp) if !st.get(parent).tparams.is_empty() => vec![Type::TypeParam(tp)],
        _ => Vec::new(),
    };
    st.get_mut(child)
        .parents
        .push(Type::Class { sym: parent, args });
}

/// `object Equiv` の implicit instance。jar: `scala/math/Equiv$<Name>$`。
fn add_equiv_instances(st: &mut SymbolTable, equiv: SymbolId) {
    let Some(equiv_mod) = st.companion_module(equiv) else {
        return;
    };
    let equiv_cls = st.module_class_of(equiv_mod);
    let big_int = crate::classpath::find_by_jvm(st, "scala/math/BigInt")
        .map(|sym| Type::Class { sym, args: vec![] });
    let big_dec = crate::classpath::find_by_jvm(st, "scala/math/BigDecimal")
        .map(|sym| Type::Class { sym, args: vec![] });
    let symbol = crate::classpath::find_by_jvm(st, "scala/Symbol")
        .map(|sym| Type::Class { sym, args: vec![] });
    let table: Vec<(&str, &str, Option<Type>)> = vec![
        ("Unit", "scala/math/Equiv$Unit$", Some(Type::Unit)),
        ("Boolean", "scala/math/Equiv$Boolean$", Some(Type::Boolean)),
        ("Byte", "scala/math/Equiv$Byte$", Some(Type::Byte)),
        ("Char", "scala/math/Equiv$Char$", Some(Type::Char)),
        ("Short", "scala/math/Equiv$Short$", Some(Type::Short)),
        ("Int", "scala/math/Equiv$Int$", Some(Type::Int)),
        ("Long", "scala/math/Equiv$Long$", Some(Type::Long)),
        ("BigInt", "scala/math/Equiv$BigInt$", big_int),
        ("BigDecimal", "scala/math/Equiv$BigDecimal$", big_dec),
        ("String", "scala/math/Equiv$String$", Some(Type::String)),
        ("Symbol", "scala/math/Equiv$Symbol$", symbol),
        // 2.13: `Equiv.Double` / `Equiv.Float` は `StrictEquiv` /
        // `IeeeEquiv` を抱える名前空間 object になり、implicit として
        // 選ばれるのは非推奨版の方（module doc 参照）。
        (
            "DeprecatedDoubleEquiv",
            "scala/math/Equiv$DeprecatedDoubleEquiv$",
            Some(Type::Double),
        ),
        (
            "DeprecatedFloatEquiv",
            "scala/math/Equiv$DeprecatedFloatEquiv$",
            Some(Type::Float),
        ),
    ];
    for (name, jvm, ty) in table {
        let Some(ty) = ty else { continue };
        add_implicit_instance(st, equiv_cls, equiv, name, jvm, ty);
    }
    let known: Vec<SymbolId> = st.get(equiv_mod).members.clone();
    for m in st.get(equiv_cls).members.clone() {
        if !known.contains(&m) {
            st.get_mut(equiv_mod).members.push(m);
        }
    }
}

/// `implicit object <name> extends Equiv[<arg>]`（`<jvm>.MODULE$`）を作る。
/// 既にその名前のメンバがあれば何もしない（`prelude_seq::add_implicit_instance`
/// と同じ形 -- 新規モジュールに分けたので複製）。
fn add_implicit_instance(
    st: &mut SymbolTable,
    equiv_cls: SymbolId,
    equiv: SymbolId,
    name: &str,
    jvm: &str,
    arg: Type,
) {
    if st
        .get(equiv_cls)
        .members
        .iter()
        .copied()
        .any(|m| st.get(m).name == name)
    {
        return;
    }
    let cls = st.alloc(
        format!("{name}$"),
        equiv_cls,
        SymKind::ModuleClass,
        Flags::MODULE.with(Flags::FINAL),
        jvm,
    );
    let m = st.alloc(name, equiv_cls, SymKind::Module, Flags::MODULE, jvm);
    st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
    let ty = Type::Class {
        sym: equiv,
        args: vec![arg],
    };
    st.get_mut(m).ty = ty.clone();
    st.get_mut(cls).ty = Type::ModuleRef(cls);
    st.get_mut(cls).parents = vec![ty];
}
