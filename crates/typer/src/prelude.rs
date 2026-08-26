use crate::symbol::{Intrinsic, SymbolTable, SymKind};
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

    st.unit_sym = class(st, st.scala_pkg, "Unit", "scala/runtime/BoxedUnit", &[Type::AnyVal]);
    st.boolean_sym = class(st, st.scala_pkg, "Boolean", "java/lang/Boolean", &[Type::AnyVal]);
    st.int_sym = class(st, st.scala_pkg, "Int", "java/lang/Integer", &[Type::AnyVal]);
    st.long_sym = class(st, st.scala_pkg, "Long", "java/lang/Long", &[Type::AnyVal]);
    let float = class(st, st.scala_pkg, "Float", "java/lang/Float", &[Type::AnyVal]);
    st.double_sym = class(st, st.scala_pkg, "Double", "java/lang/Double", &[Type::AnyVal]);
    let char_s = class(st, st.scala_pkg, "Char", "java/lang/Character", &[Type::AnyVal]);
    let _ = (float, char_s);

    st.string_sym = class(st, java_lang, "String", "java/lang/String", &[Type::AnyRef]);
    // alias in scala and Predef
    st.array_sym = class(st, st.scala_pkg, "Array", "[java/lang/Object", &[Type::AnyRef]);
    st.option_sym = class(st, st.scala_pkg, "Option", "scala/Option", &[Type::AnyRef]);
    st.some_sym = class(st, st.scala_pkg, "Some", "scala/Some", &[Type::Class {
        sym: st.option_sym,
        args: vec![],
    }]);
    st.none_sym = module(st, st.scala_pkg, "None", "scala/None$");
    st.list_sym = class(st, st.scala_pkg, "List", "scala/collection/immutable/List", &[Type::AnyRef]);
    st.nil_sym = module(st, st.scala_pkg, "Nil", "scala/collection/immutable/Nil$");
    st.cons_sym = class(st, st.scala_pkg, "$colon$colon", "scala/collection/immutable/$colon$colon", &[Type::Class {
        sym: st.list_sym,
        args: vec![],
    }]);
    // `::` is an alias users write
    let cons_alias = st.alloc("::", st.scala_pkg, SymKind::Class, Flags::CASE, "scala/collection/immutable/$colon$colon");
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

    // Predef
    st.predef = module(st, st.scala_pkg, "Predef", "scala/Predef$");
    add_predef_members(st);

    // FunctionN / TupleN names
    for n in 0..=2 {
        let _ = class(
            st,
            st.scala_pkg,
            &format!("Function{n}"),
            &format!("scala/Function{n}"),
            &[Type::AnyRef],
        );
    }
    for n in 2..=3 {
        let _ = class(
            st,
            st.scala_pkg,
            &format!("Tuple{n}"),
            &format!("scala/Tuple{n}"),
            &[Type::AnyRef],
        );
    }

    // Auto-import into the outermost scope: scala._, java.lang._, Predef._
    st.push_scope();
    import_members(st, st.scala_pkg);
    import_members(st, java_lang);
    import_members(st, st.predef);
    // String alias
    st.enter_in_current("String", st.string_sym);
    st.enter_in_current("Unit", st.unit_sym);
    st.enter_in_current("::", st.cons_sym);
}

fn class(st: &mut SymbolTable, owner: SymbolId, name: &str, jvm: &str, parents: &[Type]) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Class, Flags::FINAL, jvm);
    st.get_mut(id).parents = parents.to_vec();
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

fn method(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    params: Vec<Type>,
    ret: Type,
    intrinsic: Intrinsic,
) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Method, Flags::FINAL, "");
    st.get_mut(id).ty = Type::Method {
        paramss: vec![params],
        ret: Box::new(ret),
    };
    st.get_mut(id).intrinsic = intrinsic;
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
    method(st, any, "==", vec![Type::Any], Type::Boolean, Intrinsic::None);
    method(st, any, "!=", vec![Type::Any], Type::Boolean, Intrinsic::None);
    method(st, any, "equals", vec![Type::Any], Type::Boolean, Intrinsic::None);
    method(st, any, "hashCode", vec![], Type::Int, Intrinsic::None);
    method(st, any, "toString", vec![], Type::String, Intrinsic::AnyToString);
    method(st, any, "asInstanceOf", vec![], Type::Any, Intrinsic::None);
    method(st, any, "isInstanceOf", vec![], Type::Boolean, Intrinsic::None);
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
    method(st, c, "toDouble", vec![], Type::Double, Intrinsic::IntToDouble);
    method(st, c, "toString", vec![], Type::String, Intrinsic::AnyToString);
    // Long / Double overloads of + etc. for widening at the member
    method(st, c, "+", vec![Type::Long], Type::Long, Intrinsic::LongBin("+"));
    method(st, c, "+", vec![Type::Double], Type::Double, Intrinsic::DoubleBin("+"));
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
        method(st, c, op, vec![Type::Long], Type::Boolean, Intrinsic::LongBin(op));
    }
    method(st, c, "unary_-", vec![], Type::Long, Intrinsic::LongUn("-"));
    method(st, c, "toInt", vec![], Type::Int, Intrinsic::None);
    method(st, c, "toDouble", vec![], Type::Double, Intrinsic::LongToDouble);
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
        method(st, c, op, vec![Type::Double], Type::Boolean, Intrinsic::DoubleBin(op));
    }
    method(st, c, "unary_-", vec![], Type::Double, Intrinsic::DoubleUn("-"));
}

