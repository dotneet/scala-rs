use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub fn install_prelude(st: &mut SymbolTable, library_abi: bool) {
    let root = st.root;
    st.scala_pkg = st.alloc("scala", root, SymKind::Package, Flags::PACKAGE, "scala");
    let java = st.alloc("java", root, SymKind::Package, Flags::PACKAGE, "java");
    let java_lang = st.alloc("lang", java, SymKind::Package, Flags::PACKAGE, "java/lang");

    st.any_sym = class(st, st.scala_pkg, "Any", "java/lang/Object", &[]);
    st.anyref_sym = class(st, st.scala_pkg, "AnyRef", "java/lang/Object", &[Type::Any]);
    st.anyval_sym = class(st, st.scala_pkg, "AnyVal", "java/lang/Object", &[Type::Any]);
    st.object_sym = class(st, java_lang, "Object", "java/lang/Object", &[Type::AnyRef]);
    mark_java(st, st.object_sym);

    st.unit_sym = class(
        st,
        st.scala_pkg,
        "Unit",
        "scala/runtime/BoxedUnit",
        &[Type::AnyVal],
    );
    st.boolean_sym = class(
        st,
        st.scala_pkg,
        "Boolean",
        "java/lang/Boolean",
        &[Type::AnyVal],
    );
    st.int_sym = class(
        st,
        st.scala_pkg,
        "Int",
        "java/lang/Integer",
        &[Type::AnyVal],
    );
    st.long_sym = class(st, st.scala_pkg, "Long", "java/lang/Long", &[Type::AnyVal]);
    st.float_sym = class(
        st,
        st.scala_pkg,
        "Float",
        "java/lang/Float",
        &[Type::AnyVal],
    );
    st.double_sym = class(
        st,
        st.scala_pkg,
        "Double",
        "java/lang/Double",
        &[Type::AnyVal],
    );
    let char_s = class(
        st,
        st.scala_pkg,
        "Char",
        "java/lang/Character",
        &[Type::AnyVal],
    );
    let _ = char_s;

    st.string_sym = class(st, java_lang, "String", "java/lang/String", &[Type::AnyRef]);
    mark_java(st, st.string_sym);
    let throwable = class(
        st,
        java_lang,
        "Throwable",
        "java/lang/Throwable",
        &[Type::AnyRef],
    );
    mark_java(st, throwable);
    let exception = class(
        st,
        java_lang,
        "Exception",
        "java/lang/Exception",
        &[Type::Class {
            sym: throwable,
            args: vec![],
        }],
    );
    mark_java(st, exception);
    let _runtime_ex = class(
        st,
        java_lang,
        "RuntimeException",
        "java/lang/RuntimeException",
        &[Type::Class {
            sym: exception,
            args: vec![],
        }],
    );
    mark_java(st, _runtime_ex);
    let jclass = class(st, java_lang, "Class", "java/lang/Class", &[Type::AnyRef]);
    mark_java(st, jclass);
    method(st, jclass, "getName", vec![], Type::String, Intrinsic::None);
    st.array_sym = class(
        st,
        st.scala_pkg,
        "Array",
        "[java/lang/Object",
        &[Type::AnyRef],
    );
    st.option_sym = class(st, st.scala_pkg, "Option", "scala/Option", &[Type::AnyRef]);
    st.some_sym = class(
        st,
        st.scala_pkg,
        "Some",
        "scala/Some",
        &[Type::Class {
            sym: st.option_sym,
            args: vec![],
        }],
    );
    st.none_sym = module_extending(
        st,
        st.scala_pkg,
        "None",
        "scala/None$",
        Type::Class {
            sym: st.option_sym,
            args: vec![],
        },
    );
    st.list_sym = class(
        st,
        st.scala_pkg,
        "List",
        "scala/collection/immutable/List",
        &[Type::AnyRef],
    );
    st.nil_sym = module_extending(
        st,
        st.scala_pkg,
        "Nil",
        "scala/collection/immutable/Nil$",
        Type::Class {
            sym: st.list_sym,
            args: vec![],
        },
    );
    st.cons_sym = class(
        st,
        st.scala_pkg,
        "$colon$colon",
        "scala/collection/immutable/$colon$colon",
        &[Type::Class {
            sym: st.list_sym,
            args: vec![],
        }],
    );
    let cons_alias = st.alloc(
        "::",
        st.scala_pkg,
        SymKind::Class,
        Flags::CASE,
        "scala/collection/immutable/$colon$colon",
    );
    st.get_mut(cons_alias).ty = Type::Class {
        sym: st.cons_sym,
        args: vec![],
    };

    add_any_members(st);
    add_int_members(st);
    add_long_members(st);
    add_double_members(st);
    add_float_members(st);
    add_bool_members(st);
    add_string_members(st, library_abi);
    add_array_members(st);
    let with_filter = add_with_filter(st);
    let option_wf = add_option_with_filter(st);
    let iterator = if library_abi {
        Some(add_iterator(st))
    } else {
        None
    };
    let string_ops = if library_abi {
        Some(add_string_ops(st))
    } else {
        None
    };
    add_option_members(st, option_wf, library_abi);
    add_list_members(st, with_filter, iterator, library_abi);
    add_function_types(st);
    add_partial_function(st);
    if library_abi {
        add_list_collect(st);
        let ct = add_classtag(st, jclass);
        add_string_context(st);
        add_array_companion(st, ct);
    }
    let ordered = add_ordered(st);
    add_delayed_init_app(st);

    // Some companion with apply
    let some_mod = module(st, st.scala_pkg, "Some", "scala/Some$");
    let some_cls = st.module_class_of(some_mod);
    method(
        st,
        some_cls,
        "apply",
        vec![Type::Any],
        Type::Class {
            sym: st.some_sym,
            args: vec![],
        },
        Intrinsic::None,
    );
    let mems = st.get(some_cls).members.clone();
    st.get_mut(some_mod).members.extend(mems);

    let tuple2 = class(st, st.scala_pkg, "Tuple2", "scala/Tuple2", &[Type::AnyRef]);
    let t2a = type_param(st, tuple2, "A");
    let t2b = type_param(st, tuple2, "B");
    st.get_mut(tuple2).tparams = vec![t2a, t2b];
    let f1 = st.alloc("_1", tuple2, SymKind::Term, Flags::FINAL, "");
    st.get_mut(f1).ty = Type::TypeParam(t2a);
    let f2 = st.alloc("_2", tuple2, SymKind::Term, Flags::FINAL, "");
    st.get_mut(f2).ty = Type::TypeParam(t2b);
    st.get_mut(tuple2).ctor_fields = vec![f1, f2];
    method(
        st,
        tuple2,
        "<init>",
        vec![Type::Any, Type::Any],
        Type::Class {
            sym: tuple2,
            args: vec![],
        },
        Intrinsic::None,
    );
    let _ = class(st, st.scala_pkg, "Tuple3", "scala/Tuple3", &[Type::AnyRef]);

    // Marker trait `scala.Dynamic`. JVM interface lives in scala-library.jar;
    // we only need the symbol so `class D extends Dynamic` typechecks.
    let _dynamic = iface(st, st.scala_pkg, "Dynamic", "scala/Dynamic");
    let language = module(st, st.scala_pkg, "language", "scala/language$");
    let lang_cls = st.module_class_of(language);
    let dynamics = st.alloc(
        "dynamics",
        lang_cls,
        SymKind::Term,
        Flags::IMPLICIT.with(Flags::LAZY).with(Flags::FINAL),
        "",
    );
    st.get_mut(dynamics).ty = Type::Boolean;
    st.get_mut(language).members.push(dynamics);
    for feat in ["postfixOps", "implicitConversions"] {
        let id = st.alloc(
            feat,
            lang_cls,
            SymKind::Term,
            Flags::IMPLICIT.with(Flags::LAZY).with(Flags::FINAL),
            "",
        );
        st.get_mut(id).ty = Type::Boolean;
        st.get_mut(language).members.push(id);
    }

    let rich_int = if library_abi {
        Some(add_rich_int_and_range(st))
    } else {
        None
    };
    let rich_ldc = if library_abi {
        Some(add_rich_long_double_char(st))
    } else {
        None
    };
    if library_abi {
        add_map_and_vector(st);
        add_set(st);
        add_seq_and_lazylist(st);
        add_either(st);
        add_try(st, throwable);
        add_xml(st);
        add_enumeration(st);
    }
    add_annotation_pkg(st);
    add_java_sam(st, java, java_lang);

    let arrow = if library_abi {
        let a = class(
            st,
            st.scala_pkg,
            "ArrowAssoc",
            "scala/Predef$ArrowAssoc",
            &[Type::AnyVal],
        );
        let af = st.alloc("self", a, SymKind::Term, Flags::PARAM, "");
        st.get_mut(af).ty = Type::Any;
        st.get_mut(a).ctor_fields = vec![af];
        method(
            st,
            a,
            "->",
            vec![Type::Any],
            Type::Class {
                sym: tuple2,
                args: vec![Type::Any, Type::Any],
            },
            Intrinsic::None,
        );
        a
    } else {
        let a = class(
            st,
            st.scala_pkg,
            "ArrowAssoc",
            "scala/runtime/ArrowAssoc",
            &[Type::AnyRef],
        );
        let af = st.alloc("self", a, SymKind::Term, Flags::PARAM, "");
        st.get_mut(af).ty = Type::Any;
        st.get_mut(a).ctor_fields = vec![af];
        method(
            st,
            a,
            "->",
            vec![Type::Any],
            Type::Class {
                sym: tuple2,
                args: vec![Type::Any, Type::Any],
            },
            Intrinsic::None,
        );
        a
    };

    st.predef = module(st, st.scala_pkg, "Predef", "scala/Predef$");
    add_predef_members(st, arrow, string_ops, rich_int, rich_ldc, library_abi);

    st.push_scope();
    st.enter_in_current("scala", st.scala_pkg);
    st.enter_in_current("java", java);
    import_members(st, st.scala_pkg);
    import_members(st, java_lang);
    import_members(st, st.predef);
    st.enter_in_current("String", st.string_sym);
    st.enter_in_current("Unit", st.unit_sym);
    st.enter_in_current("::", st.cons_sym);
    st.enter_in_current("Ordered", ordered);
}

