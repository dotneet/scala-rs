//! `SeqView` の「`C` を返すメンバ」。
//!
//! `prelude.rs` からは [`install`] を 1 行呼ぶだけにしてある。
//!
//! 実 2.13.16 の宣言は
//!
//! ```text
//! trait SeqView[+A] extends SeqOps[A, View, View[A]] with View[A]
//! ```
//!
//! で、`map` / `take` / `drop` / `reverse` / `sorted` などは `SeqView` を返す
//! ように**上書きされている**が、`filter` / `filterNot` / `takeWhile` /
//! `dropWhile` / `collect` / `flatMap` は上書きされておらず、`IterableOps` の
//! `C` = **`View[A]`** をそのまま返す（`javap scala.collection.SeqView` に
//! これらが現れないことで確かめられる）。
//!
//! scala-rs の `SeqView` は親を `View[A]` としか書いていないので、pickle から
//! `IterableOps.filter: C` を補うときの `C` が受け手の `SeqView[A]` に潰れて
//! いた。すると `xs.view.filter(p)` の静的型が `SeqView[Int]` になり、codegen
//! は戻り値に `checkcast scala/collection/SeqView` を出す。実行時の値は
//! `scala.collection.View$Filter`（`View` ではあるが `SeqView` ではない）なので、
//! **コンパイルは通り、実行して初めて `ClassCastException` になった**。
//!
//! ここで戻り型 `View[A]` を明示して直す。JVM 側の descriptor は `C` の消去で
//! ある `Ljava/lang/Object;` なので、`jvm_name` に手書きの descriptor を置く
//! （`prelude.rs` の `View$.fill` と同じ扱い）。呼び出し owner は `SeqView`
//! のままでよい: 実 scalac も `invokeinterface SeqView.filter` を出す。
//!
//! 私有ランタイム（`--no-scala-library`）に `SeqView` は無い。見つからなければ
//! 何もしない。

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// `(名前, 引数の消去 descriptor, 戻りが要素型そのままか)`。
/// `false` は `collect` / `flatMap` のように新しい要素型 `B` を導入するもの。
pub(crate) const C_MEMBERS: &[(&str, &str, bool)] = &[
    ("filter", "(Lscala/Function1;)Ljava/lang/Object;", true),
    ("filterNot", "(Lscala/Function1;)Ljava/lang/Object;", true),
    ("takeWhile", "(Lscala/Function1;)Ljava/lang/Object;", true),
    ("dropWhile", "(Lscala/Function1;)Ljava/lang/Object;", true),
    (
        "collect",
        "(Lscala/PartialFunction;)Ljava/lang/Object;",
        false,
    ),
    ("flatMap", "(Lscala/Function1;)Ljava/lang/Object;", false),
];

/// `SeqView` を受け手にしたときに `View` を返すと宣言した名前。
/// `check.rs` の `returns_receiver_collection` による受け手への作り直しは、
/// この名前に対しては**行ってはならない**（`SeqView` の `C` は `View[A]`）。
pub(crate) fn declares_view_result(name: &str) -> bool {
    C_MEMBERS.iter().any(|(n, _, _)| *n == name)
}

pub(crate) fn install(st: &mut SymbolTable) {
    let Some(seq_view) = find_iface(st, "scala/collection/SeqView") else {
        return;
    };
    let Some(view) = find_iface(st, "scala/collection/View") else {
        return;
    };
    let Some(a) = st.get(seq_view).tparams.first().copied() else {
        return;
    };
    // `View.map` は `IterableOps.map: CC[B]` の消去なので、実際の descriptor は
    // `(Lscala/Function1;)Ljava/lang/Object;`。`prelude.rs` の宣言は戻り型を
    // `View[B]` と書いてあり、`jvm_name` が空だと
    // `(Lscala/Function1;)Lscala/collection/View;` を呼びに行って
    // `NoSuchMethodError` になる（`xs.view.filter(p).map(f)` で踏んだ）。
    if let Some(m) = st.lookup_member(view, "map").into_iter().next() {
        if st.get(m).jvm_name.is_empty() {
            st.set_jvm_name(m, "(Lscala/Function1;)Ljava/lang/Object;");
        }
    }
    for (name, desc, same_elem) in C_MEMBERS {
        // 誰かが先に宣言していたら触らない（重複はオーバーロード集合を壊す）。
        if !st.lookup_member(seq_view, name).is_empty() {
            continue;
        }
        add_c_member(st, seq_view, view, a, name, desc, *same_elem);
    }
}

fn find_iface(st: &SymbolTable, jvm: &str) -> Option<SymbolId> {
    crate::classpath::find_by_jvm(st, jvm).filter(|s| st.get(*s).kind == SymKind::Class)
}

/// `def filter(p: A => Boolean): View[A]` / `def collect[B](pf: PartialFunction[A, B]): View[B]`。
fn add_c_member(
    st: &mut SymbolTable,
    seq_view: SymbolId,
    view: SymbolId,
    a: SymbolId,
    name: &str,
    desc: &str,
    same_elem: bool,
) {
    let id = st.alloc(name, seq_view, SymKind::Method, Flags::EMPTY, "");
    let ta = Type::TypeParam(a);
    let (param, elem) = if same_elem {
        (fn1(&ta, &Type::Boolean), ta.clone())
    } else {
        let b = st.alloc("B", id, SymKind::TypeParam, Flags::EMPTY, "");
        st.get_mut(b).ty = Type::TypeParam(b);
        st.get_mut(id).tparams = vec![b];
        let tb = Type::TypeParam(b);
        let p = if name == "collect" {
            partial_fn(st, &ta, &tb)
        } else {
            // `flatMap` の引数は `A => IterableOnce[B]` だが、消去は `Function1`
            // なので要素型だけ合っていればよい。
            fn1(&ta, &tb)
        };
        (p, tb)
    };
    st.get_mut(id).ty = Type::Method {
        paramss: vec![vec![param]],
        ret: Box::new(Type::Class {
            sym: view,
            args: vec![elem],
        }),
    };
    st.set_jvm_name(id, desc.to_string());
    st.get_mut(seq_view).members.push(id);
}

fn fn1(from: &Type, to: &Type) -> Type {
    Type::Function {
        params: vec![from.clone()],
        ret: Box::new(to.clone()),
    }
}

fn partial_fn(st: &SymbolTable, from: &Type, to: &Type) -> Type {
    match crate::classpath::find_by_jvm(st, "scala/PartialFunction") {
        Some(pf) => Type::Class {
            sym: pf,
            args: vec![from.clone(), to.clone()],
        },
        None => fn1(from, to),
    }
}
