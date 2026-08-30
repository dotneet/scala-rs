//! The `IterableFactory` / `MapFactory` edge every collection companion has,
//! and the `Factory` evidence 2.13 derives from it.
//!
//! `xs.to(Vector)` is `IterableOps.to[C1](factory: Factory[A, C1]): C1`, and
//! `Vector` is a *companion object*, not a `Factory`. What makes the call type
//! is `object IterableFactory { implicit def toFactory[A, CC[_]](factory:
//! IterableFactory[CC]): Factory[A, CC[A]] }` — a view whose parameter is the
//! companion read at `IterableFactory`. The prelude declares those companions
//! itself (`prelude.rs`, `prelude_coll.rs`, `prelude_mutcoll.rs`) and left
//! them extending nothing, so the view's parameter never matched and every
//! `to(…)` reported
//!
//! ```text
//! error: no matching overload for (Factory[Int, C1])C1 with arguments (Vector$)
//! ```
//!
//! Only the edge is declared here. The conversion itself, and `Factory`, come
//! from the jar; `--no-scala-library` has neither, so nothing is installed
//! there and the existing diagnostic stands.
//!
//! Checked against `javap -p -s` on scala-library-2.13.16:
//! `object Vector extends StrictOptimizedSeqFactory[Vector]` — whose base
//! `scala.collection.IterableFactory<Vector>` is what
//! `IterableFactory.toFactory` takes — and, for maps,
//! `object Map extends MapFactory$Delegate<Map>`, a
//! `scala.collection.MapFactory<Map>`.
//!
//! The sorted companions (`TreeSet`, `TreeMap`, `SortedSet`, `SortedMap`,
//! `ArraySeq`) are deliberately absent: their evidence is
//! `EvidenceIterableFactory.toFactory(factory)(implicit ev: Ev[A])` /
//! `SortedMapFactory.toFactory`, which take a further implicit argument.

use crate::symbol::SymbolTable;
use scala_rs_parser::{SymbolId, Type};

/// `(companion module jvm name, collection class jvm name)`.
const ITERABLE_FACTORIES: &[(&str, &str)] = &[
    (
        "scala/collection/immutable/List$",
        "scala/collection/immutable/List",
    ),
    (
        "scala/collection/immutable/Vector$",
        "scala/collection/immutable/Vector",
    ),
    (
        "scala/collection/immutable/Seq$",
        "scala/collection/immutable/Seq",
    ),
    (
        "scala/collection/immutable/IndexedSeq$",
        "scala/collection/immutable/IndexedSeq",
    ),
    (
        "scala/collection/immutable/Set$",
        "scala/collection/immutable/Set",
    ),
    (
        "scala/collection/immutable/LazyList$",
        "scala/collection/immutable/LazyList",
    ),
    (
        "scala/collection/immutable/Queue$",
        "scala/collection/immutable/Queue",
    ),
    ("scala/collection/Iterable$", "scala/collection/Iterable"),
    ("scala/collection/Seq$", "scala/collection/Seq"),
    ("scala/collection/Set$", "scala/collection/Set"),
    (
        "scala/collection/mutable/ArrayBuffer$",
        "scala/collection/mutable/ArrayBuffer",
    ),
    (
        "scala/collection/mutable/ListBuffer$",
        "scala/collection/mutable/ListBuffer",
    ),
    (
        "scala/collection/mutable/Set$",
        "scala/collection/mutable/Set",
    ),
    (
        "scala/collection/mutable/HashSet$",
        "scala/collection/mutable/HashSet",
    ),
    (
        "scala/collection/mutable/LinkedHashSet$",
        "scala/collection/mutable/LinkedHashSet",
    ),
    (
        "scala/collection/mutable/ArrayDeque$",
        "scala/collection/mutable/ArrayDeque",
    ),
    (
        "scala/collection/mutable/Queue$",
        "scala/collection/mutable/Queue",
    ),
    (
        "scala/collection/mutable/Stack$",
        "scala/collection/mutable/Stack",
    ),
];

const MAP_FACTORIES: &[(&str, &str)] = &[
    (
        "scala/collection/immutable/Map$",
        "scala/collection/immutable/Map",
    ),
    (
        "scala/collection/immutable/HashMap$",
        "scala/collection/immutable/HashMap",
    ),
    (
        "scala/collection/mutable/Map$",
        "scala/collection/mutable/Map",
    ),
    (
        "scala/collection/mutable/HashMap$",
        "scala/collection/mutable/HashMap",
    ),
    (
        "scala/collection/mutable/LinkedHashMap$",
        "scala/collection/mutable/LinkedHashMap",
    ),
];

