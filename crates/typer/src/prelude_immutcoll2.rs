use crate::prelude::{class, fn1, iface, method, module, type_param};
use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub(crate) fn add_bit_set(st: &mut SymbolTable) {
    let immp = crate::classpath::ensure_package(st, "scala/collection/immutable");
    let bs = class(
        st,
        immp,
        "BitSet",
        "scala/collection/immutable/BitSet",
        &[Type::AnyRef],
    );
    method(
        st,
        bs,
        "contains",
        vec![Type::Int],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        bs,
        "foreach",
        vec![fn1(Type::Int, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let bs_t = Type::Class {
        sym: bs,
        args: vec![],
    };
    let bs_mod = module(st, immp, "BitSet", "scala/collection/immutable/BitSet$");
    let bs_cls = st.module_class_of(bs_mod);
    method(
        st,
        bs_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Int))],
        bs_t,
        Intrinsic::None,
    );
    let mems = st.get(bs_cls).members.clone();
    st.get_mut(bs_mod).members.extend(mems);
}
pub(crate) fn add_option_members(st: &mut SymbolTable, option_wf: SymbolId, library_abi: bool) {
    let o = st.option_sym;
    let a = type_param(st, o, "A");
    st.get_mut(o).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let opt = Type::Class {
        sym: o,
        args: vec![ta.clone()],
    };
    method(st, o, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, o, "get", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        o,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        o,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        opt.clone(),
        Intrinsic::None,
    );
    method(
        st,
        o,
        "flatMap",
        vec![fn1(ta.clone(), opt.clone())],
        opt.clone(),
        Intrinsic::None,
    );
    method(
        st,
        o,
        "withFilter",
        vec![fn1(ta.clone(), Type::Boolean)],
        if library_abi {
            Type::Class {
                sym: option_wf,
                args: vec![ta],
            }
        } else {
            opt
        },
        Intrinsic::None,
    );

    let some = st.some_sym;
    let sa = type_param(st, some, "A");
    st.get_mut(some).tparams = vec![sa];
    let tsa = Type::TypeParam(sa);
    // `Some[A] extends Option[A]`: without the argument a `case Some(x)` on an
    // `Option[Int]` cannot recover `Int`.
    st.get_mut(some).parents = vec![Type::Class {
        sym: o,
        args: vec![tsa.clone()],
    }];
    method(
        st,
        some,
        "<init>",
        vec![tsa.clone()],
        Type::Class {
            sym: some,
            args: vec![tsa.clone()],
        },
        Intrinsic::None,
    );
    st.get_mut(some).ctor_fields = {
        // The jar's `Some.value` field is private; destructuring goes through
        // the `value()` accessor. The private runtime keeps a public field.
        let acc = if library_abi { "value" } else { "" };
        let f = st.alloc("value", some, SymKind::Term, Flags::PARAM, acc);
        st.get_mut(f).ty = tsa;
        vec![f]
    };
    // `st.none_sym` is the *module* symbol; its expression type is
    // `Type::ModuleRef(<module class>)` (see `prelude::module`), so anything
    // that walks `None`'s ancestry from a typed `None` expression (e.g.
    // `SymbolTable::lub`'s `base_type_seq`) reads the *module class*'s
    // `parents`, not the module's own. Setting `.parents` on `none_sym` here
    // was a no-op for that purpose: `module_extending` (which created
    // `none_sym`) had already stamped the module *class* with the raw,
    // unparameterized `Option` from its `parent` argument, and this line
    // never touched that copy. The result: `lub(None, Some(x))` degraded to
    // raw `Option` (dropping the element type) instead of `Option[X]`,
    // e.g. `val r = if (c) None else Some(x)` losing `x`'s type. Fixed by
    // writing to the module class, matching `module_extending`.
    let none_cls = st.module_class_of(st.none_sym);
    st.get_mut(none_cls).parents = vec![Type::Class {
        sym: o,
        args: vec![Type::Nothing],
    }];
}
pub(crate) fn add_cons_members(st: &mut SymbolTable, library_abi: bool) {
    let cons = st.cons_sym;
    let ca = type_param(st, cons, "A");
    st.get_mut(cons).tparams = vec![ca];
    let tca = Type::TypeParam(ca);
    let list_ca = Type::Class {
        sym: st.list_sym,
        args: vec![tca.clone()],
    };
    // `::[A] extends List[A]`, so `case h :: t` on a `List[Int]` binds `h: Int`.
    st.get_mut(cons).parents = vec![list_ca.clone()];
    // `Nil` is built before `List` has its type parameter, so the parent it
    // was given then is the *raw* `List`. Restate it now that `List[A]`
    // exists — and on the module *class*, which is what `Type::ModuleRef`
    // names and where the parent walk looks; the module symbol's own parent
    // list is never consulted.
    let nil_cls = st.module_class_of(st.nil_sym);
    let nil_parent = vec![Type::Class {
        sym: st.list_sym,
        args: vec![Type::Nothing],
    }];
    st.get_mut(st.nil_sym).parents = nil_parent.clone();
    st.get_mut(nil_cls).parents = nil_parent;
    let (head_acc, tail_acc) = if library_abi {
        ("head", "tail")
    } else {
        ("", "")
    };
    let h = st.alloc("head", cons, SymKind::Term, Flags::PARAM, head_acc);
    st.get_mut(h).ty = tca;
    let t = st.alloc("tl", cons, SymKind::Term, Flags::PARAM, tail_acc);
    st.get_mut(t).ty = list_ca;
    st.get_mut(cons).ctor_fields = vec![h, t];
    let f = st.get(cons).flags.with(Flags::CASE);
    st.get_mut(cons).flags = f;
}
pub(crate) fn add_list_members(
    st: &mut SymbolTable,
    with_filter: SymbolId,
    iterator: Option<SymbolId>,
    library_abi: bool,
) {
    let l = st.list_sym;
    let a = type_param(st, l, "A");
    st.get_mut(l).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let list_t = Type::Class {
        sym: l,
        args: vec![ta.clone()],
    };
    method(st, l, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, l, "head", vec![], ta.clone(), Intrinsic::None);
    method(st, l, "tail", vec![], list_t.clone(), Intrinsic::None);
    method(
        st,
        l,
        "::",
        vec![Type::Any],
        list_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        l,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        l,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        list_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        l,
        "flatMap",
        vec![fn1(ta.clone(), list_t.clone())],
        list_t.clone(),
        Intrinsic::None,
    );
    let wf_ret = if library_abi {
        Type::Class {
            sym: with_filter,
            // `CC` is the *constructor* `List`, so `map[B]` gives `List[B]`.
            args: vec![
                ta.clone(),
                Type::Class {
                    sym: l,
                    args: vec![],
                },
            ],
        }
    } else {
        list_t.clone()
    };
    method(
        st,
        l,
        "withFilter",
        vec![fn1(ta.clone(), Type::Boolean)],
        wf_ret,
        Intrinsic::None,
    );
    if let Some(it) = iterator {
        method(
            st,
            l,
            "iterator",
            vec![],
            Type::Class {
                sym: it,
                args: vec![ta.clone()],
            },
            Intrinsic::None,
        );
    }

    let list_mod = module(st, st.scala_pkg, "List", "scala/collection/immutable/List$");
    let mcls = st.module_class_of(list_mod);
    let seq = method(
        st,
        mcls,
        "unapplySeq",
        vec![list_t.clone()],
        Type::Class {
            sym: st.option_sym,
            args: vec![list_t.clone()],
        },
        Intrinsic::None,
    );
    let _ = seq;
    if library_abi {
        let list_apply = method(
            st,
            mcls,
            "apply",
            vec![Type::Repeated(Box::new(Type::Any))],
            list_t.clone(),
            Intrinsic::None,
        );
        let la = type_param(st, list_apply, "A");
        st.get_mut(list_apply).tparams = vec![la];
        st.get_mut(list_apply).ty = Type::Method {
            paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(la)))]],
            ret: Box::new(Type::Class {
                sym: l,
                args: vec![Type::TypeParam(la)],
            }),
        };
    }
    let mems = st.get(mcls).members.clone();
    st.get_mut(list_mod).members.extend(mems);
}
pub(crate) fn add_function_types(st: &mut SymbolTable) {
    // Function3/4 are needed for `Using.resources` 3–4 resource overloads.
    for n in 0..=4 {
        let f = iface(
            st,
            st.scala_pkg,
            &format!("Function{n}"),
            &format!("scala/Function{n}"),
        );
        // `FunctionN` really is `FunctionN[-T1, …, -Tn, +R]`, and `apply` is
        // its one *abstract* method. Both matter: a trait written as
        // `trait C[-T] extends (T => R)` inherits that `apply`, and reading it
        // through `C[X]` is what makes `C` a SAM whose parameter is `X`.
        // Without the parameters there is nothing for `subst_as_seen_from` to
        // substitute, and without `ABSTRACT` the SAM search finds no method.
        let mut tps: Vec<SymbolId> = (1..=n)
            .map(|i| type_param(st, f, &format!("T{i}")))
            .collect();
        let r = type_param(st, f, "R");
        tps.push(r);
        st.get_mut(f).tparams = tps.clone();
        let params: Vec<Type> = tps[..n].iter().map(|p| Type::TypeParam(*p)).collect();
        let apply = method(st, f, "apply", params, Type::TypeParam(r), Intrinsic::None);
        st.get_mut(apply).flags = Flags::ABSTRACT;
    }
}
pub(crate) fn add_partial_function(st: &mut SymbolTable) {
    let f1 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Function1")
        .unwrap_or(SymbolId::NONE);
    let pf = iface(st, st.scala_pkg, "PartialFunction", "scala/PartialFunction");
    let a = type_param(st, pf, "A");
    let b = type_param(st, pf, "B");
    st.get_mut(pf).tparams = vec![a, b];
    let ta = Type::TypeParam(a);
    let tb = Type::TypeParam(b);
    st.get_mut(pf).parents = vec![
        Type::Class {
            sym: f1,
            args: vec![ta.clone(), tb.clone()],
        },
        Type::AnyRef,
    ];
    method(
        st,
        pf,
        "apply",
        vec![ta.clone()],
        tb.clone(),
        Intrinsic::None,
    );
    method(
        st,
        pf,
        "isDefinedAt",
        vec![ta.clone()],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        pf,
        "applyOrElse",
        vec![ta.clone(), fn1(ta, tb.clone())],
        tb,
        Intrinsic::None,
    );
}
pub(crate) fn add_list_collect(st: &mut SymbolTable) {
    let pf = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "PartialFunction")
        .unwrap_or(SymbolId::NONE);
    let l = st.list_sym;
    let a = st.get(l).tparams.first().copied().unwrap_or(SymbolId::NONE);
    let ta = if a.is_none() {
        Type::Any
    } else {
        Type::TypeParam(a)
    };
    let list_t = Type::Class {
        sym: l,
        args: vec![ta.clone()],
    };
    let pf_ty = Type::Class {
        sym: pf,
        args: vec![ta, Type::Any],
    };
    method(st, l, "collect", vec![pf_ty], list_t, Intrinsic::None);
}
pub(crate) fn add_map_and_vector(st: &mut SymbolTable) {
    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2")
        .unwrap_or(SymbolId::NONE);

    let map = iface(st, st.scala_pkg, "Map", "scala/collection/immutable/Map");
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
    method(
        st,
        map,
        "apply",
        vec![Type::Any],
        tv.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
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
        map,
        "updated",
        vec![Type::Any, Type::Any],
        map_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "+",
        vec![pair.clone()],
        map_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        map,
        "foreach",
        vec![fn1(pair.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let map_mod = module(st, st.scala_pkg, "Map", "scala/collection/immutable/Map$");
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
        vec![Type::Repeated(Box::new(pair.clone()))],
        map_t.clone(),
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

    let vec = class(
        st,
        st.scala_pkg,
        "Vector",
        "scala/collection/immutable/Vector",
        &[Type::AnyRef],
    );
    let va = type_param(st, vec, "A");
    st.get_mut(vec).tparams = vec![va];
    let ta = Type::TypeParam(va);
    let vec_t = Type::Class {
        sym: vec,
        args: vec![ta.clone()],
    };
    method(
        st,
        vec,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(st, vec, "length", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        vec,
        "updated",
        vec![Type::Int, Type::Any],
        vec_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        vec,
        ":+",
        vec![Type::Any],
        vec_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        vec,
        "foreach",
        vec![fn1(ta, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let vec_mod = module(
        st,
        st.scala_pkg,
        "Vector",
        "scala/collection/immutable/Vector$",
    );
    let vec_cls = st.module_class_of(vec_mod);
    method(
        st,
        vec_cls,
        "empty",
        vec![],
        Type::Class {
            sym: vec,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let vec_apply = method(
        st,
        vec_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        vec_t.clone(),
        Intrinsic::None,
    );
    let vaa = type_param(st, vec_apply, "A");
    st.get_mut(vec_apply).tparams = vec![vaa];
    st.get_mut(vec_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(vaa)))]],
        ret: Box::new(Type::Class {
            sym: vec,
            args: vec![Type::TypeParam(vaa)],
        }),
    };
    let mems = st.get(vec_cls).members.clone();
    st.get_mut(vec_mod).members.extend(mems);
}
pub(crate) fn add_set(st: &mut SymbolTable) {
    let set = iface(st, st.scala_pkg, "Set", "scala/collection/immutable/Set");
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
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(
        st,
        set,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    let set_mod = module(st, st.scala_pkg, "Set", "scala/collection/immutable/Set$");
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
}
pub(crate) fn add_seq_and_lazylist(st: &mut SymbolTable) {
    let seq = iface(st, st.scala_pkg, "Seq", "scala/collection/immutable/Seq");
    let sa = type_param(st, seq, "A");
    st.get_mut(seq).tparams = vec![sa];
    let ta = Type::TypeParam(sa);
    let seq_t = Type::Class {
        sym: seq,
        args: vec![ta.clone()],
    };
    method(
        st,
        seq,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        seq,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    method(st, seq, "length", vec![], Type::Int, Intrinsic::None);
    let seq_mod = module(st, st.scala_pkg, "Seq", "scala/collection/immutable/Seq$");
    let seq_cls = st.module_class_of(seq_mod);
    method(
        st,
        seq_cls,
        "empty",
        vec![],
        Type::Class {
            sym: seq,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let seq_apply = method(
        st,
        seq_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        seq_t.clone(),
        Intrinsic::None,
    );
    let saa = type_param(st, seq_apply, "A");
    st.get_mut(seq_apply).tparams = vec![saa];
    st.get_mut(seq_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(saa)))]],
        ret: Box::new(Type::Class {
            sym: seq,
            args: vec![Type::TypeParam(saa)],
        }),
    };
    let mems = st.get(seq_cls).members.clone();
    st.get_mut(seq_mod).members.extend(mems);

    let ll = class(
        st,
        st.scala_pkg,
        "LazyList",
        "scala/collection/immutable/LazyList",
        &[Type::AnyRef],
    );
    let la = type_param(st, ll, "A");
    st.get_mut(ll).tparams = vec![la];
    let tll = Type::TypeParam(la);
    let ll_t = Type::Class {
        sym: ll,
        args: vec![tll.clone()],
    };
    method(
        st,
        ll,
        "foreach",
        vec![fn1(tll.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, ll, "apply", vec![Type::Int], tll, Intrinsic::None);
    let ll_mod = module(
        st,
        st.scala_pkg,
        "LazyList",
        "scala/collection/immutable/LazyList$",
    );
    let ll_cls = st.module_class_of(ll_mod);
    method(
        st,
        ll_cls,
        "empty",
        vec![],
        Type::Class {
            sym: ll,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let ll_apply = method(
        st,
        ll_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        ll_t,
        Intrinsic::None,
    );
    let lla = type_param(st, ll_apply, "A");
    st.get_mut(ll_apply).tparams = vec![lla];
    st.get_mut(ll_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(lla)))]],
        ret: Box::new(Type::Class {
            sym: ll,
            args: vec![Type::TypeParam(lla)],
        }),
    };
    let mems = st.get(ll_cls).members.clone();
    st.get_mut(ll_mod).members.extend(mems);

    // `List` is a `Seq` in 2.13; XML `Elem` takes `Seq[Node]`.
    st.get_mut(st.list_sym).parents.push(Type::Class {
        sym: seq,
        args: vec![],
    });
    // `SeqHasAsJava` takes `scala.collection.Seq`, not `immutable.Seq`.
    let coll_seq = crate::classpath::find_or_stub_java_class(st, "scala/collection/Seq");
    let la = st.get(st.list_sym).tparams[0];
    st.get_mut(st.list_sym).parents.push(Type::Class {
        sym: coll_seq,
        args: vec![Type::TypeParam(la)],
    });
}
/// `scala.collection.View` / `SeqView` against 2.13.16.
///
/// JVM: `SeqOps.view:()SeqView`, `SeqView.map:(Function1)SeqView`,
/// `View`/`SeqView.toList:()List`, `View$.fill(I, Function0)Object`,
/// `View$.iterate(Object, I, Function1)Object`. No private View classfile.
pub(crate) fn add_view(st: &mut SymbolTable) {
    let coll = crate::classpath::ensure_package(st, "scala/collection");
    let view = iface(st, coll, "View", "scala/collection/View");
    let va = type_param(st, view, "A");
    st.get_mut(view).tparams = vec![va];
    let vta = Type::TypeParam(va);
    let list_sym = st.list_sym;
    let view_t = |a: Type| Type::Class {
        sym: view,
        args: vec![a],
    };
    let list_t = |a: Type| Type::Class {
        sym: list_sym,
        args: vec![a],
    };
    method(
        st,
        view,
        "toList",
        vec![],
        list_t(vta.clone()),
        Intrinsic::None,
    );
    let vmap = method(st, view, "map", vec![], Type::Unit, Intrinsic::None);
    let vb = type_param(st, vmap, "B");
    let vf = st.alloc("f", vmap, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(vf).ty = fn1(vta.clone(), Type::TypeParam(vb));
    st.get_mut(vmap).tparams = vec![vb];
    st.get_mut(vmap).params = vec![vf];
    st.get_mut(vmap).paramss = vec![vec![vf]];
    st.get_mut(vmap).ty = Type::Method {
        paramss: vec![vec![fn1(vta.clone(), Type::TypeParam(vb))]],
        ret: Box::new(view_t(Type::TypeParam(vb))),
    };

    let seq_view = iface(st, coll, "SeqView", "scala/collection/SeqView");
    let sa = type_param(st, seq_view, "A");
    st.get_mut(seq_view).tparams = vec![sa];
    let sta = Type::TypeParam(sa);
    st.get_mut(seq_view).parents = vec![
        Type::AnyRef,
        Type::Class {
            sym: view,
            args: vec![sta.clone()],
        },
    ];
    method(
        st,
        seq_view,
        "toList",
        vec![],
        list_t(sta.clone()),
        Intrinsic::None,
    );
    let smap = method(st, seq_view, "map", vec![], Type::Unit, Intrinsic::None);
    let sb = type_param(st, smap, "B");
    let sf = st.alloc("f", smap, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(sf).ty = fn1(sta.clone(), Type::TypeParam(sb));
    st.get_mut(smap).tparams = vec![sb];
    st.get_mut(smap).params = vec![sf];
    st.get_mut(smap).paramss = vec![vec![sf]];
    st.get_mut(smap).ty = Type::Method {
        paramss: vec![vec![fn1(sta.clone(), Type::TypeParam(sb))]],
        ret: Box::new(Type::Class {
            sym: seq_view,
            args: vec![Type::TypeParam(sb)],
        }),
    };

    if let Some(la) = st.get(st.list_sym).tparams.first().copied() {
        method(
            st,
            st.list_sym,
            "view",
            vec![],
            Type::Class {
                sym: seq_view,
                args: vec![Type::TypeParam(la)],
            },
            Intrinsic::None,
        );
    }

    let view_mod = module(st, coll, "View", "scala/collection/View$");
    let view_cls = st.module_class_of(view_mod);

    let fill = method(st, view_cls, "fill", vec![], Type::Unit, Intrinsic::None);
    let fa = type_param(st, fill, "A");
    let n = st.alloc("n", fill, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(n).ty = Type::Int;
    let elem = st.alloc("elem", fill, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(elem).ty = Type::ByName(Box::new(Type::TypeParam(fa)));
    st.get_mut(fill).tparams = vec![fa];
    st.get_mut(fill).params = vec![n, elem];
    st.get_mut(fill).paramss = vec![vec![n], vec![elem]];
    st.get_mut(fill).ty = Type::Method {
        paramss: vec![
            vec![Type::Int],
            vec![Type::ByName(Box::new(Type::TypeParam(fa)))],
        ],
        ret: Box::new(view_t(Type::TypeParam(fa))),
    };
    st.set_jvm_name(fill, "(ILscala/Function0;)Ljava/lang/Object;");

    let iterate = method(st, view_cls, "iterate", vec![], Type::Unit, Intrinsic::None);
    let ia = type_param(st, iterate, "A");
    let start = st.alloc(
        "start",
        iterate,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(start).ty = Type::TypeParam(ia);
    let len = st.alloc(
        "len",
        iterate,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(len).ty = Type::Int;
    let f = st.alloc("f", iterate, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(Type::TypeParam(ia), Type::TypeParam(ia));
    st.get_mut(iterate).tparams = vec![ia];
    st.get_mut(iterate).params = vec![start, len, f];
    st.get_mut(iterate).paramss = vec![vec![start, len], vec![f]];
    st.get_mut(iterate).ty = Type::Method {
        paramss: vec![
            vec![Type::TypeParam(ia), Type::Int],
            vec![fn1(Type::TypeParam(ia), Type::TypeParam(ia))],
        ],
        ret: Box::new(view_t(Type::TypeParam(ia))),
    };
    st.get_mut(iterate).jvm_name =
        "(Ljava/lang/Object;ILscala/Function1;)Ljava/lang/Object;".into();

    let mems = st.get(view_cls).members.clone();
    st.get_mut(view_mod).members.extend(mems);
}
pub(crate) fn add_indexedseq_and_queue(st: &mut SymbolTable) {
    let idx = iface(
        st,
        st.scala_pkg,
        "IndexedSeq",
        "scala/collection/immutable/IndexedSeq",
    );
    let ia = type_param(st, idx, "A");
    st.get_mut(idx).tparams = vec![ia];
    let ta = Type::TypeParam(ia);
    let idx_t = Type::Class {
        sym: idx,
        args: vec![ta.clone()],
    };
    method(
        st,
        idx,
        "apply",
        vec![Type::Int],
        ta.clone(),
        Intrinsic::None,
    );
    let idx_mod = module(
        st,
        st.scala_pkg,
        "IndexedSeq",
        "scala/collection/immutable/IndexedSeq$",
    );
    let idx_cls = st.module_class_of(idx_mod);
    method(
        st,
        idx_cls,
        "empty",
        vec![],
        Type::Class {
            sym: idx,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let idx_apply = method(
        st,
        idx_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        idx_t.clone(),
        Intrinsic::None,
    );
    let iaa = type_param(st, idx_apply, "A");
    st.get_mut(idx_apply).tparams = vec![iaa];
    st.get_mut(idx_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(iaa)))]],
        ret: Box::new(Type::Class {
            sym: idx,
            args: vec![Type::TypeParam(iaa)],
        }),
    };
    let mems = st.get(idx_cls).members.clone();
    st.get_mut(idx_mod).members.extend(mems);

    let tuple2 = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|id| st.get(*id).name == "Tuple2")
        .unwrap_or(SymbolId::NONE);
    let imm = crate::classpath::ensure_package(st, "scala/collection/immutable");
    let queue = class(
        st,
        imm,
        "Queue",
        "scala/collection/immutable/Queue",
        &[Type::AnyRef],
    );
    let qa = type_param(st, queue, "A");
    st.get_mut(queue).tparams = vec![qa];
    let tq = Type::TypeParam(qa);
    let queue_t = Type::Class {
        sym: queue,
        args: vec![tq.clone()],
    };
    method(
        st,
        queue,
        "enqueue",
        vec![Type::Any],
        queue_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        queue,
        "dequeue",
        vec![],
        Type::Class {
            sym: tuple2,
            args: vec![tq.clone(), queue_t.clone()],
        },
        Intrinsic::None,
    );
    method(
        st,
        queue,
        "apply",
        vec![Type::Int],
        tq.clone(),
        Intrinsic::None,
    );
    let queue_mod = module(st, imm, "Queue", "scala/collection/immutable/Queue$");
    let queue_cls = st.module_class_of(queue_mod);
    method(
        st,
        queue_cls,
        "empty",
        vec![],
        Type::Class {
            sym: queue,
            args: vec![Type::Any],
        },
        Intrinsic::None,
    );
    let q_apply = method(
        st,
        queue_cls,
        "apply",
        vec![Type::Repeated(Box::new(Type::Any))],
        queue_t.clone(),
        Intrinsic::None,
    );
    let qaa = type_param(st, q_apply, "A");
    st.get_mut(q_apply).tparams = vec![qaa];
    st.get_mut(q_apply).ty = Type::Method {
        paramss: vec![vec![Type::Repeated(Box::new(Type::TypeParam(qaa)))]],
        ret: Box::new(Type::Class {
            sym: queue,
            args: vec![Type::TypeParam(qaa)],
        }),
    };
    let mems = st.get(queue_cls).members.clone();
    st.get_mut(queue_mod).members.extend(mems);
}