fn mark_java(st: &mut SymbolTable, id: SymbolId) {
    let f = st.get(id).flags.with(Flags::JAVA);
    st.get_mut(id).flags = f;
}

/// SIP-21 Java SAM types: `Runnable`, `Comparator[T]`, `java.util.function.Function`.
fn add_java_sam(st: &mut SymbolTable, java: SymbolId, java_lang: SymbolId) {
    let runnable = iface(st, java_lang, "Runnable", "java/lang/Runnable");
    mark_java(st, runnable);
    let run = st.alloc("run", runnable, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(run).ty = Type::Method {
        paramss: Vec::new(),
        ret: Box::new(Type::Unit),
    };

    let util = st.alloc("util", java, SymKind::Package, Flags::PACKAGE, "java/util");
    let comparator = iface(st, util, "Comparator", "java/util/Comparator");
    mark_java(st, comparator);
    let ct = type_param(st, comparator, "T");
    st.get_mut(comparator).tparams = vec![ct];
    let cmp = st.alloc("compare", comparator, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(cmp).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(ct), Type::TypeParam(ct)]],
        ret: Box::new(Type::Int),
    };

    let fn_pkg = st.alloc(
        "function",
        util,
        SymKind::Package,
        Flags::PACKAGE,
        "java/util/function",
    );
    let jfun = iface(st, fn_pkg, "Function", "java/util/function/Function");
    mark_java(st, jfun);
    let ft = type_param(st, jfun, "T");
    let fr = type_param(st, jfun, "R");
    st.get_mut(jfun).tparams = vec![ft, fr];
    let apply = st.alloc("apply", jfun, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(apply).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(ft)]],
        ret: Box::new(Type::TypeParam(fr)),
    };
}

fn add_annotation_pkg(st: &mut SymbolTable) {
    let pkg = st.alloc(
        "annotation",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/annotation",
    );
    let annotation = abs_class(
        st,
        pkg,
        "Annotation",
        "scala/annotation/Annotation",
        &[Type::AnyRef],
    );
    let static_annot = abs_class(
        st,
        pkg,
        "StaticAnnotation",
        "scala/annotation/StaticAnnotation",
        &[Type::Class {
            sym: annotation,
            args: vec![],
        }],
    );
    let _ = abs_class(
        st,
        pkg,
        "switch",
        "scala/annotation/switch",
        &[Type::Class {
            sym: static_annot,
            args: vec![],
        }],
    );
    let inf = class(
        st,
        pkg,
        "implicitNotFound",
        "scala/annotation/implicitNotFound",
        &[Type::Class {
            sym: static_annot,
            args: vec![],
        }],
    );
    let inf_msg = st.alloc("msg", inf, SymKind::Term, Flags::PARAM, "");
    st.get_mut(inf_msg).ty = Type::String;
    st.get_mut(inf).ctor_fields = vec![inf_msg];
    let unc_pkg = st.alloc(
        "unchecked",
        pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/annotation/unchecked",
    );
    let _ = abs_class(
        st,
        unc_pkg,
        "uncheckedVariance",
        "scala/annotation/unchecked/uncheckedVariance",
        &[Type::Class {
            sym: static_annot,
            args: vec![],
        }],
    );
    let static_t = Type::Class {
        sym: static_annot,
        args: vec![],
    };
    // nsc: `scala.inline` / `scala.noinline` / `scala.volatile` / `scala.transient`
    for (name, jvm) in [
        ("inline", "scala/inline"),
        ("noinline", "scala/noinline"),
        ("volatile", "scala/volatile"),
        ("transient", "scala/transient"),
        ("native", "scala/native"),
    ] {
        let _ = abs_class(st, st.scala_pkg, name, jvm, &[static_t.clone()]);
    }
}

fn class(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    jvm: &str,
    parents: &[Type],
) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Class, Flags::FINAL, jvm);
    st.get_mut(id).parents = parents.to_vec();
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    id
}