/// The factory interfaces this links to; `Typer::link_collection_factories`
/// loads them from the jar first, since `install_prelude` runs before the
/// classpath is installed.
pub(crate) const FACTORY_CLASSES: &[&str] = &[
    "scala/collection/IterableFactory",
    "scala/collection/MapFactory",
    "scala/collection/IterableFactory$",
    "scala/collection/MapFactory$",
    "scala/collection/Factory",
];

pub(crate) fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    for (factory, table, map_like) in [
        (FACTORY_CLASSES[0], ITERABLE_FACTORIES, false),
        (FACTORY_CLASSES[1], MAP_FACTORIES, true),
    ] {
        let Some(fac) = crate::classpath::find_by_jvm(st, factory) else {
            continue;
        };
        // The edge below is a *conformance* edge: it exists so
        // `IterableFactory.toFactory` can take a companion. The companions
        // declare their own `apply` / `empty` / `from` (prelude.rs,
        // prelude_coll.rs, prelude_mutcoll.rs) with the concrete result type,
        // and the class file's versions return the trait's own abstract `CC`
        // -- inheriting those turned `mutable.ArrayBuffer[Int]()` into an
        // `ArrayBuffer[A]`. Nothing in the modeled subset calls a member
        // *through* `IterableFactory` itself, so the trait carries none.
        st.get_mut(fac).members.clear();
        for (module_jvm, class_jvm) in table {
            link(st, fac, module_jvm, class_jvm, map_like);
        }
    }
    add_to_factory(st);
    widen_set_concat(st);
    add_mutable_map_removed(st);
}

/// `SetOps.concat(that: IterableOnce[A]): C` — the prelude declared `++` as
/// taking another `Set[A]`, so `Set(1, 2, 3) ++ List(9)` was
/// "no matching overload". `javap -p -s scala.collection.SetOps`:
/// `public default C concat(IterableOnce<A>)`, descriptor
/// `(Lscala/collection/IterableOnce;)Lscala/collection/SetOps;` — and the
/// codegen for it (`is_stdlib_set` in gen.rs) already emits
/// `IterableOps.++:(Lscala/collection/IterableOnce;)Ljava/lang/Object;`.
fn widen_set_concat(st: &mut SymbolTable) {
    let (Some(set), Some(ioc)) = (
        crate::classpath::find_by_jvm(st, "scala/collection/immutable/Set"),
        crate::classpath::find_by_jvm(st, "scala/collection/IterableOnce"),
    ) else {
        return;
    };
    let Some(&a) = st.get(set).tparams.first() else {
        return;
    };
    let set_a = Type::Class {
        sym: set,
        args: vec![Type::TypeParam(a)],
    };
    let ioc_a = Type::Class {
        sym: ioc,
        args: vec![Type::TypeParam(a)],
    };
    let wanted = Type::Method {
        paramss: vec![vec![set_a.clone()]],
        ret: Box::new(set_a.clone()),
    };
    let targets: Vec<SymbolId> = st
        .get(set)
        .members
        .iter()
        .copied()
        .filter(|&m| st.get(m).name == "++" && st.get(m).ty == wanted)
        .collect();
    for m in targets {
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![ioc_a.clone()]],
            ret: Box::new(set_a.clone()),
        };
        if let Some(&p) = st.get(m).params.first() {
            st.get_mut(p).ty = ioc_a.clone();
        }
    }
}

/// `mutable.MapOps.-(key: K): C`. `javap -p -s
/// scala.collection.mutable.MapOps`: `public default C $minus(K)`, descriptor
/// `(Ljava/lang/Object;)Lscala/collection/mutable/MapOps;`. The prelude
/// declares `mutable.Map` itself and had no `-`, so `m - k` was
/// "value - is not a member".
fn add_mutable_map_removed(st: &mut SymbolTable) {
    let Some(map) = crate::classpath::find_by_jvm(st, "scala/collection/mutable/Map") else {
        return;
    };
    if st.get(map).tparams.len() != 2 {
        return;
    }
    if st.get(map).members.iter().any(|&m| st.get(m).name == "-") {
        return;
    }
    let (k, v) = (st.get(map).tparams[0], st.get(map).tparams[1]);
    let map_t = Type::Class {
        sym: map,
        args: vec![Type::TypeParam(k), Type::TypeParam(v)],
    };
    crate::prelude::method(
        st,
        map,
        "-",
        vec![Type::TypeParam(k)],
        map_t,
        crate::symbol::Intrinsic::None,
    );
}

