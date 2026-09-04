//! The core members of `scala.collection.immutable.List` (scala-library 2.13.16 ABI).
//!
//! `prelude.rs` calls [`add_list_core`] on a single line. The signatures declared
//! here correspond to the real 2.13.16 descriptors confirmed with `javap -s`, and
//! the matching invokes are emitted by `emit_list_core_member` in
//! `crates/backend/src/gen.rs`.
//!
//! The members the real `List` does not have itself (`size` / `mkString` / `sum` /
//! `groupBy` / `sortBy` …) are default methods on `IterableOnceOps` / `IterableOps` /
//! `SeqOps`, so their results erase to `Object`. gen.rs checkcasts / unboxes them.
//!
//! Under the **private runtime (`--no-scala-library`)**, only what
//! `add_list_core_runtime` in `crates/backend/src/runtime.rs` actually emits into the
//! classfile is declared (`length` / `size` / `nonEmpty` / `last` / `reverse` /
//! `filter` / `filterNot` / `contains` / `exists` / `forall` / `count` / `take` /
//! `drop` / `mkString`). Nothing else is declared, so the diagnostic
//! `value X is not a member of List[A]` comes out (rather than silent acceptance).

use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

// ---------------------------------------------------------------------------
// Small helpers (equivalent to prelude.rs's; kept local to this file to avoid clashes)
// ---------------------------------------------------------------------------

fn type_param(st: &mut SymbolTable, owner: SymbolId, name: &str) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(id).ty = Type::TypeParam(id);
    id
}

fn fn1(arg: Type, ret: Type) -> Type {
    Type::Function {
        params: vec![arg],
        ret: Box::new(ret),
    }
}

fn fn2(a: Type, b: Type, ret: Type) -> Type {
    Type::Function {
        params: vec![a, b],
        ret: Box::new(ret),
    }
}

/// A plain method (no type parameters, no implicit arguments).
fn simple(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    params: Vec<Type>,
    ret: Type,
) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Method, Flags::FINAL, "");
    let paramss = if params.is_empty() {
        Vec::new()
    } else {
        vec![params]
    };
    st.get_mut(id).ty = Type::Method {
        paramss,
        ret: Box::new(ret),
    };
    st.get_mut(id).intrinsic = Intrinsic::None;
    id
}

/// A method with several parameter lists (the second and later are treated as
/// implicit) plus method type parameters, rewriting an existing symbol in place when
/// there is one and creating a new one otherwise.
///
/// When `reuse` is `Some(id)`, `id` is rewritten (used to replace an existing
/// approximate signature such as `map`'s with a truly polymorphic one).
fn poly_in(
    st: &mut SymbolTable,
    reuse: Option<SymbolId>,
    owner: SymbolId,
    name: &str,
    tparam_names: &[&str],
    implicit_from: usize,
    build: impl FnOnce(&[Type]) -> (Vec<Vec<Type>>, Type),
) -> SymbolId {
    let m = match reuse {
        Some(id) => {
            // A reused symbol has its type parameters and arguments rebuilt.
            st.get_mut(id).tparams.clear();
            st.get_mut(id).params.clear();
            st.get_mut(id).paramss.clear();
            id
        }
        None => st.alloc(name, owner, SymKind::Method, Flags::FINAL, ""),
    };
    let tps: Vec<SymbolId> = tparam_names
        .iter()
        .map(|n| type_param(st, m, n))
        .collect::<Vec<_>>();
    let targs: Vec<Type> = tps.iter().map(|t| Type::TypeParam(*t)).collect();
    let (paramss, ret) = build(&targs);

    let mut all = Vec::new();
    let mut pss = Vec::new();
    let mut idx = 0usize;
    for (li, list) in paramss.iter().enumerate() {
        let mut cur = Vec::new();
        for ty in list {
            idx += 1;
            let implicit = li >= implicit_from;
            let (nm, flags) = if implicit {
                (
                    format!("evidence${idx}"),
                    Flags::PARAM.with(Flags::IMPLICIT),
                )
            } else {
                (format!("x${idx}"), Flags::PARAM)
            };
            let p = st.alloc(&nm, m, SymKind::Term, flags, "");
            st.get_mut(p).ty = ty.clone();
            cur.push(p);
            all.push(p);
        }
        pss.push(cur);
    }
    st.get_mut(m).tparams = tps;
    st.get_mut(m).params = all;
    st.get_mut(m).paramss = pss;
    st.get_mut(m).ty = Type::Method {
        paramss,
        ret: Box::new(ret),
    };
    st.get_mut(m).intrinsic = Intrinsic::None;
    m
}

