use crate::prelude::{class, fn1, iface, method, module, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn add_ordered(st: &mut SymbolTable) -> SymbolId {
    let math = st.alloc(
        "math",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/math",
    );
    let ordered = iface(st, math, "Ordered", "scala/math/Ordered");
    let a = type_param(st, ordered, "A");
    st.get_mut(ordered).tparams = vec![a];
    let cmp = st.alloc("compare", ordered, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(cmp).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(a)]],
        ret: Box::new(Type::Int),
    };
    for op in ["<", ">", "<=", ">="] {
        let id = st.alloc(op, ordered, SymKind::Method, Flags::EMPTY, "");
        st.get_mut(id).ty = Type::Method {
            paramss: vec![vec![Type::TypeParam(a)]],
            ret: Box::new(Type::Boolean),
        };
    }
    ordered
}
/// `scala.math.Ordering` + companion `implicit object Int` (`Ordering$Int$.MODULE$`).
pub(crate) fn add_ordering(st: &mut SymbolTable) -> SymbolId {
    let math = crate::classpath::ensure_package(st, "scala/math");
    let ordering = iface(st, math, "Ordering", "scala/math/Ordering");
    let t = type_param(st, ordering, "T");
    st.get_mut(ordering).tparams = vec![t];
    // nsc: `def compare(x: T, y: T): Int`. This was `(Any, Any): Int` --
    // `Ordering[String].compare(1, 2)` type-checked here but real scalac
    // rejects it (`found: Int(1) required: String`). `Type::TypeParam(t)`
    // erases to `Ljava/lang/Object;` exactly like `Type::Any` did (see
    // `jvm_desc` in `crates/backend/src/gen.rs`), so the erased descriptor
    // `sorted` / `sortBy` codegen expects, `(Ljava/lang/Object;Ljava/lang/
    // Object;)I`, is unchanged -- only the *typed* view becomes generic.
    method(
        st,
        ordering,
        "compare",
        vec![Type::TypeParam(t), Type::TypeParam(t)],
        Type::Int,
        Intrinsic::None,
    );
    let ord_mod = module(st, math, "Ordering", "scala/math/Ordering$");
    let ord_cls = st.module_class_of(ord_mod);
    add_ordering_instance(
        st,
        ord_cls,
        ordering,
        "Int",
        "scala/math/Ordering$Int$",
        Type::Int,
    );
    add_ordering_instance(
        st,
        ord_cls,
        ordering,
        "Char",
        "scala/math/Ordering$Char$",
        Type::Char,
    );
    let mems = st.get(ord_cls).members.clone();
    st.get_mut(ord_mod).members.extend(mems);
    ordering
}
pub(crate) fn add_ordering_instance(
    st: &mut SymbolTable,
    ord_cls: SymbolId,
    ordering: SymbolId,
    name: &str,
    jvm: &str,
    arg: Type,
) {
    let m = module(st, ord_cls, name, jvm);
    st.get_mut(m).flags = st.get(m).flags.with(Flags::IMPLICIT);
    st.get_mut(m).ty = Type::Class {
        sym: ordering,
        args: vec![arg.clone()],
    };
    let cls = st.module_class_of(m);
    st.get_mut(cls).parents = vec![Type::Class {
        sym: ordering,
        args: vec![arg],
    }];
}
fn add_sorted_factory(st: &mut SymbolTable, owner: SymbolId, cls: SymbolId, ordering: SymbolId) {
    let cls_t = Type::Class {
        sym: cls,
        args: vec![Type::Any],
    };
    let apply = method(
        st,
        owner,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        cls_t,
        Intrinsic::None,
    );
    let aa = type_param(st, apply, "A");
    let xs = st.alloc(
        "elems",
        apply,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(xs).ty = Type::Repeated(Box::new(Type::TypeParam(aa)));
    let ev = st.alloc(
        "evidence$1",
        apply,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ordering,
        args: vec![Type::TypeParam(aa)],
    };
    st.get_mut(apply).tparams = vec![aa];
    st.get_mut(apply).params = vec![xs, ev];
    st.get_mut(apply).paramss = vec![vec![xs], vec![ev]];
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![
            vec![Type::Repeated(Box::new(Type::TypeParam(aa)))],
            vec![Type::Class {
                sym: ordering,
                args: vec![Type::TypeParam(aa)],
            }],
        ],
        ret: Box::new(Type::Class {
            sym: cls,
            args: vec![Type::TypeParam(aa)],
        }),
    };
}
pub(crate) fn add_sorted_set(st: &mut SymbolTable, ordering: SymbolId) {
    let immp = crate::classpath::ensure_package(st, "scala/collection/immutable");
    let ss = iface(
        st,
        immp,
        "SortedSet",
        "scala/collection/immutable/SortedSet",
    );
    let sa = type_param(st, ss, "A");
    st.get_mut(ss).tparams = vec![sa];
    let ta = Type::TypeParam(sa);
    method(
        st,
        ss,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        ss,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let ss_mod = module(
        st,
        immp,
        "SortedSet",
        "scala/collection/immutable/SortedSet$",
    );
    let ss_cls = st.module_class_of(ss_mod);
    add_sorted_factory(st, ss_cls, ss, ordering);
    let mems = st.get(ss_cls).members.clone();
    st.get_mut(ss_mod).members.extend(mems);

    let ts = class(
        st,
        immp,
        "TreeSet",
        "scala/collection/immutable/TreeSet",
        &[Type::Class {
            sym: ss,
            args: vec![],
        }],
    );
    let tsa = type_param(st, ts, "A");
    st.get_mut(ts).tparams = vec![tsa];
    let tta = Type::TypeParam(tsa);
    st.get_mut(ts).parents = vec![Type::Class {
        sym: ss,
        args: vec![tta.clone()],
    }];
    method(
        st,
        ts,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        ts,
        "foreach",
        vec![fn1(tta, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let ts_mod = module(st, immp, "TreeSet", "scala/collection/immutable/TreeSet$");
    let ts_cls = st.module_class_of(ts_mod);
    add_sorted_factory(st, ts_cls, ts, ordering);
    let tmems = st.get(ts_cls).members.clone();
    st.get_mut(ts_mod).members.extend(tmems);
}
fn add_sorted_map_factory(
    st: &mut SymbolTable,
    owner: SymbolId,
    cls: SymbolId,
    ordering: SymbolId,
    tuple2: SymbolId,
) {
    let apply = method(
        st,
        owner,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        Type::Class {
            sym: cls,
            args: vec![Type::Any, Type::Any],
        },
        Intrinsic::None,
    );
    let k = type_param(st, apply, "K");
    let v = type_param(st, apply, "V");
    let pair = Type::Class {
        sym: tuple2,
        args: vec![Type::TypeParam(k), Type::TypeParam(v)],
    };
    let xs = st.alloc(
        "elems",
        apply,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(xs).ty = Type::Repeated(Box::new(pair.clone()));
    let ev = st.alloc(
        "evidence$1",
        apply,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: ordering,
        args: vec![Type::TypeParam(k)],
    };
    st.get_mut(apply).tparams = vec![k, v];
    st.get_mut(apply).params = vec![xs, ev];
    st.get_mut(apply).paramss = vec![vec![xs], vec![ev]];
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![
            vec![Type::Repeated(Box::new(pair))],
            vec![Type::Class {
                sym: ordering,
                args: vec![Type::TypeParam(k)],
            }],
        ],
        ret: Box::new(Type::Class {
            sym: cls,
            args: vec![Type::TypeParam(k), Type::TypeParam(v)],
        }),
    };
}
pub(crate) fn add_sorted_map(st: &mut SymbolTable, ordering: SymbolId) {
    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2")
        .unwrap_or(SymbolId::NONE);
    let immp = crate::classpath::ensure_package(st, "scala/collection/immutable");
    let sm = iface(
        st,
        immp,
        "SortedMap",
        "scala/collection/immutable/SortedMap",
    );
    let sk = type_param(st, sm, "K");
    let sv = type_param(st, sm, "V");
    st.get_mut(sm).tparams = vec![sk, sv];
    let tk = Type::TypeParam(sk);
    let tv = Type::TypeParam(sv);
    let pair = Type::Class {
        sym: tuple2,
        args: vec![tk.clone(), tv.clone()],
    };
    method(
        st,
        sm,
        "apply",
        vec![Type::Any],
        tv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        sm,
        "get",
        vec![Type::Any],
        Type::Class {
            sym: st.option_sym,
            args: vec![tv.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        sm,
        "foreach",
        vec![fn1(pair.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let sm_mod = module(
        st,
        immp,
        "SortedMap",
        "scala/collection/immutable/SortedMap$",
    );
    let sm_cls = st.module_class_of(sm_mod);
    add_sorted_map_factory(st, sm_cls, sm, ordering, tuple2);
    let mems = st.get(sm_cls).members.clone();
    st.get_mut(sm_mod).members.extend(mems);

    let tm = class(
        st,
        immp,
        "TreeMap",
        "scala/collection/immutable/TreeMap",
        &[Type::Class {
            sym: sm,
            args: vec![],
        }],
    );
    let tmk = type_param(st, tm, "K");
    let tmv = type_param(st, tm, "V");
    st.get_mut(tm).tparams = vec![tmk, tmv];
    let ttk = Type::TypeParam(tmk);
    let ttv = Type::TypeParam(tmv);
    st.get_mut(tm).parents = vec![Type::Class {
        sym: sm,
        args: vec![ttk.clone(), ttv.clone()],
    }];
    let tpair = Type::Class {
        sym: tuple2,
        args: vec![ttk.clone(), ttv.clone()],
    };
    method(
        st,
        tm,
        "apply",
        vec![Type::Any],
        ttv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        tm,
        "get",
        vec![Type::Any],
        Type::Class {
            sym: st.option_sym,
            args: vec![ttv],
        },
        Intrinsic::None,
    );
    method(
        st,
        tm,
        "foreach",
        vec![fn1(tpair, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let tm_mod = module(st, immp, "TreeMap", "scala/collection/immutable/TreeMap$");
    let tm_cls = st.module_class_of(tm_mod);
    add_sorted_map_factory(st, tm_cls, tm, ordering, tuple2);
    let tmems = st.get(tm_cls).members.clone();
    st.get_mut(tm_mod).members.extend(tmems);
}