/// `object IterableFactory { implicit def toFactory[A, CC[_]](factory:
/// IterableFactory[CC]): Factory[A, CC[A]] }` and the `MapFactory` twin.
///
/// `PickleSupply::supply_implicit_members` deliberately skips `scala/`
/// classes -- the prelude is what declares those -- so these two were nowhere,
/// and no `to(…)` could reach them.
///
/// `javap -p -s scala.collection.IterableFactory$`:
/// `toFactory(Lscala/collection/IterableFactory;)Lscala/collection/Factory;`,
/// and `MapFactory$.toFactory(Lscala/collection/MapFactory;)Lscala/collection/Factory;`.
fn add_to_factory(st: &mut SymbolTable) {
    let Some(factory) = crate::classpath::find_by_jvm(st, "scala/collection/Factory") else {
        return;
    };
    let tuple2 = crate::classpath::find_by_jvm(st, "scala/Tuple2");
    for (fac_jvm, mod_jvm, map_like) in [
        (
            "scala/collection/IterableFactory",
            "scala/collection/IterableFactory$",
            false,
        ),
        (
            "scala/collection/MapFactory",
            "scala/collection/MapFactory$",
            true,
        ),
    ] {
        let (Some(fac), Some(owner)) = (
            crate::classpath::find_by_jvm(st, fac_jvm),
            module_class_by_jvm(st, mod_jvm),
        ) else {
            continue;
        };
        // Drop what the classfile reader entered. Java generics cannot spell
        // `CC[A]`, so the class file's signature is `<A, CC> Factory<A, CC>`:
        // `xs.to(ArrayBuffer)` solved `C1 = ArrayBuffer` (the bare type
        // constructor) instead of `ArrayBuffer[Int]`. It is also not marked
        // `implicit` — that lives only in the Scala pickle, which
        // `PickleSupply::supply_implicit_members` skips for `scala/` classes.
        let keep: Vec<SymbolId> = st
            .get(owner)
            .members
            .iter()
            .copied()
            .filter(|&m| st.get(m).name != "toFactory")
            .collect();
        st.get_mut(owner).members = keep;
        if map_like && tuple2.is_none() {
            continue;
        }
        let m = crate::prelude::method(
            st,
            owner,
            "toFactory",
            vec![],
            Type::Unit,
            crate::symbol::Intrinsic::None,
        );
        st.get_mut(m).flags = st.get(m).flags.with(scala_rs_parser::Flags::IMPLICIT);
        // `CC[_]` / `CC[_, _]`: a type constructor of the factory's own arity.
        let cc = crate::prelude::type_param(st, m, "CC");
        let arity = if map_like { 2 } else { 1 };
        let cc_params: Vec<SymbolId> = (0..arity)
            .map(|i| crate::prelude::type_param(st, cc, &format!("X{i}")))
            .collect();
        st.get_mut(cc).tparams = cc_params;
        let elems: Vec<SymbolId> = if map_like {
            vec![
                crate::prelude::type_param(st, m, "K"),
                crate::prelude::type_param(st, m, "V"),
            ]
        } else {
            vec![crate::prelude::type_param(st, m, "A")]
        };
        let mut tparams = elems.clone();
        tparams.push(cc);
        st.get_mut(m).tparams = tparams;
        let elem_tys: Vec<Type> = elems.iter().map(|&e| Type::TypeParam(e)).collect();
        let built = Type::Applied {
            ctor: Box::new(Type::TypeParam(cc)),
            args: elem_tys.clone(),
        };
        let elem = if map_like {
            Type::Class {
                sym: tuple2.unwrap(),
                args: elem_tys,
            }
        } else {
            elem_tys[0].clone()
        };
        let param = Type::Class {
            sym: fac,
            args: vec![Type::TypeParam(cc)],
        };
        let p = st.alloc(
            "factory",
            m,
            crate::symbol::SymKind::Term,
            scala_rs_parser::Flags::PARAM,
            "",
        );
        st.get_mut(p).ty = param.clone();
        st.get_mut(m).params = vec![p];
        st.get_mut(m).paramss = vec![vec![p]];
        st.get_mut(m).ty = Type::Method {
            paramss: vec![vec![param]],
            ret: Box::new(Type::Class {
                sym: factory,
                args: vec![elem, built],
            }),
        };
        let mems = st.get(owner).members.clone();
        if let Some(module) = companion_module_of(st, owner) {
            st.get_mut(module).members = mems;
        }
    }
}