fn poly(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    tparam_names: &[&str],
    build: impl FnOnce(&[Type]) -> (Vec<Vec<Type>>, Type),
) -> SymbolId {
    poly_in(st, None, owner, name, tparam_names, usize::MAX, build)
}

/// A method with implicit parameter lists. Lists from `implicit_from` on are implicit.
fn poly_implicit(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    tparam_names: &[&str],
    implicit_from: usize,
    build: impl FnOnce(&[Type]) -> (Vec<Vec<Type>>, Type),
) -> SymbolId {
    poly_in(st, None, owner, name, tparam_names, implicit_from, build)
}

/// The first same-named method directly under `owner` (inherited ones are not considered).
fn own_method(st: &SymbolTable, owner: SymbolId, name: &str) -> Option<SymbolId> {
    st.get(owner)
        .members
        .iter()
        .copied()
        .find(|m| st.get(*m).name == name && st.get(*m).kind == SymKind::Method)
}

fn find_iface(st: &mut SymbolTable, jvm: &str) -> SymbolId {
    if let Some(id) = crate::classpath::find_by_jvm(st, jvm) {
        return id;
    }
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

/// Create `object <name>` under `owner` (returning the existing one if there is one).
fn find_or_make_module(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    jvm: &str,
) -> (SymbolId, SymbolId) {
    if let Some(m) = st
        .get(owner)
        .members
        .iter()
        .copied()
        .find(|m| st.get(*m).name == name && st.get(*m).kind == SymKind::Module)
    {
        let cls = st.module_class_of(m);
        return (m, cls);
    }
    let cls = st.alloc(
        format!("{name}$"),
        owner,
        SymKind::ModuleClass,
        Flags::MODULE.with(Flags::FINAL),
        jvm,
    );
    let m = st.alloc(name, owner, SymKind::Module, Flags::MODULE, jvm);
    st.get_mut(m).ty = Type::ModuleRef(cls);
    st.get_mut(cls).ty = Type::ModuleRef(cls);
    (m, cls)
}

/// `implicit object <name> extends <tc>[<arg>]` (`<jvm>.MODULE$`).
fn add_implicit_instance(
    st: &mut SymbolTable,
    companion_cls: SymbolId,
    tc: SymbolId,
    name: &str,
    jvm: &str,
    arg: Type,
) {
    if st
        .get(companion_cls)
        .members
        .iter()
        .copied()
        .any(|m| st.get(m).name == name)
    {
        return;
    }
    let (m, cls) = find_or_make_module(st, companion_cls, name, jvm);
    st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
    st.get_mut(m).ty = Type::Class {
        sym: tc,
        args: vec![arg.clone()],
    };
    st.get_mut(cls).parents = vec![Type::Class {
        sym: tc,
        args: vec![arg],
    }];
}

// ---------------------------------------------------------------------------

/// The surrounding symbols used to build types.
struct Env {
    list: SymbolId,
    a: SymbolId,
    option: SymbolId,
    tuple2: SymbolId,
    iterable_once: SymbolId,
    iterable: SymbolId,
    iterator: SymbolId,
    map: SymbolId,
    set: SymbolId,
    vector: SymbolId,
    seq: SymbolId,
    ordering: SymbolId,
    numeric: SymbolId,
    classtag: SymbolId,
    partial_fn: SymbolId,
}

impl Env {
    fn ta(&self) -> Type {
        Type::TypeParam(self.a)
    }
    fn list_of(&self, t: Type) -> Type {
        Type::Class {
            sym: self.list,
            args: vec![t],
        }
    }
    fn one(&self, sym: SymbolId, t: Type) -> Type {
        Type::Class { sym, args: vec![t] }
    }
    fn two(&self, sym: SymbolId, a: Type, b: Type) -> Type {
        Type::Class {
            sym,
            args: vec![a, b],
        }
    }
    fn pair(&self, a: Type, b: Type) -> Type {
        self.two(self.tuple2, a, b)
    }
}

fn find_in_scala_pkg(st: &SymbolTable, name: &str) -> SymbolId {
    st.get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == name)
        .unwrap_or(SymbolId::NONE)
}