fn iface(st: &mut SymbolTable, owner: SymbolId, name: &str, jvm: &str) -> SymbolId {
    let id = st.alloc(
        name,
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

fn module(st: &mut SymbolTable, owner: SymbolId, name: &str, jvm: &str) -> SymbolId {
    let cls = st.alloc(
        &format!("{name}$"),
        owner,
        SymKind::ModuleClass,
        Flags::MODULE.with(Flags::FINAL),
        jvm,
    );
    let m = st.alloc(name, owner, SymKind::Module, Flags::MODULE, jvm);
    st.get_mut(m).ty = Type::ModuleRef(cls);
    st.get_mut(cls).ty = Type::ModuleRef(cls);
    m
}

fn module_extending(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    jvm: &str,
    parent: Type,
) -> SymbolId {
    let m = module(st, owner, name, jvm);
    let cls = st.module_class_of(m);
    st.get_mut(cls).parents = vec![parent];
    m
}

fn method(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    params: Vec<Type>,
    ret: Type,
    intrinsic: Intrinsic,
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
    st.get_mut(id).intrinsic = intrinsic;
    id
}

fn add_ordered(st: &mut SymbolTable) -> SymbolId {
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

fn type_param(st: &mut SymbolTable, owner: SymbolId, name: &str) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::TypeParam, Flags::EMPTY, "");
    st.get_mut(id).ty = Type::TypeParam(id);
    id
}

fn import_members(st: &mut SymbolTable, owner: SymbolId) {
    let members = st.get(owner).members.clone();
    for m in members {
        let name = st.get(m).name.clone();
        if name.ends_with('$') {
            continue;
        }
        st.enter_in_current(&name, m);
    }
}

fn add_any_members(st: &mut SymbolTable) {
    let any = st.any_sym;
    method(
        st,
        any,
        "==",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::AnyEq,
    );
    method(
        st,
        any,
        "!=",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::AnyNe,
    );
    method(
        st,
        any,
        "equals",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, any, "hashCode", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        any,
        "toString",
        vec![],
        Type::String,
        Intrinsic::AnyToString,
    );
    method(st, any, "asInstanceOf", vec![], Type::Any, Intrinsic::None);
    method(
        st,
        any,
        "isInstanceOf",
        vec![],
        Type::Boolean,
        Intrinsic::None,
    );
    // nsc `Any.synchronized[T0](body: => T0): T0`
    let sync = method(
        st,
        any,
        "synchronized",
        vec![Type::ByName(Box::new(Type::Any))],
        Type::Any,
        Intrinsic::Synchronized,
    );
    let t0 = type_param(st, sync, "T0");
    st.get_mut(sync).tparams = vec![t0];
    st.get_mut(sync).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::TypeParam(t0)))]],
        ret: Box::new(Type::TypeParam(t0)),
    };
    let anyref = st.anyref_sym;
    method(
        st,
        anyref,
        "eq",
        vec![Type::AnyRef],
        Type::Boolean,
        Intrinsic::Eq,
    );
    method(
        st,
        anyref,
        "ne",
        vec![Type::AnyRef],
        Type::Boolean,
        Intrinsic::Ne,
    );
}

fn add_int_members(st: &mut SymbolTable) {
    let c = st.int_sym;
    for (op, ic) in [
        ("+", Intrinsic::IntBin("+")),
        ("-", Intrinsic::IntBin("-")),
        ("*", Intrinsic::IntBin("*")),
        ("/", Intrinsic::IntBin("/")),
        ("%", Intrinsic::IntBin("%")),
        ("&", Intrinsic::IntBin("&")),
        ("|", Intrinsic::IntBin("|")),
        ("^", Intrinsic::IntBin("^")),
        ("<<", Intrinsic::IntBin("<<")),
        (">>", Intrinsic::IntBin(">>")),
        (">>>", Intrinsic::IntBin(">>>")),
    ] {
        method(st, c, op, vec![Type::Int], Type::Int, ic);
    }
    for (op, ic) in [
        ("==", Intrinsic::IntBin("==")),
        ("!=", Intrinsic::IntBin("!=")),
        ("<", Intrinsic::IntBin("<")),
        ("<=", Intrinsic::IntBin("<=")),
        (">", Intrinsic::IntBin(">")),
        (">=", Intrinsic::IntBin(">=")),
    ] {
        method(st, c, op, vec![Type::Int], Type::Boolean, ic);
    }
    method(st, c, "unary_-", vec![], Type::Int, Intrinsic::IntUn("-"));
    method(st, c, "unary_~", vec![], Type::Int, Intrinsic::IntUn("~"));
    method(st, c, "toLong", vec![], Type::Long, Intrinsic::IntToLong);
    method(
        st,
        c,
        "toDouble",
        vec![],
        Type::Double,
        Intrinsic::IntToDouble,
    );
    method(
        st,
        c,
        "toString",
        vec![],
        Type::String,
        Intrinsic::AnyToString,
    );
    method(
        st,
        c,
        "+",
        vec![Type::Long],
        Type::Long,
        Intrinsic::LongBin("+"),
    );
    method(
        st,
        c,
        "+",
        vec![Type::Double],
        Type::Double,
        Intrinsic::DoubleBin("+"),
    );
}

fn add_long_members(st: &mut SymbolTable) {
    let c = st.long_sym;
    for (op, ic) in [
        ("+", Intrinsic::LongBin("+")),
        ("-", Intrinsic::LongBin("-")),
        ("*", Intrinsic::LongBin("*")),
        ("/", Intrinsic::LongBin("/")),
        ("%", Intrinsic::LongBin("%")),
    ] {
        method(st, c, op, vec![Type::Long], Type::Long, ic);
    }
    for op in ["==", "!=", "<", "<=", ">", ">="] {
        method(
            st,
            c,
            op,
            vec![Type::Long],
            Type::Boolean,
            Intrinsic::LongBin(op),
        );
    }
    method(st, c, "unary_-", vec![], Type::Long, Intrinsic::LongUn("-"));
    method(st, c, "toInt", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        c,
        "toDouble",
        vec![],
        Type::Double,
        Intrinsic::LongToDouble,
    );
}

fn add_double_members(st: &mut SymbolTable) {
    let c = st.double_sym;
    for (op, ic) in [
        ("+", Intrinsic::DoubleBin("+")),
        ("-", Intrinsic::DoubleBin("-")),
        ("*", Intrinsic::DoubleBin("*")),
        ("/", Intrinsic::DoubleBin("/")),
        ("%", Intrinsic::DoubleBin("%")),
    ] {
        method(st, c, op, vec![Type::Double], Type::Double, ic);
    }
    for op in ["==", "!=", "<", "<=", ">", ">="] {
        method(
            st,
            c,
            op,
            vec![Type::Double],
            Type::Boolean,
            Intrinsic::DoubleBin(op),
        );
    }
    method(
        st,
        c,
        "unary_-",
        vec![],
        Type::Double,
        Intrinsic::DoubleUn("-"),
    );
}

fn add_float_members(st: &mut SymbolTable) {
    let c = st.float_sym;
    method(
        st,
        c,
        "unary_-",
        vec![],
        Type::Float,
        Intrinsic::FloatUn("-"),
    );
}

