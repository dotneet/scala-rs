//! The conversion and aggregation methods of `ArrayOps`, and filling the holes in `scala.collection.MapView`.
//!
//! Called from exactly one place, `install_prelude` in `crates/typer/src/prelude.rs`
//! (only under `library_abi`). To avoid merge conflicts this is split out into a new
//! module rather than added to an existing file (`.agent-brief.md`'s policy).
//!
//! nsc 2.13.16's actual ABI is involved. The members that exist as `$extension`
//! statics on `scala.collection.ArrayOps` (`toSeq` / `groupBy` / `sortBy` /
//! `updated` / …) can simply be `invokestatic`'d, but
//! `toList` / `toSet` / `toVector` / `toBuffer` / `sum` / `product` /
//! `min` / `max` / `minBy` / `maxBy` / `mkString` / `reduce` /
//! `reduceLeft` **do not exist on `ArrayOps` itself**
//! (confirmed with `javap -s scala.collection.ArrayOps`).
//! What nsc actually does is wrap the `Array` into a
//! `scala.collection.mutable.ArraySeq` with `Predef.wrapXArray`
//! (`scala.LowPriorityImplicits`) and then call the default methods of
//! `scala.collection.IterableOnceOps`. The codegen side of this module
//! (`crates/backend/src/gen.rs`) matches that and `invokeinterface`s via `wrapXArray`.