/// The core members of `List`.
///
/// Under `library_abi`, the whole set of real scala-library 2.13.16 signatures is
/// added. Under the private runtime (`--no-scala-library`), only what
/// `crates/backend/src/runtime.rs` actually emits into the classfile is added and
/// nothing else is declared (i.e. `value X is not a member of List[A]` comes out).
pub(crate) fn add_list_core(st: &mut SymbolTable, library_abi: bool) {
    let list = st.list_sym;
    let Some(a) = st.get(list).tparams.first().copied() else {
        return;
    };
    if !library_abi {
        add_list_core_private(st, list, a);
        return;
    }

    let iterable_once = find_iface(st, "scala/collection/IterableOnce");
    if st.get(iterable_once).tparams.is_empty() {
        let p = type_param(st, iterable_once, "A");
        st.get_mut(iterable_once).tparams = vec![p];
    }
    let iterable = find_iface(st, "scala/collection/Iterable");
    if st.get(iterable).tparams.is_empty() {
        let p = type_param(st, iterable, "A");
        st.get_mut(iterable).tparams = vec![p];
    }
    // In 2.13 `List` is a `scala.collection.Iterable` / `IterableOnce`.
    add_parent(st, list, iterable, 1);
    add_parent(st, list, iterable_once, 1);

    let ordering = find_iface(st, "scala/math/Ordering");
    let numeric = add_numeric(st);
    add_ordering_instances(st, ordering);

    let env = Env {
        list,
        a,
        option: st.option_sym,
        tuple2: find_in_scala_pkg(st, "Tuple2"),
        iterable_once,
        iterable,
        iterator: find_iface(st, "scala/collection/Iterator"),
        map: find_iface(st, "scala/collection/immutable/Map"),
        set: find_iface(st, "scala/collection/immutable/Set"),
        vector: find_iface(st, "scala/collection/immutable/Vector"),
        seq: find_iface(st, "scala/collection/immutable/Seq"),
        ordering,
        numeric,
        classtag: find_iface(st, "scala/reflect/ClassTag"),
        partial_fn: find_in_scala_pkg(st, "PartialFunction"),
    };

    make_polymorphic(st, &env);
    add_filters_and_slices(st, &env);
    add_predicates_and_folds(st, &env);
    add_strings_and_aggregates(st, &env);
    add_sorting_and_zips(st, &env);
    add_conversions(st, &env);
    add_grouping(st, &env);
    add_iterator_to_list(st, &env);
}

/// Only what the private runtime's `List` classfile (`add_list_core_runtime` in
/// `crates/backend/src/runtime.rs`) implements. Members absent from here become a
/// diagnostic in non-jar mode.
fn add_list_core_private(st: &mut SymbolTable, list: SymbolId, a: SymbolId) {
    let ta = Type::TypeParam(a);
    let list_a = Type::Class {
        sym: list,
        args: vec![ta.clone()],
    };
    let pred = fn1(ta.clone(), Type::Boolean);
    simple(st, list, "length", vec![], Type::Int);
    simple(st, list, "size", vec![], Type::Int);
    simple(st, list, "nonEmpty", vec![], Type::Boolean);
    simple(st, list, "reverse", vec![], list_a.clone());
    simple(st, list, "last", vec![], ta.clone());
    simple(st, list, "filter", vec![pred.clone()], list_a.clone());
    simple(st, list, "filterNot", vec![pred.clone()], list_a.clone());
    simple(st, list, "contains", vec![ta], Type::Boolean);
    simple(st, list, "exists", vec![pred.clone()], Type::Boolean);
    simple(st, list, "forall", vec![pred.clone()], Type::Boolean);
    simple(st, list, "count", vec![pred], Type::Int);
    simple(st, list, "take", vec![Type::Int], list_a.clone());
    simple(st, list, "drop", vec![Type::Int], list_a);
    simple(st, list, "mkString", vec![], Type::String);
    simple(st, list, "mkString", vec![Type::String], Type::String);
    simple(
        st,
        list,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
    );
}