/// The `Module` symbol whose class this is, if the two are separate.
fn companion_module_of(st: &SymbolTable, module_cls: SymbolId) -> Option<SymbolId> {
    let owner = st.get(module_cls).owner;
    let name = st.get(module_cls).name.clone();
    st.get(owner)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).kind == crate::symbol::SymKind::Module && st.get(m).name == name)
}

/// `object X extends IterableFactory[X]` — the argument is the collection's
/// type *constructor*, which is what a `Type::Class` with no arguments is —
/// plus the `Factory` evidence the factory trait defines on it.
fn link(
    st: &mut SymbolTable,
    factory: SymbolId,
    module_jvm: &str,
    class_jvm: &str,
    map_like: bool,
) {
    let Some(module_cls) = module_class_by_jvm(st, module_jvm) else {
        return;
    };
    let Some(cls) = crate::classpath::find_by_jvm(st, class_jvm) else {
        return;
    };
    let parent = Type::Class {
        sym: factory,
        args: vec![Type::Class {
            sym: cls,
            args: Vec::new(),
        }],
    };
    if !st.get(module_cls).parents.contains(&parent) {
        st.get_mut(module_cls).parents.push(parent);
    }
    add_factory_evidence(st, module_cls, cls, map_like);
}

/// `trait IterableFactory[+CC[_]] { implicit def iterableFactory[A]: Factory[A,
/// CC[A]] }` (and `MapFactory`'s `mapFactory`), as the companion sees it.
///
/// This is what `implicitly[Factory[QR, Vector[QR]]]` finds — slick's
/// `buildColl[Vector](null, implicitly)`. The view `toFactory` above cannot
/// serve there: an implicit *value* search never applies a conversion.
///
/// `javap -p -s scala.collection.immutable.List$`:
/// `<A> Factory<A, List<A>> iterableFactory()`, descriptor
/// `()Lscala/collection/Factory;`, and
/// `scala.collection.immutable.Map$`: `<K, V> Factory<Tuple2<K, V>, Map<K, V>>
/// mapFactory()`.
fn add_factory_evidence(st: &mut SymbolTable, module_cls: SymbolId, cls: SymbolId, map_like: bool) {
    let Some(factory) = crate::classpath::find_by_jvm(st, "scala/collection/Factory") else {
        return;
    };
    let name = if map_like {
        "mapFactory"
    } else {
        "iterableFactory"
    };
    if st
        .get(module_cls)
        .members
        .iter()
        .any(|&m| st.get(m).name == name)
    {
        return;
    }
    let tuple2 = crate::classpath::find_by_jvm(st, "scala/Tuple2");
    if map_like && tuple2.is_none() {
        return;
    }
    let m = crate::prelude::method(
        st,
        module_cls,
        name,
        vec![],
        Type::Unit,
        crate::symbol::Intrinsic::None,
    );
    st.get_mut(m).flags = st.get(m).flags.with(scala_rs_parser::Flags::IMPLICIT);
    let elems: Vec<SymbolId> = if map_like {
        vec![
            crate::prelude::type_param(st, m, "K"),
            crate::prelude::type_param(st, m, "V"),
        ]
    } else {
        vec![crate::prelude::type_param(st, m, "A")]
    };
    st.get_mut(m).tparams = elems.clone();
    let elem_tys: Vec<Type> = elems.iter().map(|&e| Type::TypeParam(e)).collect();
    let elem = if map_like {
        Type::Class {
            sym: tuple2.unwrap(),
            args: elem_tys.clone(),
        }
    } else {
        elem_tys[0].clone()
    };
    st.get_mut(m).ty = Type::Method {
        paramss: Vec::new(),
        ret: Box::new(Type::Class {
            sym: factory,
            args: vec![
                elem,
                Type::Class {
                    sym: cls,
                    args: elem_tys,
                },
            ],
        }),
    };
    let mems = st.get(module_cls).members.clone();
    if let Some(module) = companion_module_of(st, module_cls) {
        st.get_mut(module).members = mems;
    }
}

/// The *module class* carrying a companion's members, found by its JVM name.
fn module_class_by_jvm(st: &SymbolTable, jvm: &str) -> Option<SymbolId> {
    let id = crate::classpath::find_by_jvm(st, jvm)?;
    Some(if st.get(id).kind == crate::symbol::SymKind::Module {
        st.module_class_of(id)
    } else {
        id
    })
}