fn add_bool_members(st: &mut SymbolTable) {
    let c = st.boolean_sym;
    method(
        st,
        c,
        "&&",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("&&"),
    );
    method(
        st,
        c,
        "||",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("||"),
    );
    method(
        st,
        c,
        "unary_!",
        vec![],
        Type::Boolean,
        Intrinsic::BoolUn("!"),
    );
    method(
        st,
        c,
        "==",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("=="),
    );
    method(
        st,
        c,
        "!=",
        vec![Type::Boolean],
        Type::Boolean,
        Intrinsic::BoolBin("!="),
    );
}

fn add_string_members(st: &mut SymbolTable, library_abi: bool) {
    let c = st.string_sym;
    method(
        st,
        c,
        "+",
        vec![Type::Any],
        Type::String,
        Intrinsic::StringConcat,
    );
    method(
        st,
        c,
        "charAt",
        vec![Type::Int],
        Type::Char,
        Intrinsic::None,
    );
    method(
        st,
        c,
        "concat",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    if !library_abi {
        method(st, c, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    }
    method(
        st,
        c,
        "equals",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
    );
    method(st, c, "toString", vec![], Type::String, Intrinsic::Identity);
    if !library_abi {
        method(st, c, "length", vec![], Type::Int, Intrinsic::None);
        // Private runtime: parseInt on String. Library mode uses StringOps via augmentString.
        method(st, c, "toInt", vec![], Type::Int, Intrinsic::StringToInt);
        method(st, c, "toLong", vec![], Type::Long, Intrinsic::StringToLong);
        method(
            st,
            c,
            "toDouble",
            vec![],
            Type::Double,
            Intrinsic::StringToDouble,
        );
    }
}

fn add_array_members(st: &mut SymbolTable) {
    let c = st.array_sym;
    method(st, c, "length", vec![], Type::Int, Intrinsic::None);
    method(st, c, "apply", vec![Type::Int], Type::Any, Intrinsic::None);
    method(
        st,
        c,
        "update",
        vec![Type::Int, Type::Any],
        Type::Unit,
        Intrinsic::None,
    );
}

fn fn1(arg: Type, ret: Type) -> Type {
    Type::Function {
        params: vec![arg],
        ret: Box::new(ret),
    }
}

fn add_with_filter(st: &mut SymbolTable) -> SymbolId {
    let wf = class(
        st,
        st.scala_pkg,
        "WithFilter",
        "scala/collection/WithFilter",
        &[Type::AnyRef],
    );
    let a = type_param(st, wf, "A");
    let cc = type_param(st, wf, "CC");
    st.get_mut(wf).tparams = vec![a, cc];
    let ta = Type::TypeParam(a);
    let tcc = Type::TypeParam(cc);
    method(
        st,
        wf,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        tcc.clone(),
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "flatMap",
        vec![fn1(ta.clone(), tcc.clone())],
        tcc.clone(),
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "withFilter",
        vec![fn1(ta, Type::Boolean)],
        Type::Class {
            sym: wf,
            args: vec![Type::TypeParam(a), Type::TypeParam(cc)],
        },
        Intrinsic::None,
    );
    wf
}

fn add_option_with_filter(st: &mut SymbolTable) -> SymbolId {
    let wf = class(
        st,
        st.scala_pkg,
        "Option$WithFilter",
        "scala/Option$WithFilter",
        &[Type::AnyRef],
    );
    let a = type_param(st, wf, "A");
    st.get_mut(wf).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let opt = Type::Class {
        sym: st.option_sym,
        args: vec![ta.clone()],
    };
    method(
        st,
        wf,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        opt.clone(),
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "flatMap",
        vec![fn1(ta.clone(), opt.clone())],
        opt.clone(),
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        wf,
        "withFilter",
        vec![fn1(ta, Type::Boolean)],
        Type::Class {
            sym: wf,
            args: vec![Type::TypeParam(a)],
        },
        Intrinsic::None,
    );
    wf
}

fn add_iterator(st: &mut SymbolTable) -> SymbolId {
    let it = iface(st, st.scala_pkg, "Iterator", "scala/collection/Iterator");
    let a = type_param(st, it, "A");
    st.get_mut(it).tparams = vec![a];
    let ta = Type::TypeParam(a);
    let it_t = Type::Class {
        sym: it,
        args: vec![ta.clone()],
    };
    method(st, it, "hasNext", vec![], Type::Boolean, Intrinsic::None);
    method(st, it, "next", vec![], ta.clone(), Intrinsic::None);
    method(
        st,
        it,
        "foreach",
        vec![fn1(ta.clone(), Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        it,
        "map",
        vec![fn1(ta.clone(), Type::Any)],
        it_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        it,
        "filter",
        vec![fn1(ta.clone(), Type::Boolean)],
        it_t.clone(),
        Intrinsic::None,
    );
    method(
        st,
        it,
        "withFilter",
        vec![fn1(ta, Type::Boolean)],
        it_t,
        Intrinsic::None,
    );
    it
}

fn add_string_ops(st: &mut SymbolTable) -> SymbolId {
    let so = class(
        st,
        st.scala_pkg,
        "StringOps",
        "scala/collection/StringOps",
        &[Type::AnyVal],
    );
    let f = st.alloc("repr", so, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = Type::String;
    st.get_mut(so).ctor_fields = vec![f];
    method(st, so, "toInt", vec![], Type::Int, Intrinsic::None);
    method(st, so, "toLong", vec![], Type::Long, Intrinsic::None);
    method(st, so, "toDouble", vec![], Type::Double, Intrinsic::None);
    method(st, so, "length", vec![], Type::Int, Intrinsic::None);
    method(st, so, "size", vec![], Type::Int, Intrinsic::None);
    method(st, so, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, so, "*", vec![Type::Int], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "take",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "drop",
        vec![Type::Int],
        Type::String,
        Intrinsic::None,
    );
    method(st, so, "toUpperCase", vec![], Type::String, Intrinsic::None);
    method(st, so, "toLowerCase", vec![], Type::String, Intrinsic::None);
    method(
        st,
        so,
        "stripPrefix",
        vec![Type::String],
        Type::String,
        Intrinsic::None,
    );
    method(
        st,
        so,
        "split",
        vec![Type::Char],
        Type::Array(Box::new(Type::String)),
        Intrinsic::None,
    );
    so
}

fn add_option_members(st: &mut SymbolTable, option_wf: SymbolId, library_abi: bool) {
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
    method(
        st,
        some,
        "<init>",
        vec![Type::Any],
        Type::Class {
            sym: some,
            args: vec![],
        },
        Intrinsic::None,
    );
    st.get_mut(some).ctor_fields = {
        let f = st.alloc("value", some, SymKind::Term, Flags::PARAM, "");
        st.get_mut(f).ty = Type::Any;
        vec![f]
    };
}

fn add_list_members(
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
            args: vec![ta.clone(), list_t.clone()],
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

fn add_function_types(st: &mut SymbolTable) {
    for n in 0..=2 {
        let f = iface(
            st,
            st.scala_pkg,
            &format!("Function{n}"),
            &format!("scala/Function{n}"),
        );
        let params = vec![Type::Any; n];
        method(st, f, "apply", params, Type::Any, Intrinsic::None);
    }
}

fn add_partial_function(st: &mut SymbolTable) {
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

fn add_list_collect(st: &mut SymbolTable) {
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

fn add_map_and_vector(st: &mut SymbolTable) {
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

fn add_set(st: &mut SymbolTable) {
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

fn add_seq_and_lazylist(st: &mut SymbolTable) {
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
}

fn add_either(st: &mut SymbolTable) {
    let either = class(
        st,
        st.scala_pkg,
        "Either",
        "scala/util/Either",
        &[Type::AnyRef],
    );
    let ea = type_param(st, either, "A");
    let eb = type_param(st, either, "B");
    st.get_mut(either).tparams = vec![ea, eb];
    let tb = Type::TypeParam(eb);
    let either_t = Type::Class {
        sym: either,
        args: vec![Type::TypeParam(ea), tb.clone()],
    };
    method(st, either, "isLeft", vec![], Type::Boolean, Intrinsic::None);
    method(
        st,
        either,
        "getOrElse",
        vec![Type::ByName(Box::new(Type::Any))],
        Type::Any,
        Intrinsic::None,
    );
    method(
        st,
        either,
        "map",
        vec![fn1(tb, Type::Any)],
        either_t.clone(),
        Intrinsic::None,
    );

    // nsc: `class Left[+A, +B](value: A) extends Either[A, B]`
    let left = class(
        st,
        st.scala_pkg,
        "Left",
        "scala/util/Left",
        &[either_t.clone()],
    );
    let la = type_param(st, left, "A");
    let lb = type_param(st, left, "B");
    st.get_mut(left).tparams = vec![la, lb];
    st.get_mut(left).parents = vec![Type::Class {
        sym: either,
        args: vec![Type::TypeParam(la), Type::TypeParam(lb)],
    }];
    let lf = st.alloc("value", left, SymKind::Term, Flags::FINAL, "");
    st.get_mut(lf).ty = Type::TypeParam(la);
    st.get_mut(left).ctor_fields = vec![lf];
    let left_mod = module(st, st.scala_pkg, "Left", "scala/util/Left$");
    let left_cls = st.module_class_of(left_mod);
    let left_apply = method(
        st,
        left_cls,
        "apply",
        vec![Type::Any],
        Type::Class {
            sym: left,
            args: vec![Type::TypeParam(la), Type::TypeParam(lb)],
        },
        Intrinsic::None,
    );
    st.get_mut(left_apply).tparams = vec![la, lb];
    let mems = st.get(left_cls).members.clone();
    st.get_mut(left_mod).members.extend(mems);

    // nsc: `class Right[+A, +B](value: B) extends Either[A, B]`
    let right = class(st, st.scala_pkg, "Right", "scala/util/Right", &[either_t]);
    let ra = type_param(st, right, "A");
    let rb = type_param(st, right, "B");
    st.get_mut(right).tparams = vec![ra, rb];
    st.get_mut(right).parents = vec![Type::Class {
        sym: either,
        args: vec![Type::TypeParam(ra), Type::TypeParam(rb)],
    }];
    let rf = st.alloc("value", right, SymKind::Term, Flags::FINAL, "");
    st.get_mut(rf).ty = Type::TypeParam(rb);
    st.get_mut(right).ctor_fields = vec![rf];
    let right_mod = module(st, st.scala_pkg, "Right", "scala/util/Right$");
    let right_cls = st.module_class_of(right_mod);
    let right_apply = method(
        st,
        right_cls,
        "apply",
        vec![Type::Any],
        Type::Class {
            sym: right,
            args: vec![Type::TypeParam(ra), Type::TypeParam(rb)],
        },
        Intrinsic::None,
    );
    st.get_mut(right_apply).tparams = vec![ra, rb];
    let mems = st.get(right_cls).members.clone();
    st.get_mut(right_mod).members.extend(mems);
}

fn add_try(st: &mut SymbolTable, throwable: SymbolId) {
    let try_c = class(st, st.scala_pkg, "Try", "scala/util/Try", &[Type::AnyRef]);
    let tt = type_param(st, try_c, "T");
    st.get_mut(try_c).tparams = vec![tt];
    let t_ty = Type::TypeParam(tt);
    let try_t = Type::Class {
        sym: try_c,
        args: vec![t_ty.clone()],
    };
    method(
        st,
        try_c,
        "getOrElse",
        vec![Type::ByName(Box::new(Type::Any))],
        Type::Any,
        Intrinsic::None,
    );
    method(
        st,
        try_c,
        "map",
        vec![fn1(t_ty, Type::Any)],
        try_t.clone(),
        Intrinsic::None,
    );

    let try_mod = module(st, st.scala_pkg, "Try", "scala/util/Try$");
    let try_cls = st.module_class_of(try_mod);
    method(
        st,
        try_cls,
        "apply",
        vec![Type::ByName(Box::new(Type::Any))],
        try_t.clone(),
        Intrinsic::None,
    );
    let mems = st.get(try_cls).members.clone();
    st.get_mut(try_mod).members.extend(mems);

    let success = class(
        st,
        st.scala_pkg,
        "Success",
        "scala/util/Success",
        &[try_t.clone()],
    );
    let sa = type_param(st, success, "T");
    st.get_mut(success).tparams = vec![sa];
    let sf = st.alloc("value", success, SymKind::Term, Flags::FINAL, "");
    st.get_mut(sf).ty = Type::TypeParam(sa);
    st.get_mut(success).ctor_fields = vec![sf];
    let success_mod = module(st, st.scala_pkg, "Success", "scala/util/Success$");
    let success_cls = st.module_class_of(success_mod);
    method(
        st,
        success_cls,
        "apply",
        vec![Type::Any],
        Type::Class {
            sym: success,
            args: vec![],
        },
        Intrinsic::None,
    );
    let mems = st.get(success_cls).members.clone();
    st.get_mut(success_mod).members.extend(mems);

    let throwable_ty = Type::Class {
        sym: throwable,
        args: vec![],
    };
    let failure = class(st, st.scala_pkg, "Failure", "scala/util/Failure", &[try_t]);
    let fa = type_param(st, failure, "T");
    st.get_mut(failure).tparams = vec![fa];
    let ff = st.alloc("exception", failure, SymKind::Term, Flags::FINAL, "");
    st.get_mut(ff).ty = throwable_ty.clone();
    st.get_mut(failure).ctor_fields = vec![ff];
    let failure_mod = module(st, st.scala_pkg, "Failure", "scala/util/Failure$");
    let failure_cls = st.module_class_of(failure_mod);
    method(
        st,
        failure_cls,
        "apply",
        vec![throwable_ty],
        Type::Class {
            sym: failure,
            args: vec![],
        },
        Intrinsic::None,
    );
    let mems = st.get(failure_cls).members.clone();
    st.get_mut(failure_mod).members.extend(mems);
}

fn add_rich_int_and_range(st: &mut SymbolTable) -> SymbolId {
    let range = class(
        st,
        st.scala_pkg,
        "Range",
        "scala/collection/immutable/Range",
        &[Type::AnyRef],
    );
    method(st, range, "length", vec![], Type::Int, Intrinsic::None);
    method(
        st,
        range,
        "apply",
        vec![Type::Int],
        Type::Int,
        Intrinsic::None,
    );
    method(
        st,
        range,
        "foreach",
        vec![fn1(Type::Int, Type::Unit)],
        Type::Unit,
        Intrinsic::None,
    );
    method(st, range, "toString", vec![], Type::String, Intrinsic::None);
    let ri = class(
        st,
        st.scala_pkg,
        "RichInt",
        "scala/runtime/RichInt",
        &[Type::AnyVal],
    );
    let f = st.alloc("self", ri, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = Type::Int;
    st.get_mut(ri).ctor_fields = vec![f];
    method(st, ri, "abs", vec![], Type::Int, Intrinsic::None);
    method(st, ri, "max", vec![Type::Int], Type::Int, Intrinsic::None);
    method(st, ri, "min", vec![Type::Int], Type::Int, Intrinsic::None);
    let range_t = Type::Class {
        sym: range,
        args: vec![],
    };
    method(
        st,
        ri,
        "to",
        vec![Type::Int],
        range_t.clone(),
        Intrinsic::None,
    );
    method(st, ri, "until", vec![Type::Int], range_t, Intrinsic::None);
    ri
}

fn add_rich_value(st: &mut SymbolTable, name: &str, jvm: &str, under: Type) -> SymbolId {
    let c = class(st, st.scala_pkg, name, jvm, &[Type::AnyVal]);
    let f = st.alloc("self", c, SymKind::Term, Flags::PARAM, "");
    st.get_mut(f).ty = under.clone();
    st.get_mut(c).ctor_fields = vec![f];
    c
}

fn add_rich_long_double_char(st: &mut SymbolTable) -> (SymbolId, SymbolId, SymbolId) {
    let rl = add_rich_value(st, "RichLong", "scala/runtime/RichLong", Type::Long);
    method(st, rl, "abs", vec![], Type::Long, Intrinsic::None);
    method(st, rl, "max", vec![Type::Long], Type::Long, Intrinsic::None);
    method(st, rl, "min", vec![Type::Long], Type::Long, Intrinsic::None);
    let rd = add_rich_value(st, "RichDouble", "scala/runtime/RichDouble", Type::Double);
    method(st, rd, "abs", vec![], Type::Double, Intrinsic::None);
    method(
        st,
        rd,
        "max",
        vec![Type::Double],
        Type::Double,
        Intrinsic::None,
    );
    method(
        st,
        rd,
        "min",
        vec![Type::Double],
        Type::Double,
        Intrinsic::None,
    );
    let rc = add_rich_value(st, "RichChar", "scala/runtime/RichChar", Type::Char);
    method(st, rc, "isDigit", vec![], Type::Boolean, Intrinsic::None);
    method(st, rc, "toInt", vec![], Type::Int, Intrinsic::None);
    (rl, rd, rc)
}

fn add_rich_float(st: &mut SymbolTable) -> SymbolId {
    let rf = add_rich_value(st, "RichFloat", "scala/runtime/RichFloat", Type::Float);
    method(st, rf, "abs", vec![], Type::Float, Intrinsic::None);
    method(
        st,
        rf,
        "max",
        vec![Type::Float],
        Type::Float,
        Intrinsic::None,
    );
    method(
        st,
        rf,
        "min",
        vec![Type::Float],
        Type::Float,
        Intrinsic::None,
    );
    rf
}

fn add_predef_members(
    st: &mut SymbolTable,
    arrow: SymbolId,
    string_ops: Option<SymbolId>,
    rich_int: Option<SymbolId>,
    rich_ldc: Option<(SymbolId, SymbolId, SymbolId)>,
    library_abi: bool,
) {
    let p = st.predef;
    let cls = st.get(p).ty.clone();
    let owner = match cls {
        Type::ModuleRef(id) => id,
        _ => p,
    };
    method(st, owner, "println", vec![], Type::Unit, Intrinsic::Println);
    method(
        st,
        owner,
        "println",
        vec![Type::Int],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Long],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Double],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Boolean],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::String],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "println",
        vec![Type::Any],
        Type::Unit,
        Intrinsic::Println,
    );
    method(
        st,
        owner,
        "print",
        vec![Type::Any],
        Type::Unit,
        Intrinsic::Print,
    );
    method(
        st,
        owner,
        "assert",
        vec![Type::Boolean],
        Type::Unit,
        Intrinsic::Assert,
    );
    method(
        st,
        owner,
        "assert",
        vec![Type::Boolean, Type::ByName(Box::new(Type::Any))],
        Type::Unit,
        Intrinsic::Assert,
    );
    method(
        st,
        owner,
        "require",
        vec![Type::Boolean],
        Type::Unit,
        Intrinsic::Require,
    );
    method(
        st,
        owner,
        "require",
        vec![Type::Boolean, Type::ByName(Box::new(Type::Any))],
        Type::Unit,
        Intrinsic::Require,
    );
    method(
        st,
        owner,
        "???",
        vec![],
        Type::Nothing,
        Intrinsic::NotImplemented,
    );
    let ident = method(
        st,
        owner,
        "identity",
        vec![Type::Any],
        Type::Any,
        Intrinsic::Identity,
    );
    let ia = type_param(st, ident, "A");
    st.get_mut(ident).tparams = vec![ia];
    st.get_mut(ident).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(ia)]],
        ret: Box::new(Type::TypeParam(ia)),
    };
    let loc = method(
        st,
        owner,
        "locally",
        vec![Type::Any],
        Type::Any,
        Intrinsic::Locally,
    );
    let lt = type_param(st, loc, "A");
    st.get_mut(loc).tparams = vec![lt];
    st.get_mut(loc).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(lt)]],
        ret: Box::new(Type::TypeParam(lt)),
    };
    let implm = method(
        st,
        owner,
        "implicitly",
        vec![Type::Any],
        Type::Any,
        Intrinsic::Implicitly,
    );
    let it = type_param(st, implm, "T");
    let ip = st.alloc(
        "e",
        implm,
        crate::symbol::SymKind::Term,
        Flags::PARAM.with(Flags::IMPLICIT),
        "",
    );
    st.get_mut(ip).ty = Type::TypeParam(it);
    st.get_mut(implm).tparams = vec![it];
    st.get_mut(implm).params = vec![ip];
    st.get_mut(implm).paramss = vec![vec![ip]];
    st.get_mut(implm).ty = Type::Method {
        paramss: vec![vec![Type::TypeParam(it)]],
        ret: Box::new(Type::TypeParam(it)),
    };
    let sadd = if library_abi {
        let s = class(
            st,
            st.scala_pkg,
            "any2stringadd",
            "scala/Predef$any2stringadd",
            &[Type::AnyVal],
        );
        let f = st.alloc("self", s, SymKind::Term, Flags::PARAM, "");
        st.get_mut(f).ty = Type::Any;
        st.get_mut(s).ctor_fields = vec![f];
        method(
            st,
            s,
            "+",
            vec![Type::String],
            Type::String,
            Intrinsic::None,
        );
        s
    } else {
        let s = class(
            st,
            st.scala_pkg,
            "any2stringadd",
            "scala/runtime/StringAdd",
            &[Type::AnyRef],
        );
        method(
            st,
            s,
            "+",
            vec![Type::String],
            Type::String,
            Intrinsic::StringConcat,
        );
        s
    };
    let conv_s = method(
        st,
        owner,
        "any2stringadd",
        vec![Type::Any],
        Type::Class {
            sym: sadd,
            args: vec![],
        },
        if library_abi {
            Intrinsic::Identity
        } else {
            Intrinsic::Any2StringAdd
        },
    );
    st.get_mut(conv_s).flags = st.get(conv_s).flags.with(Flags::IMPLICIT);
    let conv = method(
        st,
        owner,
        "any2ArrowAssoc",
        vec![Type::Any],
        Type::Class {
            sym: arrow,
            args: vec![],
        },
        Intrinsic::WrapArrowAssoc,
    );
    st.get_mut(conv).flags = st.get(conv).flags.with(Flags::IMPLICIT);
    if let Some(sops) = string_ops {
        let aug = method(
            st,
            owner,
            "augmentString",
            vec![Type::String],
            Type::Class {
                sym: sops,
                args: vec![],
            },
            Intrinsic::Identity,
        );
        st.get_mut(aug).flags = st.get(aug).flags.with(Flags::IMPLICIT);
    }
    if let Some(ri) = rich_int {
        let wrap = method(
            st,
            owner,
            "intWrapper",
            vec![Type::Int],
            Type::Class {
                sym: ri,
                args: vec![],
            },
            Intrinsic::Identity,
        );
        st.get_mut(wrap).flags = st.get(wrap).flags.with(Flags::IMPLICIT);
    }
    if let Some((rl, rd, rc)) = rich_ldc {
        add_numeric_wrapper(st, owner, "longWrapper", Type::Long, rl);
        add_numeric_wrapper(st, owner, "doubleWrapper", Type::Double, rd);
        add_numeric_wrapper(st, owner, "charWrapper", Type::Char, rc);
    }
    if library_abi {
        let rf = add_rich_float(st);
        add_numeric_wrapper(st, owner, "floatWrapper", Type::Float, rf);
    }
    let mems = st.get(owner).members.clone();
    st.get_mut(p).members.extend(mems.iter().copied());
    for m in mems {
        let name = st.get(m).name.clone();
        st.enter_in_current(&name, m);
    }
}

fn add_numeric_wrapper(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    from: Type,
    cls: SymbolId,
) {
    let wrap = method(
        st,
        owner,
        name,
        vec![from],
        Type::Class {
            sym: cls,
            args: vec![],
        },
        Intrinsic::Identity,
    );
    st.get_mut(wrap).flags = st.get(wrap).flags.with(Flags::IMPLICIT);
}

fn implicit_getter(st: &mut SymbolTable, owner: SymbolId, name: &str, ty: Type) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Method, Flags::IMPLICIT, "");
    st.get_mut(id).ty = Type::Method {
        paramss: vec![],
        ret: Box::new(ty),
    };
    id
}