fn add_parent(st: &mut SymbolTable, cls: SymbolId, parent: SymbolId, nargs: usize) {
    if st
        .get(cls)
        .parents
        .iter()
        .any(|p| matches!(p, Type::Class { sym, .. } if *sym == parent))
    {
        return;
    }
    let args = st
        .get(cls)
        .tparams
        .iter()
        .copied()
        .take(nargs)
        .map(Type::TypeParam)
        .collect::<Vec<_>>();
    st.get_mut(cls)
        .parents
        .push(Type::Class { sym: parent, args });
}

/// `scala.math.Numeric` and the implicit instances for `sum` / `product`.
/// JVM: `scala/math/Numeric$IntIsIntegral$.MODULE$` and friends.
fn add_numeric(st: &mut SymbolTable) -> SymbolId {
    let numeric = find_iface(st, "scala/math/Numeric");
    if st.get(numeric).tparams.is_empty() {
        let t = type_param(st, numeric, "T");
        st.get_mut(numeric).tparams = vec![t];
    }
    let math = crate::classpath::ensure_package(st, "scala/math");
    let (num_mod, num_cls) = find_or_make_module(st, math, "Numeric", "scala/math/Numeric$");
    for (name, jvm, ty) in [
        (
            "IntIsIntegral",
            "scala/math/Numeric$IntIsIntegral$",
            Type::Int,
        ),
        (
            "LongIsIntegral",
            "scala/math/Numeric$LongIsIntegral$",
            Type::Long,
        ),
        (
            "DoubleIsFractional",
            "scala/math/Numeric$DoubleIsFractional$",
            Type::Double,
        ),
        // Same reason as `Ordering$Byte$` / `Ordering$Short$`.
        (
            "ByteIsIntegral",
            "scala/math/Numeric$ByteIsIntegral$",
            Type::Byte,
        ),
        (
            "ShortIsIntegral",
            "scala/math/Numeric$ShortIsIntegral$",
            Type::Short,
        ),
    ] {
        add_implicit_instance(st, num_cls, numeric, name, jvm, ty);
    }
    let mems = st.get(num_cls).members.clone();
    st.get_mut(num_mod).members.extend(mems);
    numeric
}

/// Add more `Ordering` implicit instances for `sorted` / `max` / `sortBy`
/// (`Int` / `Char` are already installed on the prelude.rs side).
fn add_ordering_instances(st: &mut SymbolTable, ordering: SymbolId) {
    let math = crate::classpath::ensure_package(st, "scala/math");
    let (ord_mod, ord_cls) = find_or_make_module(st, math, "Ordering", "scala/math/Ordering$");
    for (name, jvm, ty) in [
        ("String", "scala/math/Ordering$String$", Type::String),
        ("Long", "scala/math/Ordering$Long$", Type::Long),
        ("Boolean", "scala/math/Ordering$Boolean$", Type::Boolean),
        // `Byte` and `Short` became real JVM primitives (they erase to
        // `java/lang/Byte` / `java/lang/Short`), so `xs.sortBy(_.keySeq)` needs an
        // `Ordering[Short]` too. The jar really does have `Ordering$Byte$` /
        // `Ordering$Short$`.
        ("Byte", "scala/math/Ordering$Byte$", Type::Byte),
        ("Short", "scala/math/Ordering$Short$", Type::Short),
        // In 2.13 `Ordering.Double` / `Ordering.Float` became namespace objects
        // (holding `TotalOrdering` / `IeeeOrdering`), and the implicits actually
        // picked are `DeprecatedDoubleOrdering` / `DeprecatedFloatOrdering`.
        // Confirmed by having scalac print `implicitly[Ordering[Double]]`.
        (
            "DeprecatedDoubleOrdering",
            "scala/math/Ordering$DeprecatedDoubleOrdering$",
            Type::Double,
        ),
        (
            "DeprecatedFloatOrdering",
            "scala/math/Ordering$DeprecatedFloatOrdering$",
            Type::Float,
        ),
        ("Unit", "scala/math/Ordering$Unit$", Type::Unit),
    ] {
        add_implicit_instance(st, ord_cls, ordering, name, jvm, ty);
    }
    let known: Vec<SymbolId> = st.get(ord_mod).members.clone();
    for m in st.get(ord_cls).members.clone() {
        if !known.contains(&m) {
            st.get_mut(ord_mod).members.push(m);
        }
    }
}

