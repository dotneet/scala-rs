//! The prelude's collection *hierarchy*, with type arguments.
//!
//! The prelude builds `List`, `Vector`, `Seq`, `Set`, `Map`, ... one at a
//! time, and each of them was left extending `AnyRef`. A handful of edges had
//! been bolted on later where some specific call site needed one, but always
//! as a *raw* parent (`List.parents.push(Class { sym: Seq, args: vec![] })`),
//! which is not the same type as `Seq[A]`:
//!
//!   - `Vector[Int] <: Seq[Int]` was simply false — the edge did not exist —
//!     so `def f: Seq[Int] = Vector(1)` failed with `type mismatch`, and so
//!     did every slick signature taking `Seq`/`Iterable` and given a `Vector`.
//!   - Where a raw edge did exist, the missing arguments had to be invented at
//!     the conformance check. Inventing `Any` makes `C[X]` and `C[Y]` both
//!     "conform" while `C[X]` fails against `C[X]`, which is where the
//!     `found: C  required: C` messages came from.
//!
//! So the edges belong in one table, each with the arguments it passes to its
//! parent, applied once at the end of prelude construction. The relations are
//! the ones `scala.collection` declares in 2.13 (checked against the real jar:
//! `Vector` is an `immutable.IndexedSeq`, `immutable.IndexedSeq` an
//! `immutable.Seq`, `immutable.Seq` a `collection.Seq`, `collection.Seq` an
//! `Iterable`, `Iterable` an `IterableOnce`; `Map[K, V]` is an
//! `Iterable[(K, V)]`).
//!
//! Only the ancestors the prelude actually models are wired — the real library
//! threads `SeqOps`/`IterableOps`/`StrictOptimized...` in between, and those
//! carry no members here, so leaving them out changes nothing but the length
//! of the chain.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// How a child passes type arguments up to a parent.
#[derive(Clone, Copy)]
enum Args {
    /// Pass the child's own type parameters straight through, in order.
    Same,
    /// `Map[K, V] <: Iterable[(K, V)]`.
    Pairs,
    /// `Range <: IndexedSeq[Int]`.
    Int,
}

