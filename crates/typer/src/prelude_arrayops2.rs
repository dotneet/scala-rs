use crate::prelude::{class, fn1, fn2, iface, method, module, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn add_array_ops(st: &mut SymbolTable) -> SymbolId {
    let aops = class(
        st,
        st.scala_pkg,
        "ArrayOps",
        "scala/collection/ArrayOps",
        &[Type::AnyVal],
    );
    let a = type_param(st, aops, "A");
    st.get_mut(aops).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let xs = st.alloc("xs", aops, SymKind::Term, Flags::PARAM, "");
    st.get_mut(xs).ty = Type::Array(Box::new(ta.clone()));
    st.get_mut(aops).ctor_fields = vec![xs];
    method(st, aops, "head", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        aops,
        "tail",
        vec![],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "filter",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "take",
        vec![Type::Int],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "drop",
        vec![Type::Int],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "dropWhile",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "exists",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "count",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "forall",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "slice",
        vec![Type::Int, Type::Int],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(st, aops, "last", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        aops,
        "init",
        vec![],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "reverse",
        vec![],
        Type::Array(Box::new(ta)),
        Intrinsic::None,
    );
    method(st, aops, "size", vec![], Type::Int, Intrinsic::None);
    method(st, aops, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, aops, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    aops
}
/// `ArrayOps.map[B](f: A => B)(implicit ClassTag[B]): Array[B]` (scala-library 2.13).
pub(crate) fn add_array_ops_map(st: &mut SymbolTable, aops: SymbolId, ct: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "map", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let f = st.alloc("f", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), Type::TypeParam(b));
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::TypeParam(b)],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![f, ev];
    st.get_mut(m).paramss = vec![vec![f], vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![fn1(ta, Type::TypeParam(b))],
            vec![Type::Class {
                sym: ct,
                args: vec![Type::TypeParam(b)],
            }],
        ],
        ret: Box::new(Type::Array(Box::new(Type::TypeParam(b)))),
    };
}
/// `ArrayOps.flatMap[B](f: A => Any)(implicit ClassTag[B]): Array[B]`.
/// Dual-run uses `List` (IterableOnce); the Array→Iterable 4-arg overload is
/// `add_array_ops_flat_map_from_array`.
pub(crate) fn add_array_ops_flat_map(st: &mut SymbolTable, aops: SymbolId, ct: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "flatMap", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let f = st.alloc("f", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), Type::Any);
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::TypeParam(b)],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![f, ev];
    st.get_mut(m).paramss = vec![vec![f], vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![fn1(ta, Type::Any)],
            vec![Type::Class {
                sym: ct,
                args: vec![Type::TypeParam(b)],
            }],
        ],
        ret: Box::new(Type::Array(Box::new(Type::TypeParam(b)))),
    };
}
/// `ArrayOps.flatMap[BS, B](f: A => BS)(implicit asIterable: BS => Iterable[B], m: ClassTag[B])`.
/// nsc 2.13.16 4-arg JVM: `flatMap$extension(Object, Function1, Function1, ClassTag)Object`.
pub(crate) fn add_array_ops_flat_map_from_array(
    st: &mut SymbolTable,
    aops: SymbolId,
    ct: SymbolId,
) {
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    let iterable = iface(st, coll, "Iterable", "scala/collection/Iterable");
    if st.get(iterable).tparams.is_empty() {
        let ia = type_param(st, iterable, "A");
        st.get_mut(iterable).tparams = vec![ia];
    }
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let _of_int = class(
        st,
        mutp,
        "ArraySeq$ofInt",
        "scala/collection/mutable/ArraySeq$ofInt",
        &[Type::Class {
            sym: iterable,
            args: vec![Type::Int],
        }],
    );
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "flatMap", vec![], Type::Unit, Intrinsic::None);
    let bs = type_param(st, m, "BS");
    let b = type_param(st, m, "B");
    let f = st.alloc("f", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), Type::TypeParam(bs));
    let as_it = st.alloc(
        "asIterable",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(as_it).ty = fn1(
        Type::TypeParam(bs),
        Type::Class {
            sym: iterable,
            args: vec![Type::TypeParam(b)],
        },
    );
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::TypeParam(b)],
    };
    st.get_mut(m).tparams = vec![bs, b];
    st.get_mut(m).params = vec![f, as_it, ev];
    st.get_mut(m).paramss = vec![vec![f], vec![as_it, ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![fn1(ta, Type::TypeParam(bs))],
            vec![
                fn1(
                    Type::TypeParam(bs),
                    Type::Class {
                        sym: iterable,
                        args: vec![Type::TypeParam(b)],
                    },
                ),
                Type::Class {
                    sym: ct,
                    args: vec![Type::TypeParam(b)],
                },
            ],
        ],
        ret: Box::new(Type::Array(Box::new(Type::TypeParam(b)))),
    };
}
/// `ArrayOps.collect[B](pf: PartialFunction[A, B])(implicit ClassTag[B]): Array[B]`.
/// nsc 2.13.16 JVM: `collect$extension(Object, PartialFunction, ClassTag)Object`.
pub(crate) fn add_array_ops_collect(st: &mut SymbolTable, aops: SymbolId, ct: SymbolId) {
    let pf = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "PartialFunction")
        .unwrap_or(SymbolId::NONE);
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "collect", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let f = st.alloc("pf", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = Type::Class {
        sym: pf,
        args: vec![ta.clone(), Type::TypeParam(b)],
    };
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::TypeParam(b)],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![f, ev];
    st.get_mut(m).paramss = vec![vec![f], vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![Type::Class {
                sym: pf,
                args: vec![ta, Type::TypeParam(b)],
            }],
            vec![Type::Class {
                sym: ct,
                args: vec![Type::TypeParam(b)],
            }],
        ],
        ret: Box::new(Type::Array(Box::new(Type::TypeParam(b)))),
    };
}
/// `ArrayOps.zip[B](that: IterableOnce[B]): Array[(A, B)]`.
/// nsc 2.13.16 JVM: `zip$extension(Object, IterableOnce)Tuple2[]`.
pub(crate) fn add_array_ops_zip(st: &mut SymbolTable, aops: SymbolId, tuple2: SymbolId) {
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    let ioc = iface(st, coll, "IterableOnce", "scala/collection/IterableOnce");
    if st.get(ioc).tparams.is_empty() {
        let ia = type_param(st, ioc, "A");
        st.get_mut(ioc).tparams = vec![ia];
    }
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
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "zip", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let that = st.alloc("that", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(that).ty = Type::Class {
        sym: ioc,
        args: vec![Type::TypeParam(b)],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![that];
    st.get_mut(m).paramss = vec![vec![that]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![Type::Class {
            sym: ioc,
            args: vec![Type::TypeParam(b)],
        }]],
        ret: Box::new(Type::Array(Box::new(Type::Class {
            sym: tuple2,
            args: vec![ta, Type::TypeParam(b)],
        }))),
    };
}
/// ArrayOps.foldLeft / fold / foldRight.
///
/// nsc 2.13.16 JVM: the `foldLeft$extension(Object, Object, Function2)Object` family.
/// `reduce` is not on ArrayOps, so it is not included.
pub(crate) fn add_array_ops_folds(st: &mut SymbolTable, aops: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);

    let m = method(st, aops, "foldLeft", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(tb.clone(), ta.clone(), tb.clone());
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(tb.clone(), ta.clone(), tb.clone())],
        ],
        ret: Box::new(tb),
    };

    let m = method(st, aops, "fold", vec![], Type::Unit, Intrinsic::None);
    let a1 = type_param(st, m, "A1");
    let ta1 = Type::TypeParam(a1);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = ta1.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(ta1.clone(), ta1.clone(), ta1.clone());
    st.get_mut(m).tparams = vec![a1];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![ta1.clone()],
            vec![fn2(ta1.clone(), ta1.clone(), ta1.clone())],
        ],
        ret: Box::new(ta1),
    };

    let m = method(st, aops, "foldRight", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(ta.clone(), tb.clone(), tb.clone());
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![vec![tb.clone()], vec![fn2(ta, tb.clone(), tb.clone())]],
        ret: Box::new(tb),
    };
}
/// ArrayOps.scanLeft[B: ClassTag](z: B)(op: (B, A) => B): Array[B]
///
/// nsc 2.13.16 JVM: `scanLeft$extension(Object, Object, Function2, ClassTag)Object`.
pub(crate) fn add_array_ops_scan_left(st: &mut SymbolTable, aops: SymbolId) {
    let reflect = crate::classpath::ensure_package(st, "scala/reflect");
    let ct = st
        .lookup_member(reflect, "ClassTag")
        .into_iter()
        .find(|&id| st.get(id).kind == crate::symbol::SymKind::Class)
        .unwrap_or(SymbolId::NONE);
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let m = method(st, aops, "scanLeft", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = st.alloc("z", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(z).ty = tb.clone();
    let op = st.alloc("op", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(op).ty = fn2(tb.clone(), ta.clone(), tb.clone());
    let ev = st.alloc(
        "evidence$1",
        m,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![tb.clone()],
    };
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op, ev];
    st.get_mut(m).paramss = vec![vec![z], vec![op], vec![ev]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(tb.clone(), ta, tb.clone())],
            vec![Type::Class {
                sym: ct,
                args: vec![tb.clone()],
            }],
        ],
        ret: Box::new(Type::Array(Box::new(tb))),
    };
}
/// ArrayOps.find / contains / distinct / takeRight / dropRight / takeWhile /
/// indices / lengthCompare against 2.13.16.
///
/// JVM: `find$extension(Object, Function1)Option`,
/// `contains$extension(Object, Object)Z`, `distinct$extension(Object)Object`,
/// `takeRight$extension` / `dropRight$extension` `(Object, I)Object`,
/// `takeWhile$extension(Object, Function1)Object`,
/// `indices$extension(Object)Range`, `lengthCompare$extension(Object, I)I`.
pub(crate) fn add_array_ops_remaining(st: &mut SymbolTable, aops: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    method(
        st,
        aops,
        "find",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Class {
            sym: st.option_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "contains",
        vec![ta.clone()],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "distinct",
        vec![],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "takeRight",
        vec![Type::Int],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "dropRight",
        vec![Type::Int],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "takeWhile",
        vec![fn1(ta.clone(), Type::Boolean)],
        Type::Array(Box::new(ta.clone())),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "lengthCompare",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    let range = st
        .lookup_member(st.scala_pkg, "Range")
        .into_iter()
        .find(|&id| st.get(id).kind == crate::symbol::SymKind::Class)
        .unwrap_or(SymbolId::NONE);
    method(
        st,
        aops,
        "indices",
        vec![],
        Type::Class {
            sym: range,
            args: vec![],
        },
        Intrinsic::None,
    );
}
/// ArrayOps.filterNot / headOption / lastOption / partition / splitAt / span
/// against 2.13.16.
///
/// JVM: `filterNot$extension(Object, Function1)Object`,
/// `headOption$extension` / `lastOption$extension(Object)Option`,
/// `partition$extension` / `span$extension(Object, Function1)Tuple2`,
/// `splitAt$extension(Object, I)Tuple2`.
pub(crate) fn add_array_ops_filter_not_opts_part(
    st: &mut SymbolTable,
    aops: SymbolId,
    tuple2: SymbolId,
) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    let arr = Type::Array(Box::new(ta.clone()));
    method(
        st,
        aops,
        "filterNot",
        vec![fn1(ta.clone(), Type::Boolean)],
        arr.clone(),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "headOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "lastOption",
        vec![],
        Type::Class {
            sym: st.option_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    let pair = Type::Class {
        sym: tuple2,
        args: vec![arr.clone(), arr],
    };
    method(
        st,
        aops,
        "partition",
        vec![fn1(ta.clone(), Type::Boolean)],
        pair.clone(),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "splitAt",
        vec![Type::Int],
        pair.clone(),
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "span",
        vec![fn1(ta, Type::Boolean)],
        pair,
        Intrinsic::None,
    );
}
/// ArrayOps.zipWithIndex / knownSize / sizeCompare against 2.13.16.
///
/// JVM: `zipWithIndex$extension(Object)[Lscala/Tuple2;`,
/// `knownSize$extension(Object)I`, `sizeCompare$extension(Object, I)I`.
pub(crate) fn add_array_ops_zip_index_size(st: &mut SymbolTable, aops: SymbolId, tuple2: SymbolId) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    method(
        st,
        aops,
        "zipWithIndex",
        vec![],
        Type::Array(Box::new(Type::Class {
            sym: tuple2,
            args: vec![ta, Type::Int],
        })),
        Intrinsic::None,
    );
    method(st, aops, "knownSize", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        aops,
        "sizeCompare",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
}
/// ArrayOps.lengthIs / sizeIs / indexOf / copyToArray / iterator against 2.13.16.
///
/// JVM: `lengthIs$extension` / `sizeIs$extension(Object)I`,
/// `indexOf$extension(Object, Object, I)I`,
/// `copyToArray$extension(Object, Object)I`,
/// `iterator$extension(Object)Iterator`.
pub(crate) fn add_array_ops_length_index_copy(
    st: &mut SymbolTable,
    aops: SymbolId,
    iterator: SymbolId,
) {
    let a = st.get(aops).tparams[0];
    let ta = Type::TypeParam(a);
    method(st, aops, "lengthIs", vec![], Type::Int, Intrinsic::None);
    method(st, aops, "sizeIs", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        aops,
        "indexOf",
        vec![ta.clone(), Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "copyToArray",
        vec![Type::Array(Box::new(ta.clone()))],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        aops,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator,
            args: vec![ta],
        },
        Intrinsic::None,
    );
}
/// `scala.Array` companion from scala-library. Do not emit `Array$.class`.
pub(crate) fn add_array_companion(st: &mut SymbolTable, ct: SymbolId) {
    let am = module(st, st.scala_pkg, "Array", "scala/Array$");
    let mc = st.module_class_of(am);
    let apply = method(
        st,
        mc,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        Type::Array(Box::new(Type::Any)),
        Intrinsic::None,
    );
    let t = type_param(st, apply, "T");
    let xs = st.alloc("xs", apply, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(xs).ty = Type::Repeated(Box::new(Type::TypeParam(t)));
    let ev = st.alloc(
        "evidence$1",
        apply,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ct,
        args: vec![Type::TypeParam(t)],
    };
    st.get_mut(apply).tparams = vec![t];
    st.get_mut(apply).params = vec![xs, ev];
    st.get_mut(apply).paramss = vec![vec![xs], vec![ev]];
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![
            vec![Type::Repeated(Box::new(Type::TypeParam(t)))],
            vec![Type::Class {
                sym: ct,
                args: vec![Type::TypeParam(t)],
            }],
        ],
        ret: Box::new(Type::Array(Box::new(Type::TypeParam(t)))),
    };
    let mems = st.get(mc).members.clone();
    st.get_mut(am).members.extend(mems);
}