/// Replace `map` / `flatMap` / `collect` with truly polymorphic signatures.
///
/// JVM (2.13.16; all of them virtuals on `List` itself):
/// - `map:(Lscala/Function1;)Lscala/collection/immutable/List;`
/// - `flatMap:(Lscala/Function1;)Lscala/collection/immutable/List;`
/// - `collect:(Lscala/PartialFunction;)Lscala/collection/immutable/List;`
fn make_polymorphic(st: &mut SymbolTable, env: &Env) {
    let l = env.list;
    let ta = env.ta();

    let existing = own_method(st, l, "map");
    poly_in(st, existing, l, "map", &["B"], usize::MAX, |t| {
        let b = t[0].clone();
        (
            vec![vec![fn1(ta.clone(), b.clone())]],
            Type::Class {
                sym: l,
                args: vec![b],
            },
        )
    });

    let ta = env.ta();
    let ioc = env.iterable_once;
    let existing = own_method(st, l, "flatMap");
    poly_in(st, existing, l, "flatMap", &["B"], usize::MAX, |t| {
        let b = t[0].clone();
        (
            vec![vec![fn1(
                ta.clone(),
                Type::Class {
                    sym: ioc,
                    args: vec![b.clone()],
                },
            )]],
            Type::Class {
                sym: l,
                args: vec![b],
            },
        )
    });

    // `collect[B](pf: PartialFunction[A, B]): List[B]`.
    // B is decided by passing a type-annotated `val pf: PartialFunction[Int, String]`
    // (the same as ArrayOps' `collect`). Passing an inline `{ case … }` literal
    // directly is not yet supported by the typer (nor is it for ArrayOps).
    if !env.partial_fn.is_none() {
        let ta = env.ta();
        let pf = env.partial_fn;
        let existing = own_method(st, l, "collect");
        poly_in(st, existing, l, "collect", &["B"], usize::MAX, |t| {
            let b = t[0].clone();
            (
                vec![vec![Type::Class {
                    sym: pf,
                    args: vec![ta.clone(), b.clone()],
                }]],
                Type::Class {
                    sym: l,
                    args: vec![b],
                },
            )
        });
    }

    // `::` / `:::` / `+:` / `:+` / `++` are polymorphic in `B >: A` too.
    let existing = own_method(st, l, "::");
    poly_in(st, existing, l, "::", &["B"], usize::MAX, |t| {
        let b = t[0].clone();
        (
            vec![vec![b.clone()]],
            Type::Class {
                sym: l,
                args: vec![b],
            },
        )
    });

    poly(st, l, ":::", &["B"], |t| {
        let b = t[0].clone();
        (
            vec![vec![Type::Class {
                sym: l,
                args: vec![b.clone()],
            }]],
            Type::Class {
                sym: l,
                args: vec![b],
            },
        )
    });
    poly(st, l, "+:", &["B"], |t| {
        let b = t[0].clone();
        (
            vec![vec![b.clone()]],
            Type::Class {
                sym: l,
                args: vec![b],
            },
        )
    });
    poly(st, l, ":+", &["B"], |t| {
        let b = t[0].clone();
        (
            vec![vec![b.clone()]],
            Type::Class {
                sym: l,
                args: vec![b],
            },
        )
    });
    let ioc = env.iterable_once;
    for name in ["++", ":++", "concat"] {
        poly(st, l, name, &["B"], |t| {
            let b = t[0].clone();
            (
                vec![vec![Type::Class {
                    sym: ioc,
                    args: vec![b.clone()],
                }]],
                Type::Class {
                    sym: l,
                    args: vec![b],
                },
            )
        });
    }
    poly(st, l, "++:", &["B"], |t| {
        let b = t[0].clone();
        (
            vec![vec![Type::Class {
                sym: ioc,
                args: vec![b.clone()],
            }]],
            Type::Class {
                sym: l,
                args: vec![b],
            },
        )
    });
    poly(st, l, "updated", &["B"], |t| {
        let b = t[0].clone();
        (
            vec![vec![Type::Int, b.clone()]],
            Type::Class {
                sym: l,
                args: vec![b],
            },
        )
    });
}

