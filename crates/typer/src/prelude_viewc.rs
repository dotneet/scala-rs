//! The "members that return `C`" of `SeqView`.
//!
//! `prelude.rs` calls [`install`] on a single line.
//!
//! The real 2.13.16 declarations are
//!
//! ```text
//! trait SeqView[+A] extends SeqOps[A, View, View[A]] with View[A]
//! ```
//!
//! and `map` / `take` / `drop` / `reverse` / `sorted` and friends are **overridden**
//! to return `SeqView`, while `filter` / `filterNot` / `takeWhile` / `dropWhile` /
//! `collect` / `flatMap` are not, and return `IterableOps`' `C` = **`View[A]`** as
//! it stands (confirmed by their absence from `javap scala.collection.SeqView`).
//!
//! scala-rs's `SeqView` writes only `View[A]` as its parent, so when
//! `IterableOps.filter: C` is supplied from the pickle its `C` collapsed to the
//! receiver's `SeqView[A]`. The static type of `xs.view.filter(p)` then became
//! `SeqView[Int]` and codegen emitted a `checkcast scala/collection/SeqView` on the
//! result. The run-time value is a `scala.collection.View$Filter` (a `View`, but not
//! a `SeqView`), so **it compiled and only threw `ClassCastException` when run**.
//!
//! The fix is to state the return type `View[A]` here. The JVM-side descriptor is
//! the erasure of `C`, `Ljava/lang/Object;`, so a hand-written descriptor goes in
//! `jvm_name` (the same treatment as `prelude.rs`'s `View$.fill`). The invoke owner
//! can stay `SeqView`: real scalac emits `invokeinterface SeqView.filter` too.
//!
//! The private runtime (`--no-scala-library`) has no `SeqView`. If it is not found,
//! do nothing.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// `(name, erased argument descriptor, whether the result keeps the element type)`.
/// `false` marks the ones that introduce a fresh element type `B`, like `collect` / `flatMap`.
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

/// The names declared to return `View` when the receiver is a `SeqView`.
/// `check.rs`'s `returns_receiver_collection` must **not** rebuild the result around
/// the receiver for these names (`SeqView`'s `C` is `View[A]`).
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
    // `View.map` is the erasure of `IterableOps.map: CC[B]`, so its real descriptor
    // is `(Lscala/Function1;)Ljava/lang/Object;`. `prelude.rs` declares the return
    // type as `View[B]`, and with an empty `jvm_name` we would call
    // `(Lscala/Function1;)Lscala/collection/View;` and get a `NoSuchMethodError`
    // (hit by `xs.view.filter(p).map(f)`).
    if let Some(m) = st.lookup_member(view, "map").into_iter().next() {
        if st.get(m).jvm_name.is_empty() {
            st.set_jvm_name(m, "(Lscala/Function1;)Ljava/lang/Object;");
        }
    }
    for (name, desc, same_elem) in C_MEMBERS {
        // Leave it alone if someone declared it first (duplicates wreck the overload set).
        if !st.lookup_member(seq_view, name).is_empty() {
            continue;
        }
        add_c_member(st, seq_view, view, a, name, desc, *same_elem);
    }
}

fn find_iface(st: &SymbolTable, jvm: &str) -> Option<SymbolId> {
    crate::classpath::find_by_jvm(st, jvm).filter(|s| st.get(*s).kind == SymKind::Class)
}

/// `def filter(p: A => Boolean): View[A]` / `def collect[B](pf: PartialFunction[A, B]): View[B]`.
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
            // `flatMap`'s argument is `A => IterableOnce[B]`, but the erasure is
            // `Function1`, so only the element type has to line up.
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