fn add_classtag(st: &mut SymbolTable, jclass: SymbolId) -> SymbolId {
    let reflect = st.alloc(
        "reflect",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/reflect",
    );
    let ct = iface(st, reflect, "ClassTag", "scala/reflect/ClassTag");
    let t = type_param(st, ct, "T");
    st.get_mut(ct).tparams = vec![t];
    let class_ty = Type::Class {
        sym: jclass,
        args: vec![],
    };
    method(st, ct, "runtimeClass", vec![], class_ty, Intrinsic::None);
    method(
        st,
        ct,
        "newArray",
        vec![Type::Int],
        Type::Array(Box::new(Type::TypeParam(t))),
        Intrinsic::None,
    );
    let ctm = module(st, reflect, "ClassTag", "scala/reflect/ClassTag$");
    let mc = st.module_class_of(ctm);
    let tag = |elem: Type| Type::Class {
        sym: ct,
        args: vec![elem],
    };
    implicit_getter(st, mc, "Int", tag(Type::Int));
    implicit_getter(st, mc, "Long", tag(Type::Long));
    implicit_getter(st, mc, "Double", tag(Type::Double));
    implicit_getter(st, mc, "Float", tag(Type::Float));
    implicit_getter(st, mc, "Boolean", tag(Type::Boolean));
    implicit_getter(st, mc, "Char", tag(Type::Char));
    implicit_getter(st, mc, "Unit", tag(Type::Unit));
    implicit_getter(st, mc, "Any", tag(Type::Any));
    implicit_getter(st, mc, "AnyRef", tag(Type::AnyRef));
    implicit_getter(st, mc, "Object", tag(Type::AnyRef));
    implicit_getter(st, mc, "Nothing", tag(Type::Nothing));
    implicit_getter(st, mc, "Null", tag(Type::Null));
    let mems = st.get(mc).members.clone();
    st.get_mut(ctm).members.extend(mems);
    ct
}