/// The `filter` family and subsequences. All virtuals on `List` itself (some erased).
///
/// JVM: `filter`/`filterNot`/`takeWhile:(Function1)List`, `take`/`takeRight`:`(I)List`,
/// `slice:(II)List`, `drop:(I)LinearSeq`, `dropWhile:(Function1)LinearSeq`,
/// `dropRight:(I)Object`, `splitAt:(I)Tuple2`, `span`/`partition:(Function1)Tuple2`,
/// `distinct` is `SeqOps.distinct:()Object`.
fn add_filters_and_slices(st: &mut SymbolTable, env: &Env) {
    let l = env.list;
    let ta = env.ta();
    let list_a = env.list_of(ta.clone());
    let pred = fn1(ta.clone(), Type::Boolean);

    for name in ["filter", "filterNot", "takeWhile", "dropWhile"] {
        simple(st, l, name, vec![pred.clone()], list_a.clone());
    }
    for name in ["take", "drop", "takeRight", "dropRight"] {
        simple(st, l, name, vec![Type::Int], list_a.clone());
    }
    simple(st, l, "slice", vec![Type::Int, Type::Int], list_a.clone());
    simple(st, l, "reverse", vec![], list_a.clone());
    simple(st, l, "distinct", vec![], list_a.clone());
    simple(st, l, "init", vec![], list_a.clone());
    simple(st, l, "toList", vec![], list_a.clone());

    let pair = env.pair(list_a.clone(), list_a.clone());
    simple(st, l, "splitAt", vec![Type::Int], pair.clone());
    simple(st, l, "span", vec![pred.clone()], pair.clone());
    simple(st, l, "partition", vec![pred.clone()], pair);

    poly(st, l, "distinctBy", &["B"], |t| {
        (vec![vec![fn1(ta.clone(), t[0].clone())]], list_a.clone())
    });
}

