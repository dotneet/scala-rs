//! Additional members for `scala.collection.{mutable,immutable}` container
//! types (`ArrayBuffer`, `ListBuffer`, `mutable.Map`/`Set`, `immutable.Map`/
//! `Set`/`Vector`, `Tuple2`). Only active in `library_abi` mode (linked
//! against a real scala-library jar) — see `crates/backend/src/gen.rs` for
//! the matching codegen (`is_stdlib_*` dispatch tables).
//!
//! Every signature here is checked against `javap -p` on
//! `scala-library-2.13.16.jar`; see the doc comment on each `add_*` function
//! for the real (erased) JVM descriptor being modeled.

use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

use crate::prelude::{fn1, fn2, iface, method, module, type_param};

fn find_class(st: &SymbolTable, owner: SymbolId, name: &str) -> SymbolId {
    st.get(owner)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == name && st.get(*id).kind == SymKind::Class)
        .unwrap_or(SymbolId::NONE)
}

/// `scala.collection.Iterable[A]` — the base read-only interface returned by
/// `Map.keys` / `Map.values` (`public default scala.collection.Iterable<K> keys();`
/// on `scala.collection.MapOps`). Modeled minimally: only the members needed
/// to consume the result (`foreach`, `mkString`, `toList`, `size`, `isEmpty`,
/// `iterator`), all inherited default methods on `IterableOnceOps`, called
/// via `invokeinterface` (see `is_stdlib_coll_iterable` in gen.rs).
fn find_or_create_iterable(st: &mut SymbolTable, iterator_sym: SymbolId) -> SymbolId {
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    // `add_array_ops_flat_map_from_array` in prelude.rs already allocates a
    // bare `scala.collection.Iterable[A]` (tparam only, no members) as an
    // implicit-conversion marker for `ArrayOps.flatMap`'s `asIterable`
    // evidence — reuse that symbol rather than creating a second, ambiguous
    // "Iterable" class, but still add our members onto it (it has none yet).
    let existing = find_class(st, coll, "Iterable");
    let it = if existing != SymbolId::NONE {
        existing
    } else {
        iface(st, coll, "Iterable", "scala/collection/Iterable")
    };
    if st.get(it).tparams.is_empty() {
        let a = type_param(st, it, "A");
        st.get_mut(it).tparams = vec![a];
    }
    let a = st.get(it).tparams[0];
    let ta = Type::TypeParam(a);
    method(
        st,
        it,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, it, "mkString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        it,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        it,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        it,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(st, it, "size", vec![], Type::Int, Intrinsic::None);
    method(st, it, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        it,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator_sym,
            args: vec![ta],
        },
        Intrinsic::None,
    );
    it
}

fn alloc_param(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    ty: Type,
    implicit: bool,
) -> SymbolId {
    let flags = if implicit {
        Flags::PARAM.with(Flags::IMPLICIT)
    } else {
        Flags::PARAM
    };
    let id = st.alloc(name, owner, SymKind::Term, flags, "");
    st.get_mut(id).ty = ty;
    id
}

/// Entry point, called once from `install_prelude` (library_abi only).
pub(crate) fn add_collections_extra(
    st: &mut SymbolTable,
    tuple2: SymbolId,
    ordering: SymbolId,
    iterator_sym: SymbolId,
) {
    let coll_iterable = find_or_create_iterable(st, iterator_sym);
    add_tuple2_extra(st, tuple2);
    add_array_buffer_extra(st, ordering, iterator_sym);
    add_list_buffer_extra(st, ordering, iterator_sym);
    add_mutable_set(st, iterator_sym);
    add_mutable_map(st, tuple2, iterator_sym, coll_iterable);
    add_immutable_map_extra(st, tuple2, ordering, iterator_sym, coll_iterable);
    add_immutable_set_extra(st, ordering, iterator_sym);
    add_vector_extra(st, ordering, iterator_sym);
}

/// `Tuple2.swap(): Tuple2[T2, T1]` — concrete, non-specialized overload.
/// javap: `public scala.Tuple2<T2, T1> swap();`
fn add_tuple2_extra(st: &mut SymbolTable, tuple2: SymbolId) {
    if tuple2 == SymbolId::NONE {
        return;
    }
    if st.get(tuple2).tparams.len() < 2 {
        return;
    }
    let t1 = st.get(tuple2).tparams[0];
    let t2 = st.get(tuple2).tparams[1];
    let swapped = Type::Class {
        sym: tuple2,
        args: vec![Type::TypeParam(t2), Type::TypeParam(t1)],
    };
    method(st, tuple2, "swap", vec![], swapped, Intrinsic::None);
    method(
        st,
        tuple2,
        "toString",
        vec![],
        Type::String,
        Intrinsic::None,
    );
}

