//! The `scala.collection.mutable` companions the prelude was missing:
//! `Queue`, `Stack`, `TreeMap`, `TreeSet`, `PriorityQueue` and `ArraySeq`.
//!
//! The *classes* were reachable already — the classpath loader enters them
//! from the jar and `supply_from_pickle` completes their members on demand,
//! so `Queue.empty[Int].enqueue(1)` compiled and ran. What did not work was
//! building one: every `scala.collection.mutable.X(…)` reported
//!
//! ```text
//! error: no matching overload for (Seq[Int])CC with arguments ()
//! ```
//!
//! These companions inherit their `apply` from `IterableFactory` /
//! `SortedIterableFactory` / `EvidenceIterableFactory`, and the typer reads
//! the *classfile* signature for it, where a repeated parameter has already
//! become `Seq[A]` and the result is the factory's own abstract `CC`. So no
//! argument list ever matched — not even the empty one — and the result type
//! named nothing constructible. Declaring the companions here is what the
//! prelude already does for `ArrayBuffer`, `ListBuffer`, `HashMap`,
//! `HashSet`, `LinkedHashMap`, `LinkedHashSet` and `ArrayDeque`; the matching
//! codegen is the `is_stdlib_*_module` dispatch in `crates/backend/src/gen.rs`.
//!
//! Only the factory members (and the few instance members whose classfile
//! signature has already lost its repeated parameter) are declared. The rest
//! of each class keeps coming from the jar, and in `--no-scala-library` mode
//! nothing is installed at all: the private runtime has no `Queue` or
//! `TreeMap` classfile to call, and the existing "not a member" diagnostic is
//! the honest answer there.
//!
//! Every signature below is `javap -p` on `scala-library-2.13.16.jar`.

