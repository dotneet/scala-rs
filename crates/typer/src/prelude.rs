use crate::symbol::{Intrinsic, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

pub fn install_prelude(st: &mut SymbolTable) {
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
    add_string_members(st);
    add_array_members(st);
    add_option_members(st);
    add_list_members(st);
    add_function_types(st);

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

    let arrow = class(
        st,
        st.scala_pkg,
        "ArrowAssoc",
        "scala/runtime/ArrowAssoc",
        &[Type::AnyRef],
    );
    let af = st.alloc("self", arrow, SymKind::Term, Flags::PARAM, "");
    st.get_mut(af).ty = Type::Any;
    st.get_mut(arrow).ctor_fields = vec![af];
    method(
        st,
        arrow,
        "->",
        vec![Type::Any],
        Type::Class {
            sym: tuple2,
            args: vec![Type::Any, Type::Any],
        },
        Intrinsic::None,
    );

    st.predef = module(st, st.scala_pkg, "Predef", "scala/Predef$");
    add_predef_members(st, arrow);

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

fn add_string_members(st: &mut SymbolTable) {
    let c = st.string_sym;
    method(
        st,
        c,
        "+",
        vec![Type::Any],
        Type::String,
        Intrinsic::StringConcat,
    );
    method(st, c, "length", vec![], Type::Int, Intrinsic::None);
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

fn add_option_members(st: &mut SymbolTable) {
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
        opt,
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

fn add_list_members(st: &mut SymbolTable) {
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
    method(
        st,
        l,
        "withFilter",
        vec![fn1(ta, Type::Boolean)],
        list_t,
        Intrinsic::None,
    );
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

fn add_predef_members(st: &mut SymbolTable, arrow: SymbolId) {
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
    let mems = st.get(owner).members.clone();
    st.get_mut(p).members.extend(mems.iter().copied());
    for m in mems {
        let name = st.get(m).name.clone();
        st.enter_in_current(&name, m);
    }
}