/// nsc 2.13.16 `scala.collection.mutable.ArrayBuffer[A]` extras.
/// Members not already declared by `add_array_buffer` in prelude.rs
/// (`apply`, `update`, `+=`, companion `empty`/`apply`).
fn add_array_buffer_extra(st: &mut SymbolTable, ordering: SymbolId, iterator_sym: SymbolId) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let buf = find_class(st, mutp, "ArrayBuffer");
    if buf == SymbolId::NONE {
        return;
    }
    add_indexed_buffer_extra(st, buf, ordering, iterator_sym);
}

/// nsc 2.13.16 `scala.collection.mutable.ListBuffer[A]` extras.
fn add_list_buffer_extra(st: &mut SymbolTable, ordering: SymbolId, iterator_sym: SymbolId) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let buf = find_class(st, mutp, "ListBuffer");
    if buf == SymbolId::NONE {
        return;
    }
    add_indexed_buffer_extra(st, buf, ordering, iterator_sym);
}

/// Shared by `ArrayBuffer` and `ListBuffer`: both are concrete classes
/// (JVM name carried on `buf`) that either declare these directly or
/// inherit them as default methods from `IterableOnceOps` / `SeqOps` /
/// `Buffer` / `Growable` / `Shrinkable`. `crates/backend/src/gen.rs`
/// special-cases each by name (`is_stdlib_arraybuffer` / `is_stdlib_listbuffer`)
/// so the concrete owner class name is used with `invokevirtual` — the JVM
/// resolves the inherited default method (Java 8+ method resolution walks
/// superinterfaces too).
fn add_indexed_buffer_extra(
    st: &mut SymbolTable,
    buf: SymbolId,
    ordering: SymbolId,
    iterator_sym: SymbolId,
) {
    let ba = st.get(buf).tparams[0];
    let ta = Type::TypeParam(ba);
    let buf_t = Type::Class {
        sym: buf,
        args: vec![ta.clone()],
    };

    method(st, buf, "length", vec![], Type::Int, Intrinsic::None);
    method(st, buf, "size", vec![], Type::Int, Intrinsic::None);
    method(st, buf, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, buf, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, buf, "head", vec![], ta.clone(), Intrinsic::None);
    method(st, buf, "last", vec![], ta.clone(), Intrinsic::None);
    method(st, buf, "clear", vec![], Type::Unit, Intrinsic::None);
    method(
        st,
        buf,
        "remove",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "insert",
        vec![Type::Int, Type::Any],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "indexOf",
        vec![Type::Any],
        Type::Int,
        Intrinsic::None,
    );
    method(st, buf, "reverse", vec![], buf_t.clone(), Intrinsic::None);
    method(
        st,
        buf,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        buf_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "filter",
        vec![fn1(ta.clone(), Type::Boolean)],
        buf_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "append",
        vec![Type::Any],
        buf_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "++=",
        vec![Type::Any],
        buf_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "-=",
        vec![Type::Any],
        buf_t.clone(),
        Intrinsic::None,
    );

    method(st, buf, "mkString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        buf,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );

    // foldLeft[B](z: B)(op: (B, A) => B): B
    let m = method(st, buf, "foldLeft", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = alloc_param(st, m, "z", tb.clone(), false);
    let op = alloc_param(st, m, "op", fn2(tb.clone(), ta.clone(), tb.clone()), false);
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

    if ordering != SymbolId::NONE {
        // sortBy[B](f: A => B)(implicit ord: Ordering[B]): buf.type
        let m = method(st, buf, "sortBy", vec![], Type::Unit, Intrinsic::None);
        let b = type_param(st, m, "B");
        let tb = Type::TypeParam(b);
        let f = alloc_param(st, m, "f", fn1(ta.clone(), tb.clone()), false);
        let ord_ty = Type::Class {
            sym: ordering,
            args: vec![tb.clone()],
        };
        let ev = alloc_param(st, m, "ord", ord_ty.clone(), true);
        st.get_mut(m).tparams = vec![b];
        st.get_mut(m).params = vec![f, ev];
        st.get_mut(m).paramss = vec![vec![f], vec![ev]];
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![fn1(ta.clone(), tb)], vec![ord_ty]],
            ret: Box::new(buf_t.clone()),
        };

        // sorted(implicit ord: Ordering[A]): buf.type
        let m = method(st, buf, "sorted", vec![], Type::Unit, Intrinsic::None);
        let ord_ty = Type::Class {
            sym: ordering,
            args: vec![ta.clone()],
        };
        let ev = alloc_param(st, m, "ord", ord_ty.clone(), true);
        st.get_mut(m).params = vec![ev];
        st.get_mut(m).paramss = vec![vec![ev]];
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![ord_ty]],
            ret: Box::new(buf_t),
        };
    }
}