fn add_bool_members(st: &mut SymbolTable) {
    let c = st.boolean_sym;
    method(st, c, "&&", vec![Type::Boolean], Type::Boolean, Intrinsic::BoolBin("&&"));
    method(st, c, "||", vec![Type::Boolean], Type::Boolean, Intrinsic::BoolBin("||"));
    method(st, c, "unary_!", vec![], Type::Boolean, Intrinsic::BoolUn("!"));
    method(st, c, "==", vec![Type::Boolean], Type::Boolean, Intrinsic::BoolBin("=="));
    method(st, c, "!=", vec![Type::Boolean], Type::Boolean, Intrinsic::BoolBin("!="));
}

fn add_string_members(st: &mut SymbolTable) {
    let c = st.string_sym;
    method(st, c, "+", vec![Type::Any], Type::String, Intrinsic::StringConcat);
    method(st, c, "length", vec![], Type::Int, Intrinsic::None);
    method(st, c, "charAt", vec![Type::Int], Type::Char, Intrinsic::None);
    method(st, c, "concat", vec![Type::String], Type::String, Intrinsic::None);
    method(st, c, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, c, "equals", vec![Type::Any], Type::Boolean, Intrinsic::None);
    method(st, c, "toString", vec![], Type::String, Intrinsic::Identity);
}

fn add_array_members(st: &mut SymbolTable) {
    let c = st.array_sym;
    method(st, c, "length", vec![], Type::Int, Intrinsic::None);
    method(st, c, "apply", vec![Type::Int], Type::Any, Intrinsic::None);
    method(st, c, "update", vec![Type::Int, Type::Any], Type::Unit, Intrinsic::None);
}

fn add_option_members(st: &mut SymbolTable) {
    let o = st.option_sym;
    method(st, o, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, o, "get", vec![], Type::Any, Intrinsic::None);
    let some = st.some_sym;
    // apply
    method(st, some, "<init>", vec![Type::Any], Type::Class { sym: some, args: vec![] }, Intrinsic::None);
}

fn add_list_members(st: &mut SymbolTable) {
    let l = st.list_sym;
    let list_t = Type::Class {
        sym: l,
        args: vec![],
    };
    method(st, l, "isEmpty", vec![], Type::Boolean, Intrinsic::None);
    method(st, l, "head", vec![], Type::Any, Intrinsic::None);
    method(st, l, "tail", vec![], list_t.clone(), Intrinsic::None);
    method(
        st,
        l,
        "foreach",
        vec![Type::Function {
            params: vec![Type::Any],
            ret: Box::new(Type::Unit),
        }],
        Type::Unit,
        Intrinsic::None,
    );
    method(
        st,
        l,
        "map",
        vec![Type::Function {
            params: vec![Type::Any],
            ret: Box::new(Type::Any),
        }],
        list_t,
        Intrinsic::None,
    );
}

fn add_predef_members(st: &mut SymbolTable) {
    let p = st.predef;
    // Predef is a module; members go on its module class
    let cls = st
        .get(p)
        .ty
        .clone();
    let owner = match cls {
        Type::ModuleRef(id) => id,
        _ => p,
    };
    method(st, owner, "println", vec![], Type::Unit, Intrinsic::Println);
    method(st, owner, "println", vec![Type::Int], Type::Unit, Intrinsic::Println);
    method(st, owner, "println", vec![Type::Long], Type::Unit, Intrinsic::Println);
    method(st, owner, "println", vec![Type::Double], Type::Unit, Intrinsic::Println);
    method(st, owner, "println", vec![Type::Boolean], Type::Unit, Intrinsic::Println);
    method(st, owner, "println", vec![Type::String], Type::Unit, Intrinsic::Println);
    method(st, owner, "println", vec![Type::Any], Type::Unit, Intrinsic::Println);
    method(st, owner, "print", vec![Type::Any], Type::Unit, Intrinsic::Print);
    // also enter on current scope via import_members of Predef module: members of module value
    // Copy member ids onto the module symbol for lookup by Predef.println
    let mems = st.get(owner).members.clone();
    st.get_mut(p).members.extend(mems.iter().copied());
    for m in mems {
        let name = st.get(m).name.clone();
        st.enter_in_current(&name, m);
    }
}