fn add_string_context(st: &mut SymbolTable) {
    let sc = class(
        st,
        st.scala_pkg,
        "StringContext",
        "scala/StringContext",
        &[Type::AnyRef],
    );
    let parts = st.alloc("parts", sc, SymKind::Term, Flags::PARAM, "");
    st.get_mut(parts).ty = Type::Repeated(Box::new(Type::String));
    st.get_mut(sc).ctor_fields = vec![parts];
    method(
        st,
        sc,
        "s",
        vec![Type::Repeated(Box::new(Type::Any))],
        Type::String,
        Intrinsic::None,
    );
    let scm = module(st, st.scala_pkg, "StringContext", "scala/StringContext$");
    let mc = st.module_class_of(scm);
    method(
        st,
        mc,
        "apply",
        vec![Type::Repeated(Box::new(Type::String))],
        Type::Class {
            sym: sc,
            args: vec![],
        },
        Intrinsic::None,
    );
    let mems = st.get(mc).members.clone();
    st.get_mut(scm).members.extend(mems);
}

/// `scala.Array` companion from scala-library. Do not emit `Array$.class`.
fn add_array_companion(st: &mut SymbolTable, ct: SymbolId) {
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

fn ctor_field(st: &mut SymbolTable, owner: SymbolId, name: &str, ty: Type) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Term, Flags::PARAM, "");
    st.get_mut(id).ty = ty;
    id
}