/// Predicates, searching and folding.
///
/// JVM: `forall`/`exists:(Function1)Z`, `contains:(Object)Z`,
/// `find:(Function1)Option`, `last:()Object`, `headOption`/`lastOption:()Option`,
/// `foldLeft`/`foldRight:(Object,Function2)Object`,
/// `IterableOnceOps.count:(Function1)I` / `reduce`/`reduceLeft`/`reduceRight:(Function2)Object`,
/// `List.scanLeft:(Object,Function2)Object`,
/// `SeqOps.indexOf:(Object)I` / `startsWith:(IterableOnce,I)Z` / `endsWith:(Iterable)Z`.
fn add_predicates_and_folds(st: &mut SymbolTable, env: &Env) {
    let l = env.list;
    let ta = env.ta();
    let pred = fn1(ta.clone(), Type::Boolean);

    for name in ["forall", "exists"] {
        simple(st, l, name, vec![pred.clone()], Type::Boolean);
    }
    simple(st, l, "count", vec![pred.clone()], Type::Int);
    simple(st, l, "contains", vec![ta.clone()], Type::Boolean);
    simple(
        st,
        l,
        "find",
        vec![pred.clone()],
        env.one(env.option, ta.clone()),
    );
    simple(st, l, "last", vec![], ta.clone());
    simple(st, l, "headOption", vec![], env.one(env.option, ta.clone()));
    simple(st, l, "lastOption", vec![], env.one(env.option, ta.clone()));
    simple(st, l, "nonEmpty", vec![], Type::Boolean);
    simple(st, l, "size", vec![], Type::Int);
    simple(st, l, "indexOf", vec![ta.clone()], Type::Int);
    simple(st, l, "indexWhere", vec![pred.clone()], Type::Int);

    for name in ["reduce", "reduceLeft", "reduceRight"] {
        simple(
            st,
            l,
            name,
            vec![fn2(ta.clone(), ta.clone(), ta.clone())],
            ta.clone(),
        );
    }

    let tb = ta.clone();
    poly(st, l, "foldLeft", &["B"], |t| {
        let b = t[0].clone();
        (
            vec![vec![b.clone()], vec![fn2(b.clone(), tb.clone(), b.clone())]],
            b,
        )
    });
    let tb = ta.clone();
    poly(st, l, "foldRight", &["B"], |t| {
        let b = t[0].clone();
        (
            vec![vec![b.clone()], vec![fn2(tb.clone(), b.clone(), b.clone())]],
            b,
        )
    });
    let tb = ta.clone();
    let l2 = l;
    poly(st, l, "scanLeft", &["B"], |t| {
        let b = t[0].clone();
        (
            vec![vec![b.clone()], vec![fn2(b.clone(), tb.clone(), b.clone())]],
            Type::Class {
                sym: l2,
                args: vec![b],
            },
        )
    });

    let ioc = env.iterable_once;
    poly(st, l, "startsWith", &["B"], |t| {
        (
            vec![vec![Type::Class {
                sym: ioc,
                args: vec![t[0].clone()],
            }]],
            Type::Boolean,
        )
    });
    let itbl = env.iterable;
    poly(st, l, "endsWith", &["B"], |t| {
        (
            vec![vec![Type::Class {
                sym: itbl,
                args: vec![t[0].clone()],
            }]],
            Type::Boolean,
        )
    });
}

/// `mkString` / `sum` / `product` / `min` / `max` / `minBy` / `maxBy`.
/// All of them default methods on `IterableOnceOps`.
fn add_strings_and_aggregates(st: &mut SymbolTable, env: &Env) {
    let l = env.list;
    let ta = env.ta();

    simple(st, l, "mkString", vec![], Type::String);
    simple(st, l, "mkString", vec![Type::String], Type::String);
    simple(
        st,
        l,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
    );

    let numeric = env.numeric;
    for name in ["sum", "product"] {
        let ta2 = ta.clone();
        poly_implicit(st, l, name, &[], 0, move |_| {
            (
                vec![vec![Type::Class {
                    sym: numeric,
                    args: vec![ta2.clone()],
                }]],
                ta2,
            )
        });
    }
    let ordering = env.ordering;
    for name in ["min", "max"] {
        let ta2 = ta.clone();
        poly_implicit(st, l, name, &[], 0, move |_| {
            (
                vec![vec![Type::Class {
                    sym: ordering,
                    args: vec![ta2.clone()],
                }]],
                ta2,
            )
        });
    }
    for name in ["minBy", "maxBy"] {
        let ta2 = ta.clone();
        poly_implicit(st, l, name, &["B"], 1, move |t| {
            let b = t[0].clone();
            (
                vec![
                    vec![fn1(ta2.clone(), b.clone())],
                    vec![Type::Class {
                        sym: ordering,
                        args: vec![b],
                    }],
                ],
                ta2,
            )
        });
    }
}