/// `(child jvm name, parent jvm name, arguments the child passes up)`.
const EDGES: &[(&str, &str, Args)] = &[
    (
        "scala/collection/Iterable",
        "scala/collection/IterableOnce",
        Args::Same,
    ),
    (
        "scala/collection/Iterator",
        "scala/collection/IterableOnce",
        Args::Same,
    ),
    (
        "scala/collection/Seq",
        "scala/collection/Iterable",
        Args::Same,
    ),
    (
        "scala/collection/Set",
        "scala/collection/Iterable",
        Args::Same,
    ),
    (
        "scala/collection/Map",
        "scala/collection/Iterable",
        Args::Pairs,
    ),
    (
        "scala/collection/View",
        "scala/collection/Iterable",
        Args::Same,
    ),
    (
        "scala/collection/SeqView",
        "scala/collection/View",
        Args::Same,
    ),
    (
        "scala/collection/immutable/Iterable",
        "scala/collection/Iterable",
        Args::Same,
    ),
    (
        "scala/collection/immutable/Seq",
        "scala/collection/Seq",
        Args::Same,
    ),
    (
        "scala/collection/immutable/IndexedSeq",
        "scala/collection/immutable/Seq",
        Args::Same,
    ),
    (
        "scala/collection/immutable/List",
        "scala/collection/immutable/Seq",
        Args::Same,
    ),
    (
        "scala/collection/immutable/Vector",
        "scala/collection/immutable/IndexedSeq",
        Args::Same,
    ),
    (
        "scala/collection/immutable/LazyList",
        "scala/collection/immutable/Seq",
        Args::Same,
    ),
    (
        "scala/collection/immutable/Queue",
        "scala/collection/immutable/Seq",
        Args::Same,
    ),
    (
        "scala/collection/immutable/Range",
        "scala/collection/immutable/IndexedSeq",
        Args::Int,
    ),
    (
        "scala/collection/immutable/ArraySeq",
        "scala/collection/immutable/IndexedSeq",
        Args::Same,
    ),
    (
        "scala/collection/immutable/Set",
        "scala/collection/Set",
        Args::Same,
    ),
    (
        "scala/collection/immutable/Map",
        "scala/collection/Map",
        Args::Same,
    ),
    (
        "scala/collection/immutable/SortedMap",
        "scala/collection/immutable/Map",
        Args::Same,
    ),
    (
        "scala/collection/immutable/TreeMap",
        "scala/collection/immutable/SortedMap",
        Args::Same,
    ),
    // The *unqualified* sorted traits. `BuildFrom`'s witnesses name them --
    // `buildFromSortedSetOps[CC[X] <: SortedSet[X] with SortedSetOps[X, CC, _]]`
    // is `scala.collection.SortedSet` -- and without the edge a `TreeSet`
    // was no `collection.SortedSet` at all: the *unsorted*
    // `buildFromIterableOps` answered instead and built through
    // `iterableFactory`, so `TreeSet(1,2).lazyZip(ys).map(f)` type-checked and
    // then died with `class Set$Set3 cannot be cast to class TreeSet`.
    // `val x: scala.collection.SortedSet[Int] = TreeSet(1)` was a
    // `type mismatch` for the same reason.
    (
        "scala/collection/immutable/SortedSet",
        "scala/collection/SortedSet",
        Args::Same,
    ),
    (
        "scala/collection/immutable/SortedMap",
        "scala/collection/SortedMap",
        Args::Same,
    ),
    // `immutable.SortedSet[A] extends Set[A] with collection.SortedSet[A]`.
    // Only the second half was wired, the asymmetry with `SortedMap` above
    // being an oversight rather than a decision.
    (
        "scala/collection/immutable/SortedSet",
        "scala/collection/immutable/Set",
        Args::Same,
    ),
    // `immutable.BitSet extends SortedSet[Int]`. Without the edge cats-kernel's
    // `x.subsetOf(y)` and `x | y` on two `BitSet`s were `no matching overload
    // for (Set[Int])Boolean with arguments (BitSet)`: the members were there,
    // read from the pickle, and the argument did not conform to its own type.
    (
        "scala/collection/immutable/BitSet",
        "scala/collection/immutable/SortedSet",
        Args::Int,
    ),
    (
        "scala/collection/mutable/Set",
        "scala/collection/Set",
        Args::Same,
    ),
    (
        "scala/collection/mutable/Map",
        "scala/collection/Map",
        Args::Same,
    ),
    (
        "scala/collection/IndexedSeq",
        "scala/collection/Seq",
        Args::Same,
    ),
    (
        "scala/collection/immutable/IndexedSeq",
        "scala/collection/IndexedSeq",
        Args::Same,
    ),
    (
        "scala/collection/mutable/Seq",
        "scala/collection/Seq",
        Args::Same,
    ),
    (
        "scala/collection/mutable/IndexedSeq",
        "scala/collection/mutable/Seq",
        Args::Same,
    ),
    (
        "scala/collection/mutable/IndexedSeq",
        "scala/collection/IndexedSeq",
        Args::Same,
    ),
    (
        "scala/collection/mutable/Buffer",
        "scala/collection/mutable/Seq",
        Args::Same,
    ),
    (
        "scala/collection/mutable/ArrayBuffer",
        "scala/collection/mutable/IndexedSeq",
        Args::Same,
    ),
    (
        "scala/collection/mutable/ArrayBuffer",
        "scala/collection/mutable/Buffer",
        Args::Same,
    ),
    (
        "scala/collection/mutable/ListBuffer",
        "scala/collection/mutable/Buffer",
        Args::Same,
    ),
];

/// The interior of the chain: classes the prelude either never built at all
/// or left as a bare placeholder, named as
/// `(jvm name, one variance character per type parameter)`.
///
/// They are prepared *before* any edge is installed, because an edge can only
/// pass `A` up to a parent that has somewhere to put it: `collection.Seq` with
/// no type parameter of its own could carry nothing from `immutable.Seq[A]` to
/// `Iterable[A]`, and the edge above it would be dropped as ill-formed.
///
/// They stay empty of members — those live on the concrete collections below.
const LINKS: &[(&str, &str)] = &[
    ("scala/collection/IterableOnce", "+"),
    ("scala/collection/Iterable", "+"),
    ("scala/collection/Seq", "+"),
    // `ArrayBuffer` was an `IndexedSeq` nowhere, so
    // `def and(ns: scala.collection.IndexedSeq[Node])` rejected the buffer
    // slick builds for it.
    ("scala/collection/IndexedSeq", "+"),
    ("scala/collection/Set", "="),
    ("scala/collection/Map", "=+"),
    // Named by `BuildFrom`'s sorted witnesses; see the edges below.
    ("scala/collection/SortedSet", "="),
    ("scala/collection/SortedMap", "=+"),
    // The mutable spine. Mutable collections are invariant.
    ("scala/collection/mutable/Seq", "="),
    ("scala/collection/mutable/IndexedSeq", "="),
    ("scala/collection/mutable/Buffer", "="),
];