fn abs_class(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    jvm: &str,
    parents: &[Type],
) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Class, Flags::ABSTRACT, jvm);
    st.get_mut(id).parents = parents.to_vec();
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    id
}

/// scala-xml 2.3 (`Elem(String, String, MetaData, NamespaceBinding, boolean, Seq[Node])`).
fn add_xml(st: &mut SymbolTable) {
    let xml = st.alloc(
        "xml",
        st.scala_pkg,
        SymKind::Package,
        Flags::PACKAGE,
        "scala/xml",
    );
    let node = abs_class(st, xml, "Node", "scala/xml/Node", &[Type::AnyRef]);
    let node_t = Type::Class {
        sym: node,
        args: vec![],
    };
    let metadata = abs_class(st, xml, "MetaData", "scala/xml/MetaData", &[Type::AnyRef]);
    let nsb = abs_class(
        st,
        xml,
        "NamespaceBinding",
        "scala/xml/NamespaceBinding",
        &[Type::AnyRef],
    );
    let _null = module_extending(
        st,
        xml,
        "Null",
        "scala/xml/Null$",
        Type::Class {
            sym: metadata,
            args: vec![],
        },
    );
    let _top = module_extending(
        st,
        xml,
        "TopScope",
        "scala/xml/TopScope$",
        Type::Class {
            sym: nsb,
            args: vec![],
        },
    );
    let seq = st
        .get(st.scala_pkg)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).name == "Seq" && st.get(m).kind == SymKind::Class)
        .expect("Seq");
    let seq_node = Type::Class {
        sym: seq,
        args: vec![node_t.clone()],
    };
    let elem = class(st, xml, "Elem", "scala/xml/Elem", &[node_t.clone()]);
    let p_prefix = ctor_field(st, elem, "prefix", Type::String);
    let p_label = ctor_field(st, elem, "label", Type::String);
    let p_attr = ctor_field(
        st,
        elem,
        "attributes",
        Type::Class {
            sym: metadata,
            args: vec![],
        },
    );
    let p_scope = ctor_field(
        st,
        elem,
        "scope",
        Type::Class {
            sym: nsb,
            args: vec![],
        },
    );
    let p_min = ctor_field(st, elem, "minimizeEmpty", Type::Boolean);
    let p_child = ctor_field(st, elem, "child", seq_node);
    st.get_mut(elem).ctor_fields = vec![p_prefix, p_label, p_attr, p_scope, p_min, p_child];
    let text = class(st, xml, "Text", "scala/xml/Text", &[node_t.clone()]);
    let td = ctor_field(st, text, "data", Type::String);
    st.get_mut(text).ctor_fields = vec![td];
    let eref = class(
        st,
        xml,
        "EntityRef",
        "scala/xml/EntityRef",
        &[node_t.clone()],
    );
    let en = ctor_field(st, eref, "entityName", Type::String);
    st.get_mut(eref).ctor_fields = vec![en];
    let comment = class(st, xml, "Comment", "scala/xml/Comment", &[node_t.clone()]);
    let ct = ctor_field(st, comment, "commentText", Type::String);
    st.get_mut(comment).ctor_fields = vec![ct];
    let pcdata = class(st, xml, "PCData", "scala/xml/PCData", &[node_t.clone()]);
    let pd = ctor_field(st, pcdata, "data", Type::String);
    st.get_mut(pcdata).ctor_fields = vec![pd];
    let pi = class(
        st,
        xml,
        "ProcInstr",
        "scala/xml/ProcInstr",
        &[node_t.clone()],
    );
    let pit = ctor_field(st, pi, "target", Type::String);
    let pip = ctor_field(st, pi, "proctext", Type::String);
    st.get_mut(pi).ctor_fields = vec![pit, pip];
    let atom = class(st, xml, "Atom", "scala/xml/Atom", &[node_t]);
    let ad = ctor_field(st, atom, "data", Type::Any);
    st.get_mut(atom).ctor_fields = vec![ad];
    let meta_t = Type::Class {
        sym: metadata,
        args: vec![],
    };
    let upa = class(
        st,
        xml,
        "UnprefixedAttribute",
        "scala/xml/UnprefixedAttribute",
        &[meta_t.clone()],
    );
    let uk = ctor_field(st, upa, "key", Type::String);
    let uv = ctor_field(st, upa, "value", Type::String);
    let un = ctor_field(st, upa, "next", meta_t.clone());
    st.get_mut(upa).ctor_fields = vec![uk, uv, un];
    let nsb_t = Type::Class {
        sym: nsb,
        args: vec![],
    };
    let np = ctor_field(st, nsb, "prefix", Type::String);
    let nu = ctor_field(st, nsb, "uri", Type::String);
    let npar = ctor_field(st, nsb, "parent", nsb_t);
    st.get_mut(nsb).ctor_fields = vec![np, nu, npar];
    let pa = class(
        st,
        xml,
        "PrefixedAttribute",
        "scala/xml/PrefixedAttribute",
        &[meta_t.clone()],
    );
    let pp = ctor_field(st, pa, "pre", Type::String);
    let pk = ctor_field(st, pa, "key", Type::String);
    let pv = ctor_field(st, pa, "value", Type::String);
    let pn = ctor_field(st, pa, "next", meta_t);
    st.get_mut(pa).ctor_fields = vec![pp, pk, pv, pn];
}