/// `sorted` / `sortBy` / `sortWith` / `zip` / `zipWithIndex`.
fn add_sorting_and_zips(st: &mut SymbolTable, env: &Env) {
    let l = env.list;
    let ta = env.ta();
    let list_a = env.list_of(ta.clone());
    let ordering = env.ordering;

    {
        let ta2 = ta.clone();
        let ret = list_a.clone();
        poly_implicit(st, l, "sorted", &[], 0, move |_| {
            (
                vec![vec![Type::Class {
                    sym: ordering,
                    args: vec![ta2],
                }]],
                ret,
            )
        });
    }
    {
        let ta2 = ta.clone();
        let ret = list_a.clone();
        poly_implicit(st, l, "sortBy", &["B"], 1, move |t| {
            let b = t[0].clone();
            (
                vec![
                    vec![fn1(ta2, b.clone())],
                    vec![Type::Class {
                        sym: ordering,
                        args: vec![b],
                    }],
                ],
                ret,
            )
        });
    }
    simple(
        st,
        l,
        "sortWith",
        vec![fn2(ta.clone(), ta.clone(), Type::Boolean)],
        list_a.clone(),
    );

    let tuple2 = env.tuple2;
    let ioc = env.iterable_once;
    {
        let ta2 = ta.clone();
        poly(st, l, "zip", &["B"], move |t| {
            let b = t[0].clone();
            (
                vec![vec![Type::Class {
                    sym: ioc,
                    args: vec![b.clone()],
                }]],
                Type::Class {
                    sym: l,
                    args: vec![Type::Class {
                        sym: tuple2,
                        args: vec![ta2, b],
                    }],
                },
            )
        });
    }
    simple(
        st,
        l,
        "zipWithIndex",
        vec![],
        env.list_of(env.pair(ta.clone(), Type::Int)),
    );
}

/// `toArray` / `toSet` / `toVector` / `toSeq` / `toIndexedSeq`.
fn add_conversions(st: &mut SymbolTable, env: &Env) {
    let l = env.list;
    let ta = env.ta();
    let ct = env.classtag;
    {
        let ta2 = ta.clone();
        poly_implicit(st, l, "toArray", &[], 0, move |_| {
            (
                vec![vec![Type::Class {
                    sym: ct,
                    args: vec![ta2.clone()],
                }]],
                Type::Array(Box::new(ta2)),
            )
        });
    }
    simple(st, l, "toSet", vec![], env.one(env.set, ta.clone()));
    simple(st, l, "toVector", vec![], env.one(env.vector, ta.clone()));
    simple(st, l, "toSeq", vec![], env.one(env.seq, ta.clone()));
}

/// `groupBy` / `grouped` / `sliding`.
fn add_grouping(st: &mut SymbolTable, env: &Env) {
    let l = env.list;
    let ta = env.ta();
    let list_a = env.list_of(ta.clone());
    let map = env.map;
    {
        let ta2 = ta.clone();
        let la = list_a.clone();
        poly(st, l, "groupBy", &["K"], move |t| {
            let k = t[0].clone();
            (
                vec![vec![fn1(ta2, k.clone())]],
                Type::Class {
                    sym: map,
                    args: vec![k, la],
                },
            )
        });
    }
    let it_of_list = env.one(env.iterator, list_a);
    simple(st, l, "grouped", vec![Type::Int], it_of_list.clone());
    simple(st, l, "sliding", vec![Type::Int], it_of_list.clone());
    simple(st, l, "sliding", vec![Type::Int, Type::Int], it_of_list);
}

/// `Iterator.toList`, for folding the results of `grouped` / `sliding`.
/// JVM: `IterableOnceOps.toList:()Lscala/collection/immutable/List;`.
fn add_iterator_to_list(st: &mut SymbolTable, env: &Env) {
    let it = env.iterator;
    if it.is_none() || own_method(st, it, "toList").is_some() {
        return;
    }
    let Some(ia) = st.get(it).tparams.first().copied() else {
        return;
    };
    let ret = env.list_of(Type::TypeParam(ia));
    simple(st, it, "toList", vec![], ret);
}