use crate::prelude::{class, method, module, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub fn install(st: &mut SymbolTable, library_abi: bool) {
    if !library_abi {
        return;
    }
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let ordering = crate::classpath::find_by_jvm(st, "scala/math/Ordering");
    let class_tag = crate::classpath::find_by_jvm(st, "scala/reflect/ClassTag");
    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2" && st.get(*id).is_class_like());

    // `object Queue extends StrictOptimizedSeqFactory[Queue]`
    //   `public <A> Queue<A> empty();`
    //   `public Object apply(immutable.Seq);`
    add_factory(st, mutp, "Queue", None);
    // `object Stack extends StrictOptimizedSeqFactory[Stack]` — same shape.
    add_factory(st, mutp, "Stack", None);
    // `object ArraySeq extends StrictOptimizedClassTagSeqFactory[ArraySeq]`
    //   `public <T> ArraySeq<T> empty(ClassTag<T>);`
    //   `public Object apply(immutable.Seq, Object);`
    let arrayseq = add_factory(st, mutp, "ArraySeq", class_tag);
    // `object TreeSet extends SortedIterableFactory[TreeSet]`
    //   `public <A> TreeSet<A> empty(Ordering<A>);`
    //   `public Object apply(immutable.Seq, Object);`
    add_factory(st, mutp, "TreeSet", ordering);
    // `object PriorityQueue extends SortedIterableFactory[PriorityQueue]`
    let pqueue = add_factory(st, mutp, "PriorityQueue", ordering);
    // `object TreeMap extends SortedMapFactory[TreeMap]`
    //   `public <K, V> TreeMap<K, V> empty(Ordering<K>);`
    //   `public Object apply(immutable.Seq, Ordering);`
    add_map_factory(st, mutp, ordering, tuple2);

    add_array_seq_members(st, arrayseq);
    add_priority_queue_members(st, pqueue);
    add_array_deque_append(st);
    add_string_builder_companion(st, mutp);
    add_constructors(st, ordering);
}

/// `object StringBuilder { def newBuilder: StringBuilder }`.
///
/// The prelude declared the class but no companion, so
/// `mutable.StringBuilder.newBuilder` found nothing to select and codegen
/// fell through to its "unresolved select" path — a classfile that compiled
/// clean and threw `RuntimeException: select StringBuilder` when run.
/// `javap`: `public scala.collection.mutable.StringBuilder newBuilder();`
fn add_string_builder_companion(st: &mut SymbolTable, mutp: SymbolId) {
    let Some(cls) = crate::classpath::find_by_jvm(st, "scala/collection/mutable/StringBuilder")
    else {
        return;
    };
    if st
        .get(mutp)
        .members
        .iter()
        .any(|&m| st.get(m).name == "StringBuilder" && st.get(m).kind == SymKind::Module)
    {
        return;
    }
    let m = module(
        st,
        mutp,
        "StringBuilder",
        "scala/collection/mutable/StringBuilder$",
    );
    let mcls = st.module_class_of(m);
    method(
        st,
        mcls,
        "newBuilder",
        vec![],
        Type::Class {
            sym: cls,
            args: vec![],
        },
        Intrinsic::None,
    );
    let mems = st.get(mcls).members.clone();
    st.get_mut(m).members.extend(mems);
}

/// The constructors `new X[T]()` needs.
///
/// Without them `new mutable.Queue[Int]()` type-checked against the implicit
/// no-argument constructor every prelude class gets and then died with
/// `NoSuchMethodError: Queue.<init>()` — the library declares
/// `class Queue[A](initialSize: Int = …)` and `class TreeSet[A]()(implicit
/// ord: Ordering[A])`, neither of which has a `()V`. The sized ones are
/// completed by codegen (`has_default_sized_ctor` in gen.rs); the sorted ones
/// are an ordinary implicit clause, so declaring it is all they need.
fn add_constructors(st: &mut SymbolTable, ordering: Option<SymbolId>) {
    for name in ["ArrayDeque", "Queue", "Stack"] {
        let jvm = format!("scala/collection/mutable/{name}");
        let Some(cls) = crate::classpath::find_by_jvm(st, &jvm) else {
            continue;
        };
        if declares_ctor(st, cls) {
            continue;
        }
        let self_t = self_type(st, cls);
        method(st, cls, "<init>", vec![], self_t.clone(), Intrinsic::None);
        // `javap`: `public scala.collection.mutable.Queue(int);`
        method(st, cls, "<init>", vec![Type::Int], self_t, Intrinsic::None);
    }
    let Some(ord) = ordering else {
        return;
    };
    // `javap`: `public scala.collection.mutable.TreeSet(scala.math.Ordering<A>);`
    for name in ["TreeMap", "TreeSet", "PriorityQueue"] {
        let jvm = format!("scala/collection/mutable/{name}");
        let Some(cls) = crate::classpath::find_by_jvm(st, &jvm) else {
            continue;
        };
        if declares_ctor(st, cls) {
            continue;
        }
        let self_t = self_type(st, cls);
        let Some(&k) = st.get(cls).tparams.first() else {
            continue;
        };
        let ctor = method(st, cls, "<init>", vec![], self_t, Intrinsic::None);
        let (ps, tys) = evidence_clause(st, ctor, Some(ord), k, Vec::new(), Vec::new());
        st.get_mut(ctor).params = ps.concat();
        st.get_mut(ctor).paramss = ps;
        let ret = match &st.get(ctor).ty {
            Type::Method { ret, .. } => (**ret).clone(),
            _ => Type::Unit,
        };
        st.get_mut(ctor).ty = Type::Method {
            paramss: tys,
            ret: Box::new(ret),
        };
    }
}

fn declares_ctor(st: &SymbolTable, cls: SymbolId) -> bool {
    st.get(cls)
        .members
        .iter()
        .any(|&m| st.get(m).name == "<init>")
}

fn self_type(st: &SymbolTable, cls: SymbolId) -> Type {
    Type::Class {
        sym: cls,
        args: st
            .get(cls)
            .tparams
            .iter()
            .map(|&t| Type::TypeParam(t))
            .collect(),
    }
}

/// A one-type-parameter `scala.collection.mutable` collection and its
/// companion's `apply[A](elems: A*)` / `empty[A]`.
///
/// `evidence` is the class of the implicit the factory demands in its second
/// parameter list (`Ordering` for the sorted ones, `ClassTag` for
/// `ArraySeq`), or `None` for the plain `IterableFactory` shape. It is a real
/// parameter of the JVM method, so it has to be declared as one: an implicit
/// clause is only searched for when the *symbol's* `paramss` carries it, and
/// leaving it in the type alone let `TreeSet(1, 2)` compile to a call with
/// the evidence missing from the stack (`VerifyError`).
fn add_factory(
    st: &mut SymbolTable,
    mutp: SymbolId,
    name: &str,
    evidence: Option<SymbolId>,
) -> SymbolId {
    let jvm = format!("scala/collection/mutable/{name}");
    let cls = class(st, mutp, name, &jvm, &[Type::AnyRef]);
    let a = type_param(st, cls, "A");
    st.get_mut(cls).tparams = vec![a];
    let cls_t = Type::Class {
        sym: cls,
        args: vec![Type::TypeParam(a)],
    };

    let m = module(st, mutp, name, &format!("{jvm}$"));
    let mcls = st.module_class_of(m);

    let empty = method(st, mcls, "empty", vec![], cls_t.clone(), Intrinsic::None);
    let ea = type_param(st, empty, "A");
    st.get_mut(empty).tparams = vec![ea];
    let (eps, epss) = evidence_clause(st, empty, evidence, ea, Vec::new(), Vec::new());
    st.get_mut(empty).params = eps.concat();
    st.get_mut(empty).paramss = eps;
    st.get_mut(empty).ty = Type::Method {
        paramss: epss,
        ret: Box::new(Type::Class {
            sym: cls,
            args: vec![Type::TypeParam(ea)],
        }),
    };

    let apply = method(
        st,
        mcls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        cls_t,
        Intrinsic::None,
    );
    let aa = type_param(st, apply, "A");
    st.get_mut(apply).tparams = vec![aa];
    let elems = repeated_param(st, apply, Type::TypeParam(aa));
    let (aps, apss) = evidence_clause(
        st,
        apply,
        evidence,
        aa,
        vec![vec![elems]],
        vec![vec![Type::Repeated(Box::new(Type::TypeParam(aa)))]],
    );
    st.get_mut(apply).params = aps.concat();
    st.get_mut(apply).paramss = aps;
    st.get_mut(apply).ty = Type::Method {
        paramss: apss,
        ret: Box::new(Type::Class {
            sym: cls,
            args: vec![Type::TypeParam(aa)],
        }),
    };

    let mems = st.get(mcls).members.clone();
    st.get_mut(m).members.extend(mems);
    cls
}

/// `object TreeMap extends SortedMapFactory[TreeMap]`: two type parameters,
/// pairs for elements, and the `Ordering` on the *key*.
fn add_map_factory(
    st: &mut SymbolTable,
    mutp: SymbolId,
    ordering: Option<SymbolId>,
    tuple2: Option<SymbolId>,
) {
    let cls = class(
        st,
        mutp,
        "TreeMap",
        "scala/collection/mutable/TreeMap",
        &[Type::AnyRef],
    );
    let k = type_param(st, cls, "K");
    let v = type_param(st, cls, "V");
    st.get_mut(cls).tparams = vec![k, v];
    let cls_t = Type::Class {
        sym: cls,
        args: vec![Type::TypeParam(k), Type::TypeParam(v)],
    };

    let m = module(st, mutp, "TreeMap", "scala/collection/mutable/TreeMap$");
    let mcls = st.module_class_of(m);

    let empty = method(st, mcls, "empty", vec![], cls_t.clone(), Intrinsic::None);
    let ek = type_param(st, empty, "K");
    let ev = type_param(st, empty, "V");
    st.get_mut(empty).tparams = vec![ek, ev];
    let (eps, epss) = evidence_clause(st, empty, ordering, ek, Vec::new(), Vec::new());
    st.get_mut(empty).params = eps.concat();
    st.get_mut(empty).paramss = eps;
    st.get_mut(empty).ty = Type::Method {
        paramss: epss,
        ret: Box::new(Type::Class {
            sym: cls,
            args: vec![Type::TypeParam(ek), Type::TypeParam(ev)],
        }),
    };

    let apply = method(
        st,
        mcls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        cls_t,
        Intrinsic::None,
    );
    let ak = type_param(st, apply, "K");
    let av = type_param(st, apply, "V");
    st.get_mut(apply).tparams = vec![ak, av];
    let pair = match tuple2 {
        Some(t2) => Type::Class {
            sym: t2,
            args: vec![Type::TypeParam(ak), Type::TypeParam(av)],
        },
        None => Type::Tuple(vec![Type::TypeParam(ak), Type::TypeParam(av)]),
    };
    let elems = repeated_param(st, apply, pair.clone());
    let (aps, apss) = evidence_clause(
        st,
        apply,
        ordering,
        ak,
        vec![vec![elems]],
        vec![vec![Type::Repeated(Box::new(pair))]],
    );
    st.get_mut(apply).params = aps.concat();
    st.get_mut(apply).paramss = aps;
    st.get_mut(apply).ty = Type::Method {
        paramss: apss,
        ret: Box::new(Type::Class {
            sym: cls,
            args: vec![Type::TypeParam(ak), Type::TypeParam(av)],
        }),
    };

    let mems = st.get(mcls).members.clone();
    st.get_mut(m).members.extend(mems);
}

fn repeated_param(st: &mut SymbolTable, owner: SymbolId, elem: Type) -> SymbolId {
    let id = st.alloc("elems", owner, SymKind::Term, Flags::PARAM, "");
    st.get_mut(id).ty = Type::Repeated(Box::new(elem));
    id
}

/// Append the factory's implicit evidence clause to both the symbol's
/// parameter lists and its method type, and hand both back.
fn evidence_clause(
    st: &mut SymbolTable,
    owner: SymbolId,
    evidence: Option<SymbolId>,
    tp: SymbolId,
    mut paramss: Vec<Vec<SymbolId>>,
    mut tyss: Vec<Vec<Type>>,
) -> (Vec<Vec<SymbolId>>, Vec<Vec<Type>>) {
    let Some(ev_cls) = evidence else {
        if paramss.is_empty() {
            paramss.push(Vec::new());
            tyss.push(Vec::new());
        }
        return (paramss, tyss);
    };
    let ev_ty = Type::Class {
        sym: ev_cls,
        args: vec![Type::TypeParam(tp)],
    };
    let id = st.alloc(
        "evidence$1",
        owner,
        SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(id).ty = ev_ty.clone();
    // `def empty[A: Ordering]: TreeSet[A]` is *parameterless* with one
    // implicit clause, not a nullary method followed by one. Written the
    // second way, `TreeSet.empty[Int]` kept the implicit clause unapplied and
    // every later selection reported "not a member of
    // `()(Ordering[Int])TreeSet[Int]`".
    paramss.push(vec![id]);
    tyss.push(vec![ev_ty]);
    (paramss, tyss)
}

/// `ArraySeq.update` is the one member `a(0) = 9` needs, and it is *abstract*
/// on `ArraySeq` itself, so nothing completed it onto the class.
/// `javap`: `public abstract void update(int, T);`
fn add_array_seq_members(st: &mut SymbolTable, cls: SymbolId) {
    let Some(&a) = st.get(cls).tparams.first() else {
        return;
    };
    let ta = Type::TypeParam(a);
    method(
        st,
        cls,
        "update",
        vec![Type::Int, ta.clone()],
        Type::Unit,
        Intrinsic::None,
    );
    // `apply` and `length` are abstract on `scala.collection.SeqOps`, which
    // `ArraySeq` does not redeclare; nothing completed them onto the class,
    // so `s(0)` and `s.length` were "not a member". `invokevirtual` on the
    // class resolves the maximally-specific superinterface method.
    method(
        st,
        cls,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(st, cls, "length", vec![], Type::Int, Intrinsic::None);
    method(st, cls, "size", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        cls,
        "toList",
        vec![],
        Type::Class {
            sym: st.list_sym,
            args: vec![ta],
        },
        Intrinsic::None,
    );
}

/// `PriorityQueue.enqueue` is `def enqueue(elems: A*): Unit`, and the
/// classfile signature the typer would otherwise read has already turned the
/// repeated parameter into `immutable.Seq[A]`, so `p.enqueue(1)` reported
/// `no matching overload for (Seq[Int])Unit with arguments (1)`.
/// `javap`: `public void enqueue(scala.collection.immutable.Seq<A>);`
fn add_priority_queue_members(st: &mut SymbolTable, cls: SymbolId) {
    let Some(&a) = st.get(cls).tparams.first() else {
        return;
    };
    let enq = method(
        st,
        cls,
        "enqueue",
        vec![Type::Repeated(Box::new(Type::TypeParam(a)))],
        Type::Unit,
        Intrinsic::None,
    );
    let elems = repeated_param(st, enq, Type::TypeParam(a));
    st.get_mut(enq).params = vec![elems];
    st.get_mut(enq).paramss = vec![vec![elems]];
}

/// `Buffer.append(elem: A): this.type` is a default method on
/// `scala.collection.mutable.Buffer`, which `ArrayDeque` (and so `Queue` and
/// `Stack`) inherits without overriding; the prelude's `ArrayDeque` declared
/// `prepend` but not its counterpart.
/// `javap`: `public default scala.collection.mutable.Buffer<A> append(A);`
fn add_array_deque_append(st: &mut SymbolTable) {
    let Some(cls) = crate::classpath::find_by_jvm(st, "scala/collection/mutable/ArrayDeque") else {
        return;
    };
    if !st.lookup_member(cls, "append").is_empty() {
        return;
    }
    let Some(&a) = st.get(cls).tparams.first() else {
        return;
    };
    let self_t = Type::Class {
        sym: cls,
        args: vec![Type::TypeParam(a)],
    };
    method(
        st,
        cls,
        "append",
        vec![Type::TypeParam(a)],
        self_t,
        Intrinsic::None,
    );
}