/// `scala.Enumeration` plus inner `Value` (`Color.Red.toString` / `.id` against the jar).
fn add_enumeration(st: &mut SymbolTable) {
    let en = abs_class(
        st,
        st.scala_pkg,
        "Enumeration",
        "scala/Enumeration",
        &[Type::AnyRef],
    );
    let val = abs_class(st, en, "Value", "scala/Enumeration$Value", &[Type::AnyRef]);
    method(st, val, "id", vec![], Type::Int, Intrinsic::None);
    let val_t = Type::Class {
        sym: val,
        args: vec![],
    };
    method(st, en, "Value", vec![], val_t, Intrinsic::None);
}

/// `scala.DelayedInit` / `scala.App` (nsc delayed constructor body).
fn add_delayed_init_app(st: &mut SymbolTable) {
    let di = iface(st, st.scala_pkg, "DelayedInit", "scala/DelayedInit");
    let d = st.alloc("delayedInit", di, SymKind::Method, Flags::ABSTRACT, "");
    st.get_mut(d).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::Unit))]],
        ret: Box::new(Type::Unit),
    };
    let p = st.alloc("x", d, SymKind::Term, Flags::PARAM.with(Flags::BYNAME), "");
    st.get_mut(p).ty = Type::ByName(Box::new(Type::Unit));
    st.get_mut(d).params = vec![p];
    st.get_mut(d).paramss = vec![vec![p]];

    let app = iface(st, st.scala_pkg, "App", "scala/App");
    st.get_mut(app).parents = vec![
        Type::Class {
            sym: di,
            args: vec![],
        },
        Type::AnyRef,
    ];
    let d2 = st.alloc("delayedInit", app, SymKind::Method, Flags::EMPTY, "");
    st.get_mut(d2).ty = Type::Method {
        paramss: vec![vec![Type::ByName(Box::new(Type::Unit))]],
        ret: Box::new(Type::Unit),
    };
    let p2 = st.alloc("x", d2, SymKind::Term, Flags::PARAM.with(Flags::BYNAME), "");
    st.get_mut(p2).ty = Type::ByName(Box::new(Type::Unit));
    st.get_mut(d2).params = vec![p2];
    st.get_mut(d2).paramss = vec![vec![p2]];

    let main = st.alloc("main", app, SymKind::Method, Flags::EMPTY, "");
    let args_ty = Type::Array(Box::new(Type::String));
    st.get_mut(main).ty = Type::Method {
        paramss: vec![vec![args_ty.clone()]],
        ret: Box::new(Type::Unit),
    };
    let ap = st.alloc("args", main, SymKind::Term, Flags::PARAM, "");
    st.get_mut(ap).ty = args_ty;
    st.get_mut(main).params = vec![ap];
    st.get_mut(main).paramss = vec![vec![ap]];
}
