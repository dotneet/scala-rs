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
    let float = class(
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
    let _ = (float, char_s);

    st.string_sym = class(st, java_lang, "String", "java/lang/String", &[Type::AnyRef]);
    let throwable = class(
        st,
        java_lang,
        "Throwable",
        "java/lang/Throwable",
        &[Type::AnyRef],
    );
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
    if library_abi {
        add_map_and_vector(st);
    }

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
    add_predef_members(st, arrow, string_ops, library_abi);

    st.push_scope();
    import_members(st, st.scala_pkg);
    import_members(st, java_lang);
    import_members(st, st.predef);
    st.enter_in_current("String", st.string_sym);
    st.enter_in_current("Unit", st.unit_sym);
    st.enter_in_current("::", st.cons_sym);
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
        Intrinsic::None,
    );
    method(
        st,
        any,
        "!=",
        vec![Type::Any],
        Type::Boolean,
        Intrinsic::None,
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
    method(st, c, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
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
            args: vec![list_t],
        },
        Intrinsic::None,
    );
    let _ = seq;
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
        vec![fn1(pair, Type::Unit)],
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
    let mems = st.get(vec_cls).members.clone();
    st.get_mut(vec_mod).members.extend(mems);
}

fn add_predef_members(
    st: &mut SymbolTable,
    arrow: SymbolId,
    string_ops: Option<SymbolId>,
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
    let mems = st.get(owner).members.clone();
    st.get_mut(p).members.extend(mems.iter().copied());
    for m in mems {
        let name = st.get(m).name.clone();
        st.enter_in_current(&name, m);
    }
}
