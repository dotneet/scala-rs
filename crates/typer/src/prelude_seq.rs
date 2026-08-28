//! `scala.collection.immutable.List` のコアメンバ（scala-library 2.13.16 ABI）。
//!
//! `prelude.rs` からは [`add_list_core`] を 1 行呼ぶだけにしてある。
//! ここで宣言するシグネチャは `javap -s` で確認した 2.13.16 の実 descriptor に
//! 対応しており、対応する invoke は `crates/backend/src/gen.rs` の
//! `emit_list_core_member` が出す。
//!
//! 実 `List` 自身が持たないメンバ（`size` / `mkString` / `sum` / `groupBy` /
//! `sortBy` …）は `IterableOnceOps` / `IterableOps` / `SeqOps` の
//! default メソッドなので、戻り値が `Object` に erase される。gen.rs 側で
//! checkcast / unbox する。
//!
//! **私有ランタイム（`--no-scala-library`）では何も足さない。**
//! 私有 `List` classfile はこれらのメソッドを持たないので、非 jar モードでは
//! 従来どおり `value X is not a member of List[A]` の診断が出る。

use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

// ---------------------------------------------------------------------------
// 小さなヘルパ（prelude.rs のものと同等。衝突を避けるためこのファイルに閉じる）
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

/// 単純メソッド（型パラメータ・暗黙引数なし）。
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

/// 複数引数リスト（2 番目以降は implicit 扱い）＋メソッド型パラメータを持つ
/// メソッドを、既存シンボルがあればその場で書き換え、無ければ新規に作る。
///
/// `reuse` が `Some(id)` のときは `id` を書き換える（`map` のような既存の
/// 近似シグネチャを真に多相なものへ差し替える用途）。
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
            // 使い回すシンボルは型パラメータ・引数を作り直す。
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

/// 暗黙引数リストを持つメソッド。`implicit_from` 番目以降の引数リストが implicit。
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

/// `owner` 直下（継承は見ない）の同名メソッドのうち最初のもの。
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

/// `object <name>` を `owner` の下に作る（既にあればそれを返す）。
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

/// `implicit object <name> extends <tc>[<arg>]`（`<jvm>.MODULE$`）。
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

/// 型構築に使う周辺シンボル。
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
        Type::Class {
            sym,
            args: vec![t],
        }
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

/// `List` のコアメンバを scala-library 2.13.16 の実シグネチャで足す。
/// **`library_abi` のときだけ**呼ぶこと。
pub(crate) fn add_list_core(st: &mut SymbolTable) {
    let list = st.list_sym;
    let Some(a) = st.get(list).tparams.first().copied() else {
        return;
    };

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
    // `List` は 2.13 で `scala.collection.Iterable` / `IterableOnce`。
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
    st.get_mut(cls).parents.push(Type::Class { sym: parent, args });
}

/// `scala.math.Numeric` と `sum` / `product` 用の implicit インスタンス。
/// JVM: `scala/math/Numeric$IntIsIntegral$.MODULE$` 等。
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
    ] {
        add_implicit_instance(st, num_cls, numeric, name, jvm, ty);
    }
    let mems = st.get(num_cls).members.clone();
    st.get_mut(num_mod).members.extend(mems);
    numeric
}

/// `sorted` / `max` / `sortBy` 用に `Ordering` の implicit インスタンスを増やす
/// （`Int` / `Char` は prelude.rs 側で既に入っている）。
fn add_ordering_instances(st: &mut SymbolTable, ordering: SymbolId) {
    let math = crate::classpath::ensure_package(st, "scala/math");
    let (ord_mod, ord_cls) = find_or_make_module(st, math, "Ordering", "scala/math/Ordering$");
    for (name, jvm, ty) in [
        ("String", "scala/math/Ordering$String$", Type::String),
        ("Long", "scala/math/Ordering$Long$", Type::Long),
        ("Boolean", "scala/math/Ordering$Boolean$", Type::Boolean),
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

/// `map` / `flatMap` / `collect` を真に多相なシグネチャへ差し替える。
///
/// JVM (2.13.16, いずれも `List` 自身の virtual):
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

    // `collect[B](pf: PartialFunction[A, B]): List[B]`。
    // 型注釈付きの `val pf: PartialFunction[Int, String]` を渡す形（ArrayOps の
    // `collect` と同じ）で B が決まる。インラインの `{ case … }` リテラルを
    // 直接渡す形は typer 側が未対応（ArrayOps でも同様）。
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

    // `::` / `:::` / `+:` / `:+` / `++` も `B >: A` で多相。
    let ta = env.ta();
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
    let _ = ta;

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

/// `filter` 系と部分列。すべて `List` 自身の virtual（一部は erase される）。
///
/// JVM: `filter`/`filterNot`/`takeWhile:(Function1)List`, `take`/`takeRight`:`(I)List`,
/// `slice:(II)List`, `drop:(I)LinearSeq`, `dropWhile:(Function1)LinearSeq`,
/// `dropRight:(I)Object`, `splitAt:(I)Tuple2`, `span`/`partition:(Function1)Tuple2`,
/// `distinct` は `SeqOps.distinct:()Object`。
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
    simple(
        st,
        l,
        "slice",
        vec![Type::Int, Type::Int],
        list_a.clone(),
    );
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

/// 述語・検索・畳み込み。
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
            vec![
                vec![b.clone()],
                vec![fn2(b.clone(), tb.clone(), b.clone())],
            ],
            b,
        )
    });
    let tb = ta.clone();
    poly(st, l, "foldRight", &["B"], |t| {
        let b = t[0].clone();
        (
            vec![
                vec![b.clone()],
                vec![fn2(tb.clone(), b.clone(), b.clone())],
            ],
            b,
        )
    });
    let tb = ta.clone();
    let l2 = l;
    poly(st, l, "scanLeft", &["B"], |t| {
        let b = t[0].clone();
        (
            vec![
                vec![b.clone()],
                vec![fn2(b.clone(), tb.clone(), b.clone())],
            ],
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

/// `mkString` / `sum` / `product` / `min` / `max` / `minBy` / `maxBy`。
/// すべて `IterableOnceOps` の default メソッド。
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

/// `sorted` / `sortBy` / `sortWith` / `zip` / `zipWithIndex`。
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

/// `toArray` / `toSet` / `toVector` / `toSeq` / `toIndexedSeq`。
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

/// `groupBy` / `grouped` / `sliding`。
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
    simple(
        st,
        l,
        "sliding",
        vec![Type::Int, Type::Int],
        it_of_list,
    );
}

/// `grouped` / `sliding` の結果を畳むための `Iterator.toList`。
/// JVM: `IterableOnceOps.toList:()Lscala/collection/immutable/List;`。
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