use crate::prelude::{fn1, fn2, iface, method, module, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// Return `scala.collection.IterableOnce[A]`, reusing an existing one if there is one.
///
/// `add_array_ops_zip` (`prelude.rs`) has already created an `IterableOnce` directly
/// under `scala/collection` and added `IterableOnce[A]` to `List`'s `parents`, so
/// creating a **fresh** symbol of the same name here would stop `List` literals from
/// being passed as arguments (as in `List(1,2).concat(...)`). Look for the existing
/// symbol and create one only if it is missing (making `List` a subtype of it too).
/// It is reused for `zipAll`'s `Iterable[B]` parameter as well (for typechecking this
/// only loosens the view to `IterableOnce`; codegen still uses the real ABI
/// descriptor `Lscala/collection/Iterable;`, so run time is unaffected).
fn find_iterable_once(st: &mut SymbolTable, coll: SymbolId) -> SymbolId {
    let existing = st
        .lookup_member(coll, "IterableOnce")
        .into_iter()
        .find(|&id| st.get(id).kind == SymKind::Class);
    let ioc = existing.unwrap_or_else(|| {
        let ioc = iface(st, coll, "IterableOnce", "scala/collection/IterableOnce");
        let ia = type_param(st, ioc, "A");
        st.get_mut(ioc).tparams = vec![ia];
        ioc
    });
    if let Some(la) = st.get(st.list_sym).tparams.first().copied() {
        let parent = Type::Class {
            sym: ioc,
            args: vec![Type::TypeParam(la)],
        };
        if !st
            .get(st.list_sym)
            .parents
            .iter()
            .any(|p| matches!(p, Type::Class { sym, .. } if *sym == ioc))
        {
            st.get_mut(st.list_sym).parents.push(parent);
        }
    }
    ioc
}

/// The entry point called from `install_prelude`.
///
/// - `aops`: `scala.collection.ArrayOps[A]` (the symbol `add_array_ops` created)
/// - `tuple2`: `scala.Tuple2[A, B]`
/// - `ordering`: `scala.math.Ordering[T]` (the symbol `add_ordering` created; the
///   existing `Ordering.Int` / `Ordering.Char` instances are used as they are)
///
/// `add_map_and_vector` creates the `Map` symbol without returning it, so we look it
/// up ourselves as `scala.Map`.
pub(crate) fn install(st: &mut SymbolTable, aops: SymbolId, tuple2: SymbolId, ordering: SymbolId) {
    let map_sym = st
        .lookup_member(st.scala_pkg, "Map")
        .into_iter()
        .find(|&id| matches!(st.get(id).kind, SymKind::Class | SymKind::ModuleClass))
        .unwrap_or(SymbolId::NONE);
    let numeric = add_numeric(st);
    add_array_ops_simple_extensions(st, aops, tuple2, ordering);
    add_array_ops_wrapped_conversions(st, aops, ordering, numeric);
    add_map_view(st, map_sym, tuple2);
}

/// `scala.math.Numeric[T]` plus the implemented `implicit object Int`
/// (`scala/math/Numeric$IntIsIntegral$`) / `Long` / `Double`.
///
/// Used to resolve the `Numeric[B]` implicit argument of `sum` / `product`. Same
/// shape as `Ordering` (`add_ordering`): the companion module's members are
/// registered with `Flags::IMPLICIT` so that `search_implicit` can find them.
fn add_numeric(st: &mut SymbolTable) -> SymbolId {
    let math = crate::classpath::ensure_package(st, "scala/math");
    // `Numeric` may already exist (prelude_seq declares it for List.sum).
    if let Some(existing) = crate::classpath::find_by_jvm(st, "scala/math/Numeric") {
        return existing;
    }
    let numeric = iface(st, math, "Numeric", "scala/math/Numeric");
    let t = type_param(st, numeric, "T");
    st.get_mut(numeric).tparams = vec![t];
    let num_mod = module(st, math, "Numeric", "scala/math/Numeric$");
    let num_cls = st.module_class_of(num_mod);
    add_numeric_instance(
        st,
        num_cls,
        numeric,
        "IntIsIntegral",
        "scala/math/Numeric$IntIsIntegral$",
        Type::Int,
    );
    add_numeric_instance(
        st,
        num_cls,
        numeric,
        "LongIsIntegral",
        "scala/math/Numeric$LongIsIntegral$",
        Type::Long,
    );
    add_numeric_instance(
        st,
        num_cls,
        numeric,
        "DoubleIsFractional",
        "scala/math/Numeric$DoubleIsFractional$",
        Type::Double,
    );
    let mems = st.get(num_cls).members.clone();
    st.get_mut(num_mod).members.extend(mems);
    numeric
}

fn add_numeric_instance(
    st: &mut SymbolTable,
    num_cls: SymbolId,
    numeric: SymbolId,
    name: &str,
    jvm: &str,
    arg: Type,
) {
    let m = module(st, num_cls, name, jvm);
    st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
    st.get_mut(m).ty = Type::Class {
        sym: numeric,
        args: vec![arg.clone()],
    };
    let cls = st.module_class_of(m);
    st.get_mut(cls).parents = vec![Type::Class {
        sym: numeric,
        args: vec![arg],
    }];
}

/// The members that exist directly on `ArrayOps` as `$extension` statics
/// (nsc 2.13.16, confirmed with `javap -s scala.collection.ArrayOps`).
///
/// `toSeq$extension` / `toIndexedSeq$extension` / `groupBy$extension` /
/// `sortBy$extension` / `sorted$extension` / `sortWith$extension` /
/// `zipAll$extension` / `indexWhere$extension` / `lastIndexOf$extension` /
/// `patch$extension` / `updated$extension` / `appended$extension` /
/// `prepended$extension` / `concat$extension`
/// (`concat` is the real implementation of `++`; `$plus$plus$extension` has the same shape).
fn add_array_ops_simple_extensions(
    st: &mut SymbolTable,
    aops: SymbolId,
    tuple2: SymbolId,
    ordering: SymbolId,
) {
    let reflect = crate::classpath::ensure_package(st, "scala/reflect");
    let ct = st
        .lookup_member(reflect, "ClassTag")
        .into_iter()
        .find(|&id| st.get(id).kind == SymKind::Class)
        .unwrap_or(SymbolId::NONE);
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let array_a = Type::Array(Box::new(ta.clone()));

    // toSeq / toIndexedSeq: `Array[A] => Seq[A]` / `=> IndexedSeq[A]`.
    // `Seq` / `IndexedSeq` are already registered as `scala.Seq` / `scala.IndexedSeq`
    // (`add_seq_and_lazylist` / `add_indexedseq_and_queue`), so look them up.
    let seq = st
        .lookup_member(st.scala_pkg, "Seq")
        .into_iter()
        .find(|&id| matches!(st.get(id).kind, SymKind::Class | SymKind::ModuleClass))
        .unwrap_or(SymbolId::NONE);
    let idx_seq = st
        .lookup_member(st.scala_pkg, "IndexedSeq")
        .into_iter()
        .find(|&id| matches!(st.get(id).kind, SymKind::Class | SymKind::ModuleClass))
        .unwrap_or(SymbolId::NONE);
    method(
        st,
        aops,
        "toSeq",
        vec![],
        Type::Class {
            sym: seq,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "toIndexedSeq",
        vec![],
        Type::Class {
            sym: idx_seq,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );

    // groupBy[K](f: A => K): Map[K, Array[A]]
    let map_sym = st
        .lookup_member(st.scala_pkg, "Map")
        .into_iter()
        .find(|&id| matches!(st.get(id).kind, SymKind::Class | SymKind::ModuleClass))
        .unwrap_or(SymbolId::NONE);
    let gb = method(st, aops, "groupBy", vec![], Type::Unit, Intrinsic::None);
    let gk = type_param(st, gb, "K");
    let f = st.alloc("f", gb, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), Type::TypeParam(gk));
    st.get_mut(gb).tparams = vec![gk];
    st.get_mut(gb).params = vec![f];
    st.get_mut(gb).paramss = vec![vec![f]];
    st.get_mut(gb).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), Type::TypeParam(gk))]],
        ret: Box::new(Type::Class {
            sym: map_sym,
            args: vec![Type::TypeParam(gk), array_a.clone()],
        }),
    };

    // sortBy[B](f: A => B)(implicit ord: Ordering[B]): Array[A]
    let sb = method(st, aops, "sortBy", vec![], Type::Unit, Intrinsic::None);
    let sbk = type_param(st, sb, "B");
    let sbf = st.alloc("f", sb, SymKind::Term, Flags::PARAM, "");
    st.get_mut(sbf).ty = fn1(ta.clone(), Type::TypeParam(sbk));
    let sbev = st.alloc(
        "ord",
        sb,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(sbev).ty = Type::Class {
        sym: ordering,
        args: vec![Type::TypeParam(sbk)],
    };
    st.get_mut(sb).tparams = vec![sbk];
    st.get_mut(sb).params = vec![sbf, sbev];
    st.get_mut(sb).paramss = vec![vec![sbf], vec![sbev]];
    st.get_mut(sb).ty = Type::Method {
        paramss: vec![
            vec![fn1(ta.clone(), Type::TypeParam(sbk))],
            vec![Type::Class {
                sym: ordering,
                args: vec![Type::TypeParam(sbk)],
            }],
        ],
        ret: Box::new(array_a.clone()),
    };

    // sorted(implicit ord: Ordering[A]): Array[A]
    let sorted_ev = st.alloc(
        "ord",
        aops,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(sorted_ev).ty = Type::Class {
        sym: ordering,
        args: vec![ta.clone()],
    };
    let sorted_m = method(st, aops, "sorted", vec![], Type::Unit, Intrinsic::None);
    st.get_mut(sorted_m).params = vec![sorted_ev];
    st.get_mut(sorted_m).paramss = vec![vec![sorted_ev]];
    st.get_mut(sorted_m).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: ordering,
            args: vec![ta.clone()],
        }]],
        ret: Box::new(array_a.clone()),
    };

    // sortWith(lt: (A, A) => Boolean): Array[A]
    method(
        st,
        aops,
        "sortWith",
        vec![fn2(ta.clone(), ta.clone(), Type::Boolean)],
        array_a.clone(),
        Intrinsic::None,
    );

    // zipAll[B](that: Iterable[B], thisElem: A, thatElem: B): Array[(A, B)]
    let iterable_once = find_iterable_once(st, coll);
    let za = method(st, aops, "zipAll", vec![], Type::Unit, Intrinsic::None);
    let zb = type_param(st, za, "B");
    let that = st.alloc("that", za, SymKind::Term, Flags::PARAM, "");
    st.get_mut(that).ty = Type::Class {
        sym: iterable_once,
        args: vec![Type::TypeParam(zb)],
    };
    let this_elem = st.alloc("thisElem", za, SymKind::Term, Flags::PARAM, "");
    st.get_mut(this_elem).ty = ta.clone();
    let that_elem = st.alloc("thatElem", za, SymKind::Term, Flags::PARAM, "");
    st.get_mut(that_elem).ty = Type::TypeParam(zb);
    st.get_mut(za).tparams = vec![zb];
    st.get_mut(za).params = vec![that, this_elem, that_elem];
    st.get_mut(za).paramss = vec![vec![that, this_elem, that_elem]];
    let pair_ab = Type::Class {
        sym: tuple2,
        args: vec![ta.clone(), Type::TypeParam(zb)],
    };
    st.get_mut(za).ty = Type::Method {
        paramss: vec![vec![
            Type::Class {
                sym: iterable_once,
                args: vec![Type::TypeParam(zb)],
            },
            ta.clone(),
            Type::TypeParam(zb),
        ]],
        ret: Box::new(Type::Array(Box::new(pair_ab))),
    };

    // indexWhere(p: A => Boolean, from: Int = 0): Int — both arities so
    // `xs.indexWhere(p)` (the common case, `from` defaulting to 0) and the
    // explicit 2-arg form both resolve; codegen fills the missing `0`.
    method(
        st,
        aops,
        "indexWhere",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "indexWhere",
        vec![fn1(ta.clone(), Type::Boolean), Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    // lastIndexOf(elem: A, end: Int): Int
    method(
        st,
        aops,
        "lastIndexOf",
        vec![ta.clone(), Type::Int],
        Type::Int,
        Intrinsic::None,
    );

    // patch(from: Int, other: IterableOnce[A], replaced: Int)(implicit ClassTag[A]): Array[A]
    let iterable_once = find_iterable_once(st, coll);
    let patch_m = method(st, aops, "patch", vec![], Type::Unit, Intrinsic::None);
    let p_from = st.alloc("from", patch_m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(p_from).ty = Type::Int;
    let p_other = st.alloc("other", patch_m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(p_other).ty = Type::Class {
        sym: iterable_once,
        args: vec![ta.clone()],
    };
    let p_replaced = st.alloc("replaced", patch_m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(p_replaced).ty = Type::Int;
    let p_ev = st.alloc(
        "evidence$1",
        patch_m,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(p_ev).ty = Type::Class {
        sym: ct,
        args: vec![ta.clone()],
    };
    st.get_mut(patch_m).params = vec![p_from, p_other, p_replaced, p_ev];
    st.get_mut(patch_m).paramss = vec![vec![p_from, p_other, p_replaced], vec![p_ev]];
    st.get_mut(patch_m).ty = Type::Method {
        paramss: vec![
            vec![
                Type::Int,
                Type::Class {
                    sym: iterable_once,
                    args: vec![ta.clone()],
                },
                Type::Int,
            ],
            vec![Type::Class {
                sym: ct,
                args: vec![ta.clone()],
            }],
        ],
        ret: Box::new(array_a.clone()),
    };

    // updated(index: Int, elem: A)(implicit ClassTag[A]): Array[A]
    add_one_elem_ct_method(
        st,
        aops,
        ct,
        "updated",
        &[Type::Int, ta.clone()],
        &ta,
        &array_a,
    );
    // appended(elem: A)(implicit ClassTag[A]): Array[A]
    add_one_elem_ct_method(
        st,
        aops,
        ct,
        "appended",
        std::slice::from_ref(&ta),
        &ta,
        &array_a,
    );
    // prepended(elem: A)(implicit ClassTag[A]): Array[A]
    add_one_elem_ct_method(
        st,
        aops,
        ct,
        "prepended",
        std::slice::from_ref(&ta),
        &ta,
        &array_a,
    );

    // concat(suffix: IterableOnce[A])(implicit ClassTag[A]): Array[A], and its
    // `++` alias.
    for name in ["concat", "++"] {
        let m = method(st, aops, name, vec![], Type::Unit, Intrinsic::None);
        let suffix = st.alloc("suffix", m, SymKind::Term, Flags::PARAM, "");
        st.get_mut(suffix).ty = Type::Class {
            sym: iterable_once,
            args: vec![ta.clone()],
        };
        let ev = st.alloc(
            "evidence$1",
            m,
            SymKind::Term,
            Flags::PARAM.with(Flags::IMPLICIT),
            "",
        );
        st.get_mut(ev).ty = Type::Class {
            sym: ct,
            args: vec![ta.clone()],
        };
        st.get_mut(m).params = vec![suffix, ev];
        st.get_mut(m).paramss = vec![vec![suffix], vec![ev]];
        st.get_mut(m).ty = Type::Method {
            paramss: vec![
                vec![Type::Class {
                    sym: iterable_once,
                    args: vec![ta.clone()],
                }],
                vec![Type::Class {
                    sym: ct,
                    args: vec![ta.clone()],
                }],
            ],
            ret: Box::new(array_a.clone()),
        };
    }
}

fn add_one_elem_ct_method(
    st: &mut SymbolTable,
    aops: SymbolId,
    ct: SymbolId,
    name: &str,
    explicit: &[Type],
    elem: &Type,
    ret: &Type,
) {
    let m = method(st, aops, name, vec![], Type::Unit, Intrinsic::None);
    let mut explicit_ids = Vec::new();
    for (i, p) in explicit.iter().enumerate() {
        let id = st.alloc(format!("p{i}"), m, SymKind::Term, Flags::PARAM, "");
        st.get_mut(id).ty = p.clone();
        explicit_ids.push(id);
    }
    let ev = st.alloc(
        "evidence$1",
        m,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![elem.clone()],
    };
    let mut params = explicit_ids.clone();
    params.push(ev);
    st.get_mut(m).params = params;
    st.get_mut(m).paramss = vec![explicit_ids, vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            explicit.to_vec(),
            vec![Type::Class {
                sym: ct,
                args: vec![elem.clone()],
            }],
        ],
        ret: Box::new(ret.clone()),
    };
}

/// The members that do not exist on `ArrayOps` itself, which nsc calls as default
/// methods of `IterableOnceOps` after wrapping into a `mutable.ArraySeq` via
/// `Predef.wrapXArray`. The codegen side (`crates/backend/src/gen.rs`) inserts the
/// `wrapXArray` call right after evaluating the receiver.
fn add_array_ops_wrapped_conversions(
    st: &mut SymbolTable,
    aops: SymbolId,
    ordering: SymbolId,
    numeric: SymbolId,
) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);

    let list_t = Type::Class {
        sym: st.list_sym,
        args: vec![ta.clone()],
    };
    method(st, aops, "toList", vec![], list_t, Intrinsic::None);

    let set_sym = st
        .lookup_member(st.scala_pkg, "Set")
        .into_iter()
        .find(|&id| matches!(st.get(id).kind, SymKind::Class | SymKind::ModuleClass))
        .unwrap_or(SymbolId::NONE);
    method(
        st,
        aops,
        "toSet",
        vec![],
        Type::Class {
            sym: set_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );

    let vector_sym = st
        .lookup_member(st.scala_pkg, "Vector")
        .into_iter()
        .find(|&id| matches!(st.get(id).kind, SymKind::Class | SymKind::ModuleClass))
        .unwrap_or(SymbolId::NONE);
    method(
        st,
        aops,
        "toVector",
        vec![],
        Type::Class {
            sym: vector_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );

    let buffer_pkg = crate::classpath::ensure_package(st, "scala/collection/mutable");
    // Reuse the existing `Buffer`; a duplicate with the same JVM name breaks
    // `asScala`, whose result type is exactly this class.
    // `toBuffer` is not declared: a stub `scala/collection/mutable/Buffer`
    // becomes a package member that the classpath loader then refuses to
    // complete, which breaks `asScala` / `asJava`. Left out rather than
    // shipped broken.
    let _ = buffer_pkg;

    // mkString(): String / mkString(sep): String / mkString(start, sep, end): String
    method(st, aops, "mkString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        aops,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );

    // reduce / reduceLeft(op: (B, A) => B / (A, A) => A): B
    add_reduce_like(st, aops, &ta, "reduce");
    add_reduce_like(st, aops, &ta, "reduceLeft");

    // sum / product (implicit Numeric[B])
    add_numeric_fold(st, aops, &ta, numeric, "sum");
    add_numeric_fold(st, aops, &ta, numeric, "product");

    // min / max (implicit Ordering[B])
    add_ordering_pick(st, aops, &ta, ordering, "min");
    add_ordering_pick(st, aops, &ta, ordering, "max");

    // minBy / maxBy (f: A => B)(implicit Ordering[B])
    add_by_pick(st, aops, &ta, ordering, "minBy");
    add_by_pick(st, aops, &ta, ordering, "maxBy");
}

/// Real nsc signature is `reduce[B >: A](op: (B, B) => B): B` /
/// `reduceLeft[B >: A](op: (B, A) => B): B`; our simplified typer has no
/// notion of a lower-bounded free type param, and `B` defaulting to `A`
/// (the overwhelmingly common case, `xs.reduce(_ + _)`) is what nsc itself
/// infers here absent any other constraint, so model it directly as
/// `(A, A) => A` rather than leaving `B` unconstrained.
fn add_reduce_like(st: &mut SymbolTable, aops: SymbolId, ta: &Type, name: &str) {
    method(
        st,
        aops,
        name,
        vec![fn2(ta.clone(), ta.clone(), ta.clone())],
        ta.clone(),
        Intrinsic::None,
    );
}

fn add_numeric_fold(
    st: &mut SymbolTable,
    aops: SymbolId,
    ta: &Type,
    numeric: SymbolId,
    name: &str,
) {
    let m = method(st, aops, name, vec![], Type::Unit, Intrinsic::None);
    let ev = st.alloc(
        "num",
        m,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: numeric,
        args: vec![ta.clone()],
    };
    st.get_mut(m).params = vec![ev];
    st.get_mut(m).paramss = vec![vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: numeric,
            args: vec![ta.clone()],
        }]],
        ret: Box::new(ta.clone()),
    };
}

fn add_ordering_pick(
    st: &mut SymbolTable,
    aops: SymbolId,
    ta: &Type,
    ordering: SymbolId,
    name: &str,
) {
    let m = method(st, aops, name, vec![], Type::Unit, Intrinsic::None);
    let ev = st.alloc(
        "ord",
        m,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ordering,
        args: vec![ta.clone()],
    };
    st.get_mut(m).params = vec![ev];
    st.get_mut(m).paramss = vec![vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: ordering,
            args: vec![ta.clone()],
        }]],
        ret: Box::new(ta.clone()),
    };
}