pub fn install(st: &mut SymbolTable) {
    for (jvm, variance) in LINKS {
        ensure_link(st, jvm, variance);
    }
    let tuple2 = find(st, "scala/Tuple2");
    for (child_jvm, parent_jvm, args) in EDGES {
        let child = find(st, child_jvm);
        if child.is_none() {
            continue;
        }
        let want = match args {
            Args::Same => st.get(child).tparams.len(),
            Args::Pairs | Args::Int => 1,
        };
        let parent = ensure_arity(st, parent_jvm, want);
        if parent.is_none() || child == parent {
            continue;
        }
        let ctps = st.get(child).tparams.clone();
        let ptps = st.get(parent).tparams.len();
        let targs: Vec<Type> = match args {
            Args::Same => ctps.iter().map(|t| Type::TypeParam(*t)).collect(),
            Args::Pairs => {
                if ctps.len() != 2 || tuple2.is_none() {
                    continue;
                }
                vec![Type::Class {
                    sym: tuple2,
                    args: vec![Type::TypeParam(ctps[0]), Type::TypeParam(ctps[1])],
                }]
            }
            Args::Int => vec![Type::Int],
        };
        // A mismatched arity would install a parent that no substitution can
        // ever line up; skip it rather than record a lie.
        if targs.len() != ptps {
            continue;
        }
        set_parent(st, child, parent, targs);
    }
}

/// The class carrying `jvm` as its JVM name, if the prelude built one.
///
/// Matching on the JVM name rather than the scope keeps this independent of
/// which package a prelude class is *owned* by: `Vector` is owned by `scala`
/// but named `scala/collection/immutable/Vector`.
fn find(st: &SymbolTable, jvm: &str) -> SymbolId {
    st.symbols
        .iter()
        .position(|s| {
            s.jvm_name == jvm && s.kind == SymKind::Class && !s.flags.contains(Flags::MODULE)
        })
        .map(|i| SymbolId(i as u32))
        .unwrap_or(SymbolId::NONE)
}

/// Find the trait named by `jvm`, creating it if nothing carries that name,
/// and give it the type parameters `variance` describes if it has none.
fn ensure_link(st: &mut SymbolTable, jvm: &str, variance: &str) {
    let id = match find(st, jvm) {
        i if i.is_none() => {
            let (pkg, simple) = jvm.rsplit_once('/').unwrap_or(("", jvm));
            let owner = crate::classpath::ensure_package(st, pkg);
            let id = st.alloc(
                simple,
                owner,
                SymKind::Class,
                Flags::INTERFACE.with(Flags::ABSTRACT).with(Flags::TRAIT),
                jvm,
            );
            st.get_mut(id).parents = vec![Type::AnyRef];
            st.get_mut(id).ty = Type::Class {
                sym: id,
                args: vec![],
            };
            id
        }
        i => i,
    };
    if !st.get(id).tparams.is_empty() {
        return;
    }
    let names = ["A", "B", "C"];
    let tps: Vec<SymbolId> = variance
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let f = match c {
                '+' => Flags::COVARIANT,
                '-' => Flags::CONTRAVARIANT,
                _ => Flags::EMPTY,
            };
            let tp = st.alloc(names[i.min(2)], id, SymKind::TypeParam, f, "");
            st.get_mut(tp).ty = Type::TypeParam(tp);
            tp
        })
        .collect();
    st.get_mut(id).tparams = tps;
}

/// The parent class named by `jvm`, guaranteed to take `want` type arguments.
///
/// Some links in the chain exist only as the untyped placeholder
/// `find_or_stub_java_class` leaves behind (`scala.collection.Seq` was stubbed
/// just so `SeqHasAsJava` had a parameter type). A placeholder with no type
/// parameters cannot carry `A` up from `immutable.Seq[A]` to `Iterable[A]`,
/// so give it the parameters its real classfile has. They are covariant in
/// every collection this table names.
fn ensure_arity(st: &mut SymbolTable, jvm: &str, want: usize) -> SymbolId {
    let id = find(st, jvm);
    if id.is_none() || want == 0 {
        return id;
    }
    if st.get(id).tparams.is_empty() {
        let names = ["A", "B", "C"];
        let tps: Vec<SymbolId> = (0..want)
            .map(|i| {
                let tp = st.alloc(
                    names[i.min(2)],
                    id,
                    SymKind::TypeParam,
                    Flags::COVARIANT,
                    "",
                );
                st.get_mut(tp).ty = Type::TypeParam(tp);
                tp
            })
            .collect();
        st.get_mut(id).tparams = tps;
    }
    id
}

/// Record `parent[targs]` as a parent of `child`, replacing any edge to the
/// same class that an earlier prelude slice left raw.
fn set_parent(st: &mut SymbolTable, child: SymbolId, parent: SymbolId, targs: Vec<Type>) {
    let ty = Type::Class {
        sym: parent,
        args: targs,
    };
    let existing = st
        .get(child)
        .parents
        .iter()
        .position(|p| matches!(p, Type::Class { sym, .. } if *sym == parent));
    match existing {
        Some(i) => st.get_mut(child).parents[i] = ty,
        None => st.get_mut(child).parents.push(ty),
    }
}