/// `scala.collection.mutable.Map[K, V]` — a NEW trait (not present before
/// this slice; only `mutable.HashMap` existed). javap:
/// `interface scala.collection.mutable.Map<K,V> extends ... MapOps<K,V,Map,Map<K,V>>`
/// companion `Map$` `extends MapFactory$Delegate<Map>` (delegates to `HashMap`
/// at runtime, but statically `Map(...)`/`Map.empty` return the trait type).
/// Every member below is a default method inherited from `MapOps` /
/// `mutable.MapOps` / `IterableOnceOps` / `Growable` / `Shrinkable`, called
/// via `invokeinterface` in gen.rs (`is_stdlib_mutable_map`).
fn add_mutable_map(
    st: &mut SymbolTable,
    tuple2: SymbolId,
    iterator_sym: SymbolId,
    coll_iterable: SymbolId,
) -> SymbolId {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    if tuple2 == SymbolId::NONE {
        return SymbolId::NONE;
    }
    let map = iface(st, mutp, "Map", "scala/collection/mutable/Map");
    let mk = type_param(st, map, "K");
    let mv = type_param(st, map, "V");
    st.get_mut(map).tparams = vec![mk, mv];
    let tk = Type::TypeParam(mk);
    let tv = Type::TypeParam(mv);
    let map_t = Type::Class {
        sym: map,
        args: vec![tk.clone(), tv.clone()],
    };
    let pair = Type::Class {
        sym: tuple2,
        args: vec![tk.clone(), tv.clone()],
    };
    let opt_v = Type::Class {
        sym: st.option_sym,
        args: vec![tv.clone()],
    };

    method(
        st,
        map,
        "apply",
        vec![tk.clone()],
        tv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "get",
        vec![tk.clone()],
        opt_v.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "update",
        vec![tk.clone(), tv.clone()],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        map,
        "contains",
        vec![tk.clone()],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        map,
        "keys",
        vec![],
        Type::Class {
            sym: coll_iterable,
            args: vec![tk.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        map,
        "values",
        vec![],
        Type::Class {
            sym: coll_iterable,
            args: vec![tv.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        map,
        "+=",
        vec![pair.clone()],
        map_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "-=",
        vec![tk.clone()],
        map_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "remove",
        vec![tk.clone()],
        opt_v.clone(),
        Intrinsic::None,
    );
    method(st, map, "size", vec![], Type::Int, Intrinsic::None);
    method(st, map, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, map, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, map, "clear", vec![], Type::Unit, Intrinsic::None);
    method(
        st,
        map,
        "foreach",
        vec![fn1(pair.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        map,
        "filter",
        vec![fn1(pair.clone(), Type::Boolean)],
        map_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![pair.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        map,
        "toSeq",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![pair.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        map,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator_sym,
            args: vec![pair.clone()],
        },
        Intrinsic::None,
    );
    method(st, map, "mkString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        map,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        map,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );

    // getOrElse[V1 >: V](key: K, default: => V1): V1 — modeled monomorphically as V.
    method(
        st,
        map,
        "getOrElse",
        vec![tk.clone(), Type::ByName(Box::new(tv.clone()))],
        tv.clone(),
        Intrinsic::None,
    );
    // getOrElseUpdate(key: K, op: => V): V
    method(
        st,
        map,
        "getOrElseUpdate",
        vec![tk.clone(), Type::ByName(Box::new(tv.clone()))],
        tv,
        Intrinsic::None,
    );

    let map_mod = module(st, mutp, "Map", "scala/collection/mutable/Map$");
    let map_cls = st.module_class_of(map_mod);
    method(
        st,
        map_cls,
        "empty",
        vec![],
        Type::Class {
            sym: map,
            args: vec![Type::Any, Type::Any],
        },
        Intrinsic::None,
    );
    let map_apply = method(
        st,
        map_cls,
        "apply",
        vec![Type::Repeated(Box::new(pair))],
        map_t,
        Intrinsic::None,
    );
    let mak = type_param(st, map_apply, "K");
    let mav = type_param(st, map_apply, "V");
    st.get_mut(map_apply).tparams = vec![mak, mav];
    let map_pair = Type::Class {
        sym: tuple2,
        args: vec![Type::TypeParam(mak), Type::TypeParam(mav)],
    };
    st.get_mut(map_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(map_pair))]],
        ret: Box::new(Type::Class {
            sym: map,
            args: vec![Type::TypeParam(mak), Type::TypeParam(mav)],
        }),
    };
    let mems = st.get(map_cls).members.clone();
    st.get_mut(map_mod).members.extend(mems);
    map
}

/// `scala.collection.mutable.Set[A]` — a NEW trait, companion delegates to
/// `HashSet` at runtime. Members are default methods on `SetOps` /
/// `mutable.SetOps` / `Growable` / `Shrinkable` / `IterableOnceOps`, called
/// via `invokeinterface` (`is_stdlib_mutable_set` in gen.rs).
fn add_mutable_set(st: &mut SymbolTable, iterator_sym: SymbolId) -> SymbolId {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let set = iface(st, mutp, "Set", "scala/collection/mutable/Set");
    let sa = type_param(st, set, "A");
    st.get_mut(set).tparams = vec![sa];
    let ta = Type::TypeParam(sa);
    let set_t = Type::Class {
        sym: set,
        args: vec![ta.clone()],
    };
    method(
        st,
        set,
        "contains",
        vec![ta.clone()],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        set,
        "+=",
        vec![ta.clone()],
        set_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        set,
        "-=",
        vec![ta.clone()],
        set_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        set,
        "remove",
        vec![ta.clone()],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, set, "size", vec![], Type::Int, Intrinsic::None);
    method(st, set, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, set, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, set, "clear", vec![], Type::Unit, Intrinsic::None);
    method(
        st,
        set,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        set,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        set_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        set,
        "filter",
        vec![fn1(ta.clone(), Type::Boolean)],
        set_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        set,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        set,
        "toSeq",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        set,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(st, set, "mkString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        set,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        set,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );

    let set_mod = module(st, mutp, "Set", "scala/collection/mutable/Set$");
    let set_cls = st.module_class_of(set_mod);
    method(
        st,
        set_cls,
        "empty",
        vec![],
        Type::Class {
            sym: set,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let set_apply = method(
        st,
        set_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        set_t,
        Intrinsic::None,
    );
    let saa = type_param(st, set_apply, "A");
    st.get_mut(set_apply).tparams = vec![saa];
    st.get_mut(set_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(saa)))]],
        ret: Box::new(Type::Class {
            sym: set,
            args: vec![Type::TypeParam(saa)],
        }),
    };
    let mems = st.get(set_cls).members.clone();
    st.get_mut(set_mod).members.extend(mems);
    set
}

/// `scala.collection.immutable.Map[K, V]` extras (base defined by
/// `add_map_and_vector` in prelude.rs: `apply`, `get`, `updated`, `+`,
/// `foreach`, companion `empty`/`apply`).
fn add_immutable_map_extra(
    st: &mut SymbolTable,
    tuple2: SymbolId,
    ordering: SymbolId,
    iterator_sym: SymbolId,
    coll_iterable: SymbolId,
) {
    let map = find_class(st, st.scala_pkg, "Map");
    if map == SymbolId::NONE || tuple2 == SymbolId::NONE {
        return;
    }
    let set = find_class(st, st.scala_pkg, "Set");
    let mk = st.get(map).tparams[0];
    let mv = st.get(map).tparams[1];
    let tk = Type::TypeParam(mk);
    let tv = Type::TypeParam(mv);
    let map_t = Type::Class {
        sym: map,
        args: vec![tk.clone(), tv.clone()],
    };
    let pair = Type::Class {
        sym: tuple2,
        args: vec![tk.clone(), tv.clone()],
    };

    method(
        st,
        map,
        "getOrElse",
        vec![Type::Any, Type::ByName(Box::new(tv.clone()))],
        tv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        map,
        "keys",
        vec![],
        Type::Class {
            sym: coll_iterable,
            args: vec![tk.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        map,
        "values",
        vec![],
        Type::Class {
            sym: coll_iterable,
            args: vec![tv.clone()],
        },
        Intrinsic::None,
    );
    if set != SymbolId::NONE {
        method(
            st,
            map,
            "keySet",
            vec![],
            Type::Class {
                sym: set,
                args: vec![tk.clone()],
            },
            Intrinsic::None,
        );
    }
    method(
        st,
        map,
        "-",
        vec![Type::Any],
        map_t.clone(),
        Intrinsic::None,
    );
    // `++` is intentionally NOT added here: `scala.collection.immutable.Map`
    // does not override `iterableFactory` (only `mapFactory`), so the
    // inherited `IterableOps.++`/`concat` default builds through the base
    // `immutable.Iterable` factory and does not reliably return a `Map` at
    // runtime for every backing implementation (verified empirically against
    // scala-library-2.13.16.jar: `Map(...) ++ Map(...)` throws
    // ClassCastException to `::` for both the 4-or-fewer-entries `MapN` and
    // larger `HashMap`-backed cases). `Set.++` does not have this problem
    // (verified working) and is supported below.
    method(st, map, "size", vec![], Type::Int, Intrinsic::None);
    method(st, map, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, map, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        map,
        "filter",
        vec![fn1(pair.clone(), Type::Boolean)],
        map_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![pair.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        map,
        "toSeq",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![pair.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        map,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator_sym,
            args: vec![pair.clone()],
        },
        Intrinsic::None,
    );
    method(st, map, "mkString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        map,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        map,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(st, map, "head", vec![], pair.clone(), Intrinsic::None);
    // foldLeft[B](z: B)(op: (B, (K, V)) => B): B
    let m = method(st, map, "foldLeft", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = alloc_param(st, m, "z", tb.clone(), false);
    let op = alloc_param(
        st,
        m,
        "op",
        fn2(tb.clone(), pair.clone(), tb.clone()),
        false,
    );
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(tb.clone(), pair.clone(), tb.clone())],
        ],
        ret: Box::new(tb),
    };
    method(
        st,
        map,
        "withDefaultValue",
        vec![Type::Any],
        map_t,
        Intrinsic::None,
    );

    // view: MapView[K, V]; view.mapValues[W](f: V => W): MapView[K, W]
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    let existing_view = find_class(st, coll, "MapView");
    let map_view = if existing_view != SymbolId::NONE {
        existing_view
    } else {
        let mv2 = iface(st, coll, "MapView", "scala/collection/MapView");
        let vk = type_param(st, mv2, "K");
        let vv = type_param(st, mv2, "V");
        st.get_mut(mv2).tparams = vec![vk, vv];
        mv2
    };
    let vk = st.get(map_view).tparams[0];
    let vv = st.get(map_view).tparams[1];
    // `view`'s return must reference *`map`'s own* K/V (`tk`/`tv`) so a
    // caller's `subst_tparams(map, [K, V], ...)` actually substitutes it —
    // `MapView`'s own fresh tparams (`vk`/`vv`) are only for members
    // declared directly on `MapView` (e.g. `mapValues` below).
    let view_t = Type::Class {
        sym: map_view,
        args: vec![tk.clone(), tv.clone()],
    };
    method(st, map, "view", vec![], view_t.clone(), Intrinsic::None);
    // `prelude_arrconv` may already declare `mapValues`; a second copy
    // would make every call ambiguous.
    if st.lookup_member(map_view, "mapValues").is_empty() {
        let mm = method(
            st,
            map_view,
            "mapValues",
            vec![],
            Type::Unit,
            Intrinsic::None,
        );
        let w = type_param(st, mm, "W");
        let tw = Type::TypeParam(w);
        let f = alloc_param(st, mm, "f", fn1(Type::TypeParam(vv), tw.clone()), false);
        st.get_mut(mm).tparams = vec![w];
        st.get_mut(mm).params = vec![f];
        st.get_mut(mm).paramss = vec![vec![f]];
        st.get_mut(mm).ty = Type::Method {
            paramss: vec![vec![fn1(Type::TypeParam(vv), tw.clone())]],
            ret: Box::new(Type::Class {
                sym: map_view,
                args: vec![Type::TypeParam(vk), tw],
            }),
        };
    }
    method(
        st,
        map_view,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![Type::Class {
                sym: tuple2,
                args: vec![Type::TypeParam(vk), Type::TypeParam(vv)],
            }],
        },
        Intrinsic::None,
    );
    method(
        st,
        map_view,
        "mkString",
        vec![],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        map_view,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        map_view,
        "foreach",
        vec![fn1(
            Type::Class {
                sym: tuple2,
                args: vec![Type::TypeParam(vk), Type::TypeParam(vv)],
            },
            Type::Unit,
        )],
        Type::Unit,
        Intrinsic::None,
    );

    let _ = ordering;
}

/// `scala.collection.immutable.Set[A]` extras (base defined by `add_set` in
/// prelude.rs: `contains`, `foreach`, companion `empty`/`apply`).
fn add_immutable_set_extra(st: &mut SymbolTable, _ordering: SymbolId, iterator_sym: SymbolId) {
    let set = find_class(st, st.scala_pkg, "Set");
    if set == SymbolId::NONE {
        return;
    }
    let sa = st.get(set).tparams[0];
    let ta = Type::TypeParam(sa);
    let set_t = Type::Class {
        sym: set,
        args: vec![ta.clone()],
    };
    method(
        st,
        set,
        "+",
        vec![ta.clone()],
        set_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        set,
        "-",
        vec![Type::Any],
        set_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        set,
        "++",
        vec![Type::Class {
            sym: set,
            args: vec![ta.clone()],
        }],
        set_t.clone(),
        Intrinsic::None,
    );
    method(st, set, "size", vec![], Type::Int, Intrinsic::None);
    method(st, set, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, set, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        set,
        "filter",
        vec![fn1(ta.clone(), Type::Boolean)],
        set_t,
        Intrinsic::None,
    );
    method(
        st,
        set,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        Type::Class {
            sym: set,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    method(
        st,
        set,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        set,
        "toSeq",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        set,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(st, set, "mkString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        set,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        set,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(st, set, "head", vec![], ta, Intrinsic::None);
}

/// `scala.collection.immutable.Vector[A]` extras (base defined by
/// `add_map_and_vector`: `apply`, `length`, `updated`, `:+`, `foreach`,
/// companion `empty`/`apply`).
fn add_vector_extra(st: &mut SymbolTable, _ordering: SymbolId, iterator_sym: SymbolId) {
    let vec = find_class(st, st.scala_pkg, "Vector");
    if vec == SymbolId::NONE {
        return;
    }
    let va = st.get(vec).tparams[0];
    let ta = Type::TypeParam(va);
    let vec_t = Type::Class {
        sym: vec,
        args: vec![ta.clone()],
    };
    method(st, vec, "size", vec![], Type::Int, Intrinsic::None);
    method(st, vec, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, vec, "nonEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, vec, "head", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        vec,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        vec_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        vec,
        "filter",
        vec![fn1(ta.clone(), Type::Boolean)],
        vec_t,
        Intrinsic::None,
    );
    method(
        st,
        vec,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        vec,
        "toSeq",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        vec,
        "iterator",
        vec![],
        Type::Class {
            sym: iterator_sym,
            args: vec![ta.clone()],
        },
        Intrinsic::None,
    );
    method(st, vec, "mkString", vec![], Type::String, Intrinsic::None);
    method(
        st,
        vec,
        "mkString",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        vec,
        "mkString",
        vec![Type::String, Type::String, Type::String],
        Type::String,
        Intrinsic::None,
    );

    let m = method(st, vec, "foldLeft", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, m, "B");
    let tb = Type::TypeParam(b);
    let z = alloc_param(st, m, "z", tb.clone(), false);
    let op = alloc_param(st, m, "op", fn2(tb.clone(), ta, tb.clone()), false);
    st.get_mut(m).tparams = vec![b];
    st.get_mut(m).params = vec![z, op];
    st.get_mut(m).paramss = vec![vec![z], vec![op]];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![
            vec![tb.clone()],
            vec![fn2(tb.clone(), Type::TypeParam(va), tb.clone())],
        ],
        ret: Box::new(tb),
    };
}