fn add_by_pick(st: &mut SymbolTable, aops: SymbolId, ta: &Type, ordering: SymbolId, name: &str) {
    let m = method(st, aops, name, vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let f = st.alloc("f", m, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), tb.clone());
    let ev = st.alloc(
        "ord",
        m,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ordering,
        args: vec![tb.clone()],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![f, ev];
    st.get_mut(m).paramss = vec![vec![f], vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![fn1(ta.clone(), tb.clone())],
            vec![Type::Class {
                sym: ordering,
                args: vec![tb],
            }],
        ],
        ret: Box::new(ta.clone()),
    };
}

/// `scala.collection.MapView[K, V]` + `Map.view()`.
///
/// nsc 2.13.16: `MapView` is a trait extending `MapOps` and `View`, and `keys` /
/// `values` / `filterKeys` / `mapValues` are default methods on `MapView` itself.
/// `toMap` / `toList` / `toSeq` / `size` / `isEmpty` / `foreach` are default methods
/// on `IterableOnceOps` (which `MapView` also extends), so codegen
/// `invokeinterface`s them on `scala/collection/IterableOnceOps`.
/// `method`, unless the owner already declares a member with that name.
fn method_absent(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    params: Vec<Type>,
    ret: Type,
    ic: Intrinsic,
) -> SymbolId {
    if let Some(id) = st
        .get(owner)
        .members
        .iter()
        .copied()
        .find(|m| st.get(*m).name == name)
    {
        return id;
    }
    method(st, owner, name, params, ret, ic)
}

