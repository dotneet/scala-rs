use crate::prelude::{class, fn1, fn_n, iface, mark_java, method, module, type_param};
use crate::symbol::{Intrinsic, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// `scala.util.chaining` (`package$chaining$`) + `ChainingOps` against 2.13.16.
///
/// `import scala.util.chaining._` brings IMPLICIT `scalaUtilChainingOps`.
/// JVM: `pipe$extension` / `tap$extension(Object, Function1)Object`.
pub(crate) fn add_chaining(st: &mut SymbolTable) {
    let util = crate::classpath::ensure_package(st, "scala/util");
    let ops = class(
        st,
        util,
        "ChainingOps",
        "scala/util/ChainingOps",
        &[Type::AnyVal],
    );
    let a = type_param(st, ops, "A");
    st.get_mut(ops).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let self_f = st.alloc("self", ops, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(self_f).ty = ta.clone();
    st.get_mut(ops).ctor_fields = vec![self_f];

    let pipe = method(st, ops, "pipe", vec![], Type::Unit, Intrinsic::None);
    let b = type_param(st, pipe, "B");
    let f = st.alloc("f", pipe, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(ta.clone(), Type::TypeParam(b));
    st.get_mut(pipe).tparams = vec![b];
    st.get_mut(pipe).params = vec![f];
    st.get_mut(pipe).paramss = vec![vec![f]];
    st.get_mut(pipe).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), Type::TypeParam(b))]],
        ret: Box::new(Type::TypeParam(b)),
    };

    let tap = method(st, ops, "tap", vec![], Type::Unit, Intrinsic::None);
    let u = type_param(st, tap, "U");
    let g = st.alloc("f", tap, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(g).ty = fn1(ta.clone(), Type::TypeParam(u));
    st.get_mut(tap).tparams = vec![u];
    st.get_mut(tap).params = vec![g];
    st.get_mut(tap).paramss = vec![vec![g]];
    st.get_mut(tap).ty = Type::Method {
        paramss: vec![vec![fn1(ta.clone(), Type::TypeParam(u))]],
        ret: Box::new(ta.clone()),
    };

    let chaining = module(st, util, "chaining", "scala/util/package$chaining$");
    let mcls = st.module_class_of(chaining);
    let conv = method(
        st,
        mcls,
        "scalaUtilChainingOps",
        vec![Type::Any],
        Type::Class {
            sym: ops,
            args: vec![],
        },
        Intrinsic::Identity,
    );
    let ca = type_param(st, conv, "A");
    let cta = Type::TypeParam(ca);
    st.get_mut(conv).tparams = vec![ca];
    st.get_mut(conv).ty = Type::Method {
        paramss: vec![vec![cta.clone()]],
        ret: Box::new(Type::Class {
            sym: ops,
            args: vec![cta],
        }),
    };
    st.get_mut(conv).flags = st.get(conv).flags.with(Flags::IMPLICIT);
    let mems = st.get(mcls).members.clone();
    st.get_mut(chaining).members.extend(mems);
}
/// `scala.util.Using.resource` / `Using.apply` / `Using.Manager` / `Using.resources`
/// + `Releasable[-R]` against scala-library 2.13.16.
///
/// nsc 2.13.16 JVM:
/// `Using$.resource(Object, Function1, Using$Releasable)Object`,
/// `Using$.apply(Function0, Function1, Using$Releasable)Try`,
/// `Using$Manager$.apply(Function1)Try`,
/// `Using$Manager.apply/acquire(Object, Using$Releasable)`,
/// `Using$.resources` 2–4 resource overloads (Function2/3/4).
/// Implicit `Using$Releasable$AutoCloseableIsReleasable$.MODULE$`.
pub(crate) fn add_using(st: &mut SymbolTable) {
    let util = crate::classpath::ensure_package(st, "scala/util");
    let java_lang = crate::classpath::ensure_package(st, "java/lang");
    let auto_closeable = st
        .lookup_member(java_lang, "AutoCloseable")
        .into_iter()
        .find(|&id| st.get(id).is_class_like())
        .unwrap_or_else(|| {
            let ac = iface(st, java_lang, "AutoCloseable", "java/lang/AutoCloseable");
            mark_java(st, ac);
            let close = st.alloc(
                "close",
                ac,
                crate::symbol::SymKind::Method,
                Flags::ABSTRACT,
                "",
            );
            st.get_mut(close).ty = Type::Method {
                paramss: Vec::new(),
                ret: Box::new(Type::Unit),
            };
            ac
        });

    let releasable = iface(st, util, "Releasable", "scala/util/Using$Releasable");
    let r = type_param(st, releasable, "R");
    st.get_mut(r).flags = st.get(r).flags.with(Flags::CONTRAVARIANT);
    st.get_mut(releasable).tparams = vec![r];
    let release = st.alloc(
        "release",
        releasable,
        crate::symbol::SymKind::Method,
        Flags::ABSTRACT,
        "",
    );
    st.get_mut(release).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(r)]],
        ret: Box::new(Type::Unit),
    };

    let rel_mod = module(st, util, "Releasable", "scala/util/Using$Releasable$");
    let rel_cls = st.module_class_of(rel_mod);
    crate::prelude_ordering2::add_ordering_instance(
        st,
        rel_cls,
        releasable,
        "AutoCloseableIsReleasable",
        "scala/util/Using$Releasable$AutoCloseableIsReleasable$",
        Type::Class {
            sym: auto_closeable,
            args: vec![],
        },
    );
    let mems = st.get(rel_cls).members.clone();
    st.get_mut(rel_mod).members.extend(mems);

    let using_mod = module(st, util, "Using", "scala/util/Using$");
    let using_cls = st.module_class_of(using_mod);
    let res = method(
        st,
        using_cls,
        "resource",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let rr = type_param(st, res, "R");
    let aa = type_param(st, res, "A");
    st.get_mut(res).tparams = vec![rr, aa];
    let r_t = Type::TypeParam(rr);
    let a_t = Type::TypeParam(aa);
    let resource = st.alloc(
        "resource",
        res,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(resource).ty = r_t.clone();
    let f = st.alloc("f", res, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn1(r_t.clone(), a_t.clone());
    let ev = st.alloc(
        "releasable",
        res,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ev).ty = Type::Class {
        sym: releasable,
        args: vec![r_t.clone()],
    };
    st.get_mut(res).params = vec![resource, f, ev];
    st.get_mut(res).paramss = vec![vec![resource], vec![f], vec![ev]];
    st.get_mut(res).ty = Type::Method {
        paramss: vec![
            vec![r_t.clone()],
            vec![fn1(r_t.clone(), a_t.clone())],
            vec![Type::Class {
                sym: releasable,
                args: vec![r_t],
            }],
        ],
        ret: Box::new(a_t),
    };

    let try_c = st
        .lookup_member(st.scala_pkg, "Try")
        .into_iter()
        .find(|&id| st.get(id).kind == crate::symbol::SymKind::Class)
        .expect("Try");

    // nsc `Using.apply[R, A](resource: => R)(f: R => A)(implicit Releasable[R]): Try[A]`
    // JVM: `Using$.apply(Function0, Function1, Using$Releasable)Try`
    let app = method(st, using_cls, "apply", vec![], Type::Unit, Intrinsic::None);
    let ar = type_param(st, app, "R");
    let aa2 = type_param(st, app, "A");
    st.get_mut(app).tparams = vec![ar, aa2];
    let ar_t = Type::TypeParam(ar);
    let aa2_t = Type::TypeParam(aa2);
    let app_res = st.alloc(
        "resource",
        app,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(app_res).ty = Type::ByName(Box::new(ar_t.clone()));
    let app_f = st.alloc("f", app, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(app_f).ty = fn1(ar_t.clone(), aa2_t.clone());
    let app_ev = st.alloc(
        "releasable",
        app,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(app_ev).ty = Type::Class {
        sym: releasable,
        args: vec![ar_t.clone()],
    };
    st.get_mut(app).params = vec![app_res, app_f, app_ev];
    st.get_mut(app).paramss = vec![vec![app_res], vec![app_f], vec![app_ev]];
    st.get_mut(app).ty = Type::Method {
        paramss: vec![
            vec![Type::ByName(Box::new(ar_t.clone()))],
            vec![fn1(ar_t.clone(), aa2_t.clone())],
            vec![Type::Class {
                sym: releasable,
                args: vec![ar_t],
            }],
        ],
        ret: Box::new(Type::Class {
            sym: try_c,
            args: vec![aa2_t],
        }),
    };
    st.get_mut(app).jvm_name =
        "(Lscala/Function0;Lscala/Function1;Lscala/util/Using$Releasable;)Lscala/util/Try;".into();

    let manager_cls = class(
        st,
        using_cls,
        "Manager",
        "scala/util/Using$Manager",
        &[Type::AnyRef],
    );
    method(
        st,
        manager_cls,
        "<init>",
        vec![],
        Type::Class {
            sym: manager_cls,
            args: vec![],
        },
        Intrinsic::None,
    );
    let mgr_t = Type::Class {
        sym: manager_cls,
        args: vec![],
    };

    let mgr_app = method(
        st,
        manager_cls,
        "apply",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let mr = type_param(st, mgr_app, "R");
    st.get_mut(mgr_app).tparams = vec![mr];
    let mr_t = Type::TypeParam(mr);
    let mgr_res = st.alloc(
        "resource",
        mgr_app,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(mgr_res).ty = mr_t.clone();
    let mgr_ev = st.alloc(
        "releasable",
        mgr_app,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(mgr_ev).ty = Type::Class {
        sym: releasable,
        args: vec![mr_t.clone()],
    };
    st.get_mut(mgr_app).params = vec![mgr_res, mgr_ev];
    st.get_mut(mgr_app).paramss = vec![vec![mgr_res], vec![mgr_ev]];
    st.get_mut(mgr_app).ty = Type::Method {
        paramss: vec![
            vec![mr_t.clone()],
            vec![Type::Class {
                sym: releasable,
                args: vec![mr_t.clone()],
            }],
        ],
        ret: Box::new(mr_t),
    };
    st.get_mut(mgr_app).jvm_name =
        "(Ljava/lang/Object;Lscala/util/Using$Releasable;)Ljava/lang/Object;".into();

    let mgr_acq = method(
        st,
        manager_cls,
        "acquire",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let acr = type_param(st, mgr_acq, "R");
    st.get_mut(mgr_acq).tparams = vec![acr];
    let acr_t = Type::TypeParam(acr);
    let acq_res = st.alloc(
        "resource",
        mgr_acq,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(acq_res).ty = acr_t.clone();
    let acq_ev = st.alloc(
        "releasable",
        mgr_acq,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(acq_ev).ty = Type::Class {
        sym: releasable,
        args: vec![acr_t.clone()],
    };
    st.get_mut(mgr_acq).params = vec![acq_res, acq_ev];
    st.get_mut(mgr_acq).paramss = vec![vec![acq_res], vec![acq_ev]];
    st.get_mut(mgr_acq).ty = Type::Method {
        paramss: vec![
            vec![acr_t.clone()],
            vec![Type::Class {
                sym: releasable,
                args: vec![acr_t],
            }],
        ],
        ret: Box::new(Type::Unit),
    };
    st.set_jvm_name(
        mgr_acq,
        "(Ljava/lang/Object;Lscala/util/Using$Releasable;)V",
    );

    let manager_mod = module(st, using_cls, "Manager", "scala/util/Using$Manager$");
    let manager_mcls = st.module_class_of(manager_mod);
    let mobj_app = method(
        st,
        manager_mcls,
        "apply",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let ma = type_param(st, mobj_app, "A");
    st.get_mut(mobj_app).tparams = vec![ma];
    let ma_t = Type::TypeParam(ma);
    let op = st.alloc(
        "op",
        mobj_app,
        crate::symbol::SymKind::Term,
        Flags::PARAM,
        "",
    );
    st.get_mut(op).ty = fn1(mgr_t, ma_t.clone());
    st.get_mut(mobj_app).params = vec![op];
    st.get_mut(mobj_app).paramss = vec![vec![op]];
    st.get_mut(mobj_app).ty = Type::Method {
        paramss: vec![vec![fn1(
            Type::Class {
                sym: manager_cls,
                args: vec![],
            },
            ma_t.clone(),
        )]],
        ret: Box::new(Type::Class {
            sym: try_c,
            args: vec![ma_t],
        }),
    };
    st.set_jvm_name(mobj_app, "(Lscala/Function1;)Lscala/util/Try;");
    let mm_mems = st.get(manager_mcls).members.clone();
    st.get_mut(manager_mod).members.extend(mm_mems);

    // nsc `Using.resources` 2–4 resource overloads. First resource is by-value,
    // later ones by-name; result is `A` (throws, unlike `Using.apply`).
    add_using_resources(st, using_cls, releasable, 2);
    add_using_resources(st, using_cls, releasable, 3);
    add_using_resources(st, using_cls, releasable, 4);

    let mems = st.get(using_cls).members.clone();
    st.get_mut(using_mod).members.extend(mems);
}
/// nsc `Using.resources[R1, …, Rn, A](r1, r2: => …)(f)(implicit Releasable*)`.
///
/// JVM 2-arg: `(Object, Function0, Function2, Releasable, Releasable)Object`
/// and similarly Function3/Function4 for n=3/4.
fn add_using_resources(st: &mut SymbolTable, using_cls: SymbolId, releasable: SymbolId, n: usize) {
    let m = method(
        st,
        using_cls,
        "resources",
        vec![],
        Type::Unit,
        Intrinsic::None,
    );
    let mut rs = Vec::new();
    for i in 1..=n {
        rs.push(type_param(st, m, &format!("R{i}")));
    }
    let a = type_param(st, m, "A");
    let mut tps = rs.clone();
    tps.push(a);
    st.get_mut(m).tparams = tps;

    let mut p_ids = Vec::new();
    let mut p_tys = Vec::new();
    for (i, r) in rs.iter().enumerate() {
        let base = Type::TypeParam(*r);
        let ty = if i == 0 {
            base
        } else {
            Type::ByName(Box::new(base))
        };
        let p = st.alloc(
            &format!("resource{}", i + 1),
            m,
            crate::symbol::SymKind::Term,
            Flags::PARAM,
            "",
        );
        st.get_mut(p).ty = ty.clone();
        p_ids.push(p);
        p_tys.push(ty);
    }

    let fn_ty = fn_n(
        rs.iter().map(|r| Type::TypeParam(*r)).collect(),
        Type::TypeParam(a),
    );
    let f = st.alloc("f", m, crate::symbol::SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = fn_ty.clone();

    let mut ev_ids = Vec::new();
    let mut ev_tys = Vec::new();
    for (i, r) in rs.iter().enumerate() {
        let ev = st.alloc(
            &format!("evidence${}", i + 1),
            m,
            crate::symbol::SymKind::Term,
            Flags::PARAM.with(Flags::IMPLICIT),
            "",
        );
        let et = Type::Class {
            sym: releasable,
            args: vec![Type::TypeParam(*r)],
        };
        st.get_mut(ev).ty = et.clone();
        ev_ids.push(ev);
        ev_tys.push(et);
    }

    let mut all_params = p_ids.clone();
    all_params.push(f);
    all_params.extend(ev_ids.iter().copied());
    st.get_mut(m).params = all_params;
    st.get_mut(m).paramss = vec![p_ids, vec![f], ev_ids];
    st.get_mut(m).ty = Type::Method {
        paramss: vec![p_tys, vec![fn_ty], ev_tys],
        ret: Box::new(Type::TypeParam(a)),
    };
    let mut desc = String::from("(Ljava/lang/Object;");
    for _ in 1..n {
        desc.push_str("Lscala/Function0;");
    }
    desc.push_str(&format!("Lscala/Function{n};"));
    for _ in 0..n {
        desc.push_str("Lscala/util/Using$Releasable;");
    }
    desc.push_str(")Ljava/lang/Object;");
    st.set_jvm_name(m, desc);
}
