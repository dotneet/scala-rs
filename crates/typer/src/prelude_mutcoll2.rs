use crate::prelude::{class, fn1, method, module, type_param};
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::{SymbolId, Type};

pub(crate) fn add_array_buffer(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let buf = class(
        st,
        mutp,
        "ArrayBuffer",
        "scala/collection/mutable/ArrayBuffer",
        &[Type::AnyRef],
    );
    let ba = type_param(st, buf, "A");
    st.get_mut(buf).tparams = vec![ba];
    let ta = Type::TypeParam(ba);
    let buf_t = Type::Class {
        sym: buf,
        args: vec![ta.clone()],
    };
    method(
        st,
        buf,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "update",
        vec![Type::Int, Type::Any],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "+=",
        vec![Type::Any],
        buf_t.clone(),
        Intrinsic::None,
    );
    let buf_mod = module(
        st,
        mutp,
        "ArrayBuffer",
        "scala/collection/mutable/ArrayBuffer$",
    );
    let buf_cls = st.module_class_of(buf_mod);
    method(
        st,
        buf_cls,
        "empty",
        vec![],
        Type::Class {
            sym: buf,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let buf_apply = method(
        st,
        buf_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        buf_t.clone(),
        Intrinsic::None,
    );
    let baa = type_param(st, buf_apply, "A");
    st.get_mut(buf_apply).tparams = vec![baa];
    st.get_mut(buf_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(baa)))]],
        ret: Box::new(Type::Class {
            sym: buf,
            args: vec![Type::TypeParam(baa)],
        }),
    };
    let mems = st.get(buf_cls).members.clone();
    st.get_mut(buf_mod).members.extend(mems);
}
pub(crate) fn add_list_buffer(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let buf = class(
        st,
        mutp,
        "ListBuffer",
        "scala/collection/mutable/ListBuffer",
        &[Type::AnyRef],
    );
    let ba = type_param(st, buf, "A");
    st.get_mut(buf).tparams = vec![ba];
    let ta = Type::TypeParam(ba);
    let buf_t = Type::Class {
        sym: buf,
        args: vec![ta.clone()],
    };
    method(
        st,
        buf,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(
        st,
        buf,
        "+=",
        vec![Type::Any],
        buf_t.clone(),
        Intrinsic::None,
    );
    let buf_mod = module(
        st,
        mutp,
        "ListBuffer",
        "scala/collection/mutable/ListBuffer$",
    );
    let buf_cls = st.module_class_of(buf_mod);
    method(
        st,
        buf_cls,
        "empty",
        vec![],
        Type::Class {
            sym: buf,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let buf_apply = method(
        st,
        buf_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        buf_t.clone(),
        Intrinsic::None,
    );
    let baa = type_param(st, buf_apply, "A");
    st.get_mut(buf_apply).tparams = vec![baa];
    st.get_mut(buf_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(baa)))]],
        ret: Box::new(Type::Class {
            sym: buf,
            args: vec![Type::TypeParam(baa)],
        }),
    };
    let mems = st.get(buf_cls).members.clone();
    st.get_mut(buf_mod).members.extend(mems);
}
pub(crate) fn add_array_deque(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let deq = class(
        st,
        mutp,
        "ArrayDeque",
        "scala/collection/mutable/ArrayDeque",
        &[Type::AnyRef],
    );
    let da = type_param(st, deq, "A");
    st.get_mut(deq).tparams = vec![da];
    let ta = Type::TypeParam(da);
    let deq_t = Type::Class {
        sym: deq,
        args: vec![ta.clone()],
    };
    method(
        st,
        deq,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(
        st,
        deq,
        "+=",
        vec![Type::Any],
        deq_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        deq,
        "prepend",
        vec![Type::Any],
        deq_t.clone(),
        Intrinsic::None,
    );
    let deq_mod = module(
        st,
        mutp,
        "ArrayDeque",
        "scala/collection/mutable/ArrayDeque$",
    );
    let deq_cls = st.module_class_of(deq_mod);
    let deq_empty = method(
        st,
        deq_cls,
        "empty",
        vec![],
        Type::Class {
            sym: deq,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let ea = type_param(st, deq_empty, "A");
    st.get_mut(deq_empty).tparams = vec![ea];
    st.get_mut(deq_empty).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Class {
            sym: deq,
            args: vec![Type::TypeParam(ea)],
        }),
    };
    let deq_apply = method(
        st,
        deq_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        deq_t.clone(),
        Intrinsic::None,
    );
    let daa = type_param(st, deq_apply, "A");
    st.get_mut(deq_apply).tparams = vec![daa];
    st.get_mut(deq_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(daa)))]],
        ret: Box::new(Type::Class {
            sym: deq,
            args: vec![Type::TypeParam(daa)],
        }),
    };
    let mems = st.get(deq_cls).members.clone();
    st.get_mut(deq_mod).members.extend(mems);
}
pub(crate) fn add_hash_map(st: &mut SymbolTable) {
    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2")
        .unwrap_or(SymbolId::NONE);
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let hm = class(
        st,
        mutp,
        "HashMap",
        "scala/collection/mutable/HashMap",
        &[Type::AnyRef],
    );
    let mk = type_param(st, hm, "K");
    let mv = type_param(st, hm, "V");
    st.get_mut(hm).tparams = vec![mk, mv];
    let tk = Type::TypeParam(mk);
    let tv = Type::TypeParam(mv);
    let hm_t = Type::Class {
        sym: hm,
        args: vec![tk.clone(), tv.clone()],
    };
    let pair = Type::Class {
        sym: tuple2,
        args: vec![tk, tv.clone()],
    };
    method(
        st,
        hm,
        "apply",
        vec![Type::Any],
        tv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        hm,
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
        hm,
        "update",
        vec![Type::Any, Type::Any],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, hm, "+=", vec![Type::Any], hm_t.clone(), Intrinsic::None);
    let hm_mod = module(st, mutp, "HashMap", "scala/collection/mutable/HashMap$");
    let hm_cls = st.module_class_of(hm_mod);
    let hm_empty = method(
        st,
        hm_cls,
        "empty",
        vec![],
        Type::Class {
            sym: hm,
            args: vec![Type::Any, Type::Any],
        },
        Intrinsic::None,
    );
    let ek = type_param(st, hm_empty, "K");
    let ev = type_param(st, hm_empty, "V");
    st.get_mut(hm_empty).tparams = vec![ek, ev];
    st.get_mut(hm_empty).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Class {
            sym: hm,
            args: vec![Type::TypeParam(ek), Type::TypeParam(ev)],
        }),
    };
    let hm_apply = method(
        st,
        hm_cls,
        "apply",
        vec![Type::Repeated(Box::new(pair.clone()))],
        hm_t.clone(),
        Intrinsic::None,
    );
    let hak = type_param(st, hm_apply, "K");
    let hav = type_param(st, hm_apply, "V");
    st.get_mut(hm_apply).tparams = vec![hak, hav];
    let hm_pair = Type::Class {
        sym: tuple2,
        args: vec![Type::TypeParam(hak), Type::TypeParam(hav)],
    };
    st.get_mut(hm_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(hm_pair))]],
        ret: Box::new(Type::Class {
            sym: hm,
            args: vec![Type::TypeParam(hak), Type::TypeParam(hav)],
        }),
    };
    let mems = st.get(hm_cls).members.clone();
    st.get_mut(hm_mod).members.extend(mems);
}
pub(crate) fn add_hash_set(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let hs = class(
        st,
        mutp,
        "HashSet",
        "scala/collection/mutable/HashSet",
        &[Type::AnyRef],
    );
    let sa = type_param(st, hs, "A");
    st.get_mut(hs).tparams = vec![sa];
    let ta = Type::TypeParam(sa);
    let hs_t = Type::Class {
        sym: hs,
        args: vec![ta.clone()],
    };
    method(
        st,
        hs,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, hs, "+=", vec![Type::Any], hs_t.clone(), Intrinsic::None);
    let hs_mod = module(st, mutp, "HashSet", "scala/collection/mutable/HashSet$");
    let hs_cls = st.module_class_of(hs_mod);
    let hs_empty = method(
        st,
        hs_cls,
        "empty",
        vec![],
        Type::Class {
            sym: hs,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let ea = type_param(st, hs_empty, "A");
    st.get_mut(hs_empty).tparams = vec![ea];
    st.get_mut(hs_empty).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Class {
            sym: hs,
            args: vec![Type::TypeParam(ea)],
        }),
    };
    let hs_apply = method(
        st,
        hs_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        hs_t.clone(),
        Intrinsic::None,
    );
    let haa = type_param(st, hs_apply, "A");
    st.get_mut(hs_apply).tparams = vec![haa];
    st.get_mut(hs_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(haa)))]],
        ret: Box::new(Type::Class {
            sym: hs,
            args: vec![Type::TypeParam(haa)],
        }),
    };
    let mems = st.get(hs_cls).members.clone();
    st.get_mut(hs_mod).members.extend(mems);
}
pub(crate) fn add_linked_hash_map(st: &mut SymbolTable) {
    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2")
        .unwrap_or(SymbolId::NONE);
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let lhm = class(
        st,
        mutp,
        "LinkedHashMap",
        "scala/collection/mutable/LinkedHashMap",
        &[Type::AnyRef],
    );
    let mk = type_param(st, lhm, "K");
    let mv = type_param(st, lhm, "V");
    st.get_mut(lhm).tparams = vec![mk, mv];
    let tk = Type::TypeParam(mk);
    let tv = Type::TypeParam(mv);
    let lhm_t = Type::Class {
        sym: lhm,
        args: vec![tk.clone(), tv.clone()],
    };
    let pair = Type::Class {
        sym: tuple2,
        args: vec![tk, tv.clone()],
    };
    method(
        st,
        lhm,
        "apply",
        vec![Type::Any],
        tv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        lhm,
        "update",
        vec![Type::Any, Type::Any],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        lhm,
        "+=",
        vec![Type::Any],
        lhm_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        lhm,
        "foreach",
        vec![fn1(pair.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let lhm_mod = module(
        st,
        mutp,
        "LinkedHashMap",
        "scala/collection/mutable/LinkedHashMap$",
    );
    let lhm_cls = st.module_class_of(lhm_mod);
    let lhm_empty = method(
        st,
        lhm_cls,
        "empty",
        vec![],
        Type::Class {
            sym: lhm,
            args: vec![Type::Any, Type::Any],
        },
        Intrinsic::None,
    );
    let ek = type_param(st, lhm_empty, "K");
    let ev = type_param(st, lhm_empty, "V");
    st.get_mut(lhm_empty).tparams = vec![ek, ev];
    st.get_mut(lhm_empty).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Class {
            sym: lhm,
            args: vec![Type::TypeParam(ek), Type::TypeParam(ev)],
        }),
    };
    let lhm_apply = method(
        st,
        lhm_cls,
        "apply",
        vec![Type::Repeated(Box::new(pair.clone()))],
        lhm_t.clone(),
        Intrinsic::None,
    );
    let lak = type_param(st, lhm_apply, "K");
    let lav = type_param(st, lhm_apply, "V");
    st.get_mut(lhm_apply).tparams = vec![lak, lav];
    let lhm_pair = Type::Class {
        sym: tuple2,
        args: vec![Type::TypeParam(lak), Type::TypeParam(lav)],
    };
    st.get_mut(lhm_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(lhm_pair))]],
        ret: Box::new(Type::Class {
            sym: lhm,
            args: vec![Type::TypeParam(lak), Type::TypeParam(lav)],
        }),
    };
    let mems = st.get(lhm_cls).members.clone();
    st.get_mut(lhm_mod).members.extend(mems);
}
pub(crate) fn add_linked_hash_set(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    let lhs = class(
        st,
        mutp,
        "LinkedHashSet",
        "scala/collection/mutable/LinkedHashSet",
        &[Type::AnyRef],
    );
    let sa = type_param(st, lhs, "A");
    st.get_mut(lhs).tparams = vec![sa];
    let ta = Type::TypeParam(sa);
    let lhs_t = Type::Class {
        sym: lhs,
        args: vec![ta.clone()],
    };
    method(
        st,
        lhs,
        "contains",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        lhs,
        "+=",
        vec![Type::Any],
        lhs_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        lhs,
        "foreach",
        vec![fn1(ta, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let lhs_mod = module(
        st,
        mutp,
        "LinkedHashSet",
        "scala/collection/mutable/LinkedHashSet$",
    );
    let lhs_cls = st.module_class_of(lhs_mod);
    let lhs_empty = method(
        st,
        lhs_cls,
        "empty",
        vec![],
        Type::Class {
            sym: lhs,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let ea = type_param(st, lhs_empty, "A");
    st.get_mut(lhs_empty).tparams = vec![ea];
    st.get_mut(lhs_empty).ty = Type::Method {
        paramss: vec![vec![]],
        ret: Box::new(Type::Class {
            sym: lhs,
            args: vec![Type::TypeParam(ea)],
        }),
    };
    let lhs_apply = method(
        st,
        lhs_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        lhs_t.clone(),
        Intrinsic::None,
    );
    let haa = type_param(st, lhs_apply, "A");
    st.get_mut(lhs_apply).tparams = vec![haa];
    st.get_mut(lhs_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(haa)))]],
        ret: Box::new(Type::Class {
            sym: lhs,
            args: vec![Type::TypeParam(haa)],
        }),
    };
    let mems = st.get(lhs_cls).members.clone();
    st.get_mut(lhs_mod).members.extend(mems);
}
/// Base `scala.collection.mutable.StringBuilder` class symbol. The full
/// member set (constructors, `append` overloads, `+=`, `insert`, `reverse`,
/// ...) is added by `prelude_text::add_string_builder_full`, which reuses
/// this same symbol (and aliases it under `scala` for the bare name) rather
/// than declaring a second, conflicting one.
pub(crate) fn add_string_builder(st: &mut SymbolTable) {
    let mutp = crate::classpath::ensure_package(st, "scala/collection/mutable");
    class(
        st,
        mutp,
        "StringBuilder",
        "scala/collection/mutable/StringBuilder",
        &[Type::AnyRef],
    );
}