fn add_map_view(st: &mut SymbolTable, map_sym: SymbolId, tuple2: SymbolId) {
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    // `prelude_coll` may already have declared `MapView`; reuse it so calls do
    // not become ambiguous between two identical members.
    let existing = crate::classpath::find_by_jvm(st, "scala/collection/MapView");
    let mapview = match existing {
        Some(id) => id,
        None => iface(st, coll, "MapView", "scala/collection/MapView"),
    };
    let (mk, mv) = match st.get(mapview).tparams.as_slice() {
        [k, v] => (*k, *v),
        _ => {
            let k = type_param(st, mapview, "K");
            let v = type_param(st, mapview, "V");
            st.get_mut(mapview).tparams = vec![k, v];
            (k, v)
        }
    };
    let tk = Type::TypeParam(mk);
    let tv = Type::TypeParam(mv);
    let mapview_t = |v: Type| Type::Class {
        sym: mapview,
        args: vec![tk.clone(), v],
    };

    // Reuse the existing `Iterable`; a second one with the same JVM name
    // shadows the real members (`asJava` and friends stop resolving).
    let iterable = match crate::classpath::find_by_jvm(st, "scala/collection/Iterable") {
        Some(id) => id,
        None => iface(st, coll, "Iterable", "scala/collection/Iterable"),
    };
    let _it_a = match st.get(iterable).tparams.first() {
        Some(a) => *a,
        None => {
            let a = type_param(st, iterable, "A");
            st.get_mut(iterable).tparams = vec![a];
            a
        }
    };
    let iterable_t = |a: Type| Type::Class {
        sym: iterable,
        args: vec![a],
    };
    // Add at least toList/foreach on the Iterable side too (via IterableOnceOps), so
    // that `mv.keys.toList` / `mv.values.toList` go through.
    method_absent(
        st,
        iterable,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![it_a_type(st, iterable)],
        },
        Intrinsic::None,
    );
    method_absent(
        st,
        iterable,
        "foreach",
        vec![fn1(it_a_type(st, iterable), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, iterable, "size", vec![], Type::Int, Intrinsic::None);

    method_absent(
        st,
        mapview,
        "keys",
        vec![],
        iterable_t(tk.clone()),
        Intrinsic::None,
    );
    method_absent(
        st,
        mapview,
        "values",
        vec![],
        iterable_t(tv.clone()),
        Intrinsic::None,
    );
    method_absent(
        st,
        mapview,
        "filterKeys",
        vec![fn1(tk.clone(), Type::Boolean)],
        mapview_t(tv.clone()),
        Intrinsic::None,
    );

    let mvm = method_absent(
        st,
        mapview,
        "mapValues",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let w = type_param(st, mvm, "W");
    let tw = Type::TypeParam(w);
    let f = st.alloc("f", mvm, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(tv.clone(), tw.clone());
    st.get_mut(mvm).tparams = vec![w];
    st.get_mut(mvm).params = vec![f];
    st.get_mut(mvm).paramss = vec![vec![f]];
    st.get_mut(mvm).ty = Type::Method {
        paramss: vec![vec![fn1(tv.clone(), tw.clone())]],
        ret: Box::new(mapview_t(tw)),
    };

    // toMap: Map[K, V] -- codegen synthesises the `<:<` evidence with
    // `scala.$less$colon$less$.MODULE$.refl()`, so on the typer side this gets a
    // simple argument-less signature (a `MapView[K, V]`'s elements are always
    // (K, V), so `A <:< (K, V)` always holds).
    method_absent(
        st,
        mapview,
        "toMap",
        vec![],
        Type::Class {
            sym: map_sym,
            args: vec![tk.clone(), tv.clone()],
        },
        Intrinsic::None,
    );
    let pair = Type::Class {
        sym: tuple2,
        args: vec![tk.clone(), tv.clone()],
    };
    method_absent(
        st,
        mapview,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![pair.clone()],
        },
        Intrinsic::None,
    );
    let seq = st
        .lookup_member(st.scala_pkg, "Seq")
        .into_iter()
        .find(|&id| matches!(st.get(id).kind, SymKind::Class | SymKind::ModuleClass))
        .unwrap_or(SymbolId::NONE);
    method_absent(
        st,
        mapview,
        "toSeq",
        vec![],
        Type::Class {
            sym: seq,
            args: vec![pair],
        },
        Intrinsic::None,
    );
    method(st, mapview, "size", vec![], Type::Int, Intrinsic::None);
    method_absent(
        st,
        mapview,
        "isEmpty",
        vec![],
        Type::Boolean,
        Intrinsic::None,
    );
    method_absent(
        st,
        mapview,
        "foreach",
        vec![fn1(
            Type::Class {
                sym: tuple2,
                args: vec![tk.clone(), tv.clone()],
            },
            Type::Unit,
        )],
        Type::Unit,
        Intrinsic::None,
    );

    // `Map.view()` must be typed in terms of *Map's own* `K`/`V` type
    // params (not `MapView`'s, which `mapview_t`/`tk`/`tv` above are), or
    // substitution at a call site (`Map[Int, X].view` → `MapView[Int, X]`)
    // silently fails to replace them and the type param itself leaks
    // through (e.g. `grouped.view.mapValues(_.size)` sees `_: V` instead
    // of `_: Array[Int]`).
    let map_tparams = st.get(map_sym).tparams.clone();
    let (map_k, map_v) = (
        Type::TypeParam(map_tparams[0]),
        Type::TypeParam(map_tparams[1]),
    );
    method(
        st,
        map_sym,
        "view",
        vec![],
        Type::Class {
            sym: mapview,
            args: vec![map_k, map_v],
        },
        Intrinsic::None,
    );
}

fn it_a_type(st: &SymbolTable, iterable: SymbolId) -> Type {
    Type::TypeParam(st.get(iterable).tparams[0])
}
