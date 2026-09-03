//! Install symbols recovered from classpath classfiles / ScalaSignature pickles.

use scala_rs_parser::{Flags, SymbolId, Type};

use crate::check::{ClasspathClass, ClasspathType, ClasspathTypeParam};
use crate::symbol::{SymKind, SymbolTable};

pub fn install_classpath(st: &mut SymbolTable, classes: &[ClasspathClass]) {
    let mut installed: Vec<(usize, SymbolId)> = Vec::new();
    // Nested classfiles (`enrich/package$Rich`) must see their outer module
    // (`enrich/package$`) first, otherwise they land as `package$Rich` on the
    // package and `import enrich._` cannot convert via `Rich`.
    let mut order: Vec<usize> = (0..classes.len()).collect();
    order.sort_by_key(|&i| nest_depth(&classes[i].jvm_name));
    for i in order {
        let c = &classes[i];
        if c.jvm_name.contains("$anon") || c.jvm_name.ends_with("$class") {
            continue;
        }
        if is_forwarder_of_module(classes, c) {
            continue;
        }
        // Pure Java classfiles have no ScalaSignature. Installing them here
        // (root owner, no JAVA/PROTECTED/STATIC) shadows on-demand completion
        // via `install_java_class` and drops JLS flags. The Java loader on
        // `binary_path` completes them instead.
        if c.pickle.is_none() {
            continue;
        }
        let (owner, simple) = classpath_symbol_owner(st, &c.jvm_name);
        if simple.is_empty() {
            continue;
        }
        if c.is_module {
            let jvm = if c.jvm_name.ends_with('$') {
                c.jvm_name.clone()
            } else {
                format!("{}$", c.jvm_name)
            };
            let existing = st
                .lookup_member(owner, &simple)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::Module)
                .or_else(|| {
                    if owner == st.root {
                        st.lookup(&simple)
                            .into_iter()
                            .find(|&s| st.get(s).kind == SymKind::Module)
                    } else {
                        None
                    }
                });
            if let Some(m) = existing {
                installed.push((i, st.module_class_of(m)));
                continue;
            }
            let cls = st.alloc(
                &format!("{simple}$"),
                owner,
                SymKind::ModuleClass,
                Flags::MODULE.with(Flags::FINAL),
                &jvm,
            );
            let m = st.alloc(&simple, owner, SymKind::Module, Flags::MODULE, &jvm);
            st.get_mut(m).ty = Type::ModuleRef(cls);
            st.get_mut(cls).ty = Type::ModuleRef(cls);
            if owner == st.root {
                st.enter_in_current(&simple, m);
            }
            installed.push((i, cls));
        } else {
            let existing = st
                .lookup_member(owner, &simple)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::Class)
                .or_else(|| {
                    if owner == st.root {
                        st.lookup(&simple)
                            .into_iter()
                            .find(|&s| st.get(s).kind == SymKind::Class)
                    } else {
                        None
                    }
                });
            if let Some(id) = existing {
                installed.push((i, id));
                continue;
            }
            // A Scala trait compiles to an interface. Without the flag the
            // backend emits `invokevirtual` against it and the JVM answers
            // with `IncompatibleClassChangeError` at the first call.
            let flags = if c.is_interface {
                Flags::INTERFACE
            } else {
                Flags::EMPTY
            };
            let id = st.alloc(&simple, owner, SymKind::Class, flags, &c.jvm_name);
            st.get_mut(id).ty = Type::Class {
                sym: id,
                args: vec![],
            };
            st.get_mut(id).parents = vec![Type::AnyRef];
            if owner == st.root {
                st.enter_in_current(&simple, id);
            }
            installed.push((i, id));
        }
    }

    for (i, owner) in &installed {
        let (i, owner) = (*i, *owner);
        let c = &classes[i];
        install_tparams(st, owner, &c.pickle_tparams);
        if let Some(p) = &c.pickle {
            for m in p {
                if has_member(st, owner, &m.name) {
                    continue;
                }
                if m.is_val {
                    let mut ty = resolve_type_in(st, owner, &m.ret, &[]);
                    // The pickle subset keeps member types as *simple* names,
                    // so a val whose type lives in another package comes back
                    // unresolved. The getter's descriptor in the class file
                    // names the same type in full, so fall back to it rather
                    // than install a member nothing can be selected from.
                    if matches!(ty, Type::Named { .. }) {
                        if let Some(g) = c
                            .methods
                            .iter()
                            .find(|g| g.name == m.name && g.desc.starts_with("()"))
                        {
                            let (_, ret) = parse_method_desc(st, &g.desc);
                            if !matches!(ret, Type::Named { .. }) {
                                ty = ret;
                            }
                        }
                    }
                    add_term(st, owner, &m.name, ty);
                    continue;
                }
                if m.is_ctor {
                    install_ctor(st, owner, m);
                    continue;
                }
                let id = add_method(
                    st,
                    owner,
                    &m.name,
                    m.param_names.clone(),
                    m.param_types.clone(),
                    m.ret.clone(),
                    m.tparams.clone(),
                );
                if m.is_implicit {
                    let f = st.get(id).flags.with(Flags::IMPLICIT);
                    st.get_mut(id).flags = f;
                }
            }
        }
        for m in &c.methods {
            if has_member(st, owner, &m.name) {
                continue;
            }
            if m.name == "<init>" && !st.get(owner).ctor_fields.is_empty() {
                continue;
            }
            let (params, ret) = parse_method_desc(st, &m.desc);
            let names: Vec<String> = (0..params.len()).map(|i| format!("x${i}")).collect();
            add_method_erased(st, owner, &m.name, names, params, ret);
        }
        mark_defaults_from_getters(st, owner);
        copy_package_object_members(st, owner, c);
    }

    attach_classpath_parents(st, classes, &installed);
}

/// Give each `-cp` class the parents its classfile header names.
///
/// The pickle subset records member types by *simple* name and drops the
/// inheritance graph entirely, so without this a class from `-cp` looks like it
/// extends nothing: `t.greet()` on a `trait T extends Base` is "not a member",
/// and a member that *is* found is attributed to the wrong owner. The classfile
/// header is where `super_class`/`interfaces` survive intact.
///
/// Run after every class is installed, so a parent that comes later in the scan
/// is already there, and after members are installed, so `has_member` keeps
/// deciding on a class's *own* declarations exactly as before.
///
/// Type arguments are not recoverable from the header (`extends Base[Int]` is
/// just `Base` there), so a parent is attached without them. That is why an
/// existing symbol whose parents are already known -- the prelude's, or the
/// Java loader's, both of which carry arguments -- is left alone.
fn attach_classpath_parents(
    st: &mut SymbolTable,
    classes: &[ClasspathClass],
    installed: &[(usize, SymbolId)],
) {
    let mut by_jvm: std::collections::HashMap<String, SymbolId> = std::collections::HashMap::new();
    for s in st.symbols.iter() {
        if s.is_class_like() && !s.jvm_name.is_empty() {
            by_jvm.entry(s.jvm_name.clone()).or_insert(s.id);
        }
    }
    for (i, owner) in installed {
        let c = &classes[*i];
        let owner = *owner;
        // Only when nothing better is known. `parents` is `[AnyRef]` for a
        // class this module just created and richer for one that was already
        // declared elsewhere.
        if st
            .get(owner)
            .parents
            .iter()
            .any(|p| !matches!(p, Type::AnyRef))
        {
            continue;
        }
        let mut ps = vec![Type::AnyRef];
        for n in c.super_name.iter().chain(c.interfaces.iter()) {
            if n == "java/lang/Object" {
                continue;
            }
            let Some(&pid) = by_jvm.get(n.as_str()) else {
                continue;
            };
            if pid == owner || pid == st.object_sym {
                continue;
            }
            let ty = Type::Class {
                sym: pid,
                args: vec![],
            };
            if !ps.iter().any(|p| same_class(p, &ty)) {
                ps.push(ty);
            }
        }
        if ps.len() > 1 {
            st.get_mut(owner).parents = ps;
        }
    }
}

fn has_member(st: &SymbolTable, owner: SymbolId, name: &str) -> bool {
    st.lookup_member(owner, name)
        .iter()
        .any(|&id| matches!(st.get(id).kind, SymKind::Method | SymKind::Term))
}

fn install_tparams(st: &mut SymbolTable, owner: SymbolId, tps: &[ClasspathTypeParam]) {
    if tps.is_empty() || !st.get(owner).tparams.is_empty() {
        return;
    }
    let ids = alloc_tparams(st, owner, tps);
    st.get_mut(owner).tparams = ids;
}

/// Allocate type parameter symbols, keeping each one's own parameters. Without
/// them a `F[_]` read from a classfile looked like a proper type and every
/// `Applicative[F]` failed the kind check.
fn alloc_tparams(
    st: &mut SymbolTable,
    owner: SymbolId,
    tps: &[ClasspathTypeParam],
) -> Vec<SymbolId> {
    let mut ids = Vec::new();
    for tp in tps {
        let id = st.alloc(&tp.name, owner, SymKind::TypeParam, Flags::EMPTY, "");
        st.get_mut(id).ty = Type::TypeParam(id);
        if !tp.tparams.is_empty() {
            let nested = alloc_tparams(st, id, &tp.tparams);
            st.get_mut(id).tparams = nested;
        }
        ids.push(id);
    }
    ids
}

fn install_java_tparams(st: &mut SymbolTable, owner: SymbolId, params: &[crate::javasign::JParam]) {
    if params.is_empty() || !st.get(owner).tparams.is_empty() {
        return;
    }
    // A JVM generic signature cannot express a higher kind, so these are all
    // proper types.
    let names: Vec<ClasspathTypeParam> = params
        .iter()
        .map(|p| ClasspathTypeParam::simple(p.name.clone()))
        .collect();
    install_tparams(st, owner, &names);
    let env = tparam_env(st, owner);
    let ids = st.get(owner).tparams.clone();
    for (p, id) in params.iter().zip(ids) {
        let bounds: Vec<Type> = p
            .bounds
            .iter()
            .map(|b| jtype_to_type(st, b, &env))
            .filter(|t| !matches!(t, Type::Any | Type::AnyRef))
            .collect();
        st.get_mut(id).parents = bounds;
    }
}

fn add_term(st: &mut SymbolTable, owner: SymbolId, name: &str, ty: Type) -> SymbolId {
    let id = st.alloc(name, owner, SymKind::Term, Flags::EMPTY, "");
    st.get_mut(id).ty = ty;
    id
}

fn install_ctor(st: &mut SymbolTable, owner: SymbolId, m: &crate::check::ClasspathPickleMethod) {
    let mut fields = Vec::new();
    for (i, (n, tn)) in m.param_names.iter().zip(m.param_types.iter()).enumerate() {
        let pname = if n.is_empty() {
            format!("x${i}")
        } else {
            n.clone()
        };
        let ty = resolve_type_in(st, owner, tn, &[]);
        let existing = st
            .lookup_member(owner, &pname)
            .into_iter()
            .find(|&id| st.get(id).kind == SymKind::Term);
        let fid = if let Some(id) = existing {
            st.get_mut(id).ty = ty;
            id
        } else {
            add_term(st, owner, &pname, ty)
        };
        fields.push(fid);
    }
    st.get_mut(owner).ctor_fields = fields.clone();
    add_method(
        st,
        owner,
        "<init>",
        m.param_names.clone(),
        m.param_types.clone(),
        ClasspathType::simple("Unit"),
        Vec::new(),
    );
}

fn resolve_type_in(
    st: &SymbolTable,
    owner: SymbolId,
    ty: &ClasspathType,
    method_tps: &[SymbolId],
) -> Type {
    let args: Vec<Type> = ty
        .args
        .iter()
        .map(|a| resolve_type_in(st, owner, a, method_tps))
        .collect();
    let name = ty.name.as_str();
    if name == "_" {
        return Type::Wildcard;
    }
    for id in method_tps.iter().chain(st.get(owner).tparams.iter()) {
        if st.get(*id).name == name {
            return apply_args(Type::TypeParam(*id), args);
        }
    }
    let mut cur = owner;
    let mut seen = std::collections::HashSet::new();
    while !cur.is_none() && seen.insert(cur.0) {
        if let Some(id) = st
            .lookup_member(cur, name)
            .into_iter()
            .find(|&s| st.get(s).is_class_like())
        {
            return match st.get(id).kind {
                SymKind::Module | SymKind::ModuleClass => apply_args(Type::ModuleRef(id), args),
                _ => Type::Class { sym: id, args },
            };
        }
        cur = st.get(cur).owner;
    }
    resolve_type_name_args(st, name, args)
}

/// Apply type arguments to a constructor that is not a class symbol. A bare
/// constructor stays as it is.
fn apply_args(ctor: Type, args: Vec<Type>) -> Type {
    if args.is_empty() {
        return ctor;
    }
    Type::Applied {
        ctor: Box::new(ctor),
        args,
    }
}

fn add_method(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    param_names: Vec<String>,
    param_type_names: Vec<ClasspathType>,
    ret_name: ClasspathType,
    tparams: Vec<ClasspathTypeParam>,
) -> SymbolId {
    let flags = if name.contains("$default$") {
        Flags::SYNTHETIC
    } else if name == "<init>" {
        Flags::CONSTRUCTOR
    } else {
        Flags::EMPTY
    };
    let id = st.alloc(name, owner, SymKind::Method, flags, "");
    let tp_ids = alloc_tparams(st, id, &tparams);
    st.get_mut(id).tparams = tp_ids.clone();
    let params: Vec<Type> = param_type_names
        .iter()
        .map(|n| resolve_type_in(st, owner, n, &tp_ids))
        .collect();
    let ret = resolve_type_in(st, owner, &ret_name, &tp_ids);
    let mut pids = Vec::new();
    for (i, (n, ty)) in param_names.iter().zip(params.iter()).enumerate() {
        let pname = if n.is_empty() {
            format!("x${i}")
        } else {
            n.clone()
        };
        let pid = st.alloc(&pname, id, SymKind::Term, Flags::PARAM, "");
        st.get_mut(pid).ty = ty.clone();
        pids.push(pid);
    }
    st.get_mut(id).params = pids.clone();
    st.get_mut(id).paramss = if pids.is_empty() { vec![] } else { vec![pids] };
    st.get_mut(id).ty = Type::Method {
        paramss: if params.is_empty() {
            vec![]
        } else {
            vec![params]
        },
        ret: Box::new(ret),
    };
    id
}

fn add_method_erased(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    param_names: Vec<String>,
    params: Vec<Type>,
    ret: Type,
) -> SymbolId {
    add_method_types(st, owner, name, param_names, params, ret)
}

fn is_forwarder_of_module(classes: &[ClasspathClass], c: &ClasspathClass) -> bool {
    if c.is_module {
        return false;
    }
    // A case class `Point.class` sits next to companion `Point$.class`. That is
    // not a static forwarder: the pickle describes a real class (ctor / vals).
    if c.pickle
        .as_ref()
        .is_some_and(|p| p.iter().any(|m| m.is_ctor || m.is_val))
    {
        return false;
    }
    let dollar = format!("{}$", c.jvm_name);
    classes
        .iter()
        .any(|o| o.is_module && (o.jvm_name == dollar || o.jvm_name == c.jvm_name))
}

fn simple_name(jvm: &str) -> String {
    let last = jvm.rsplit('/').next().unwrap_or(jvm);
    last.trim_end_matches('$').to_string()
}

fn nest_depth(jvm: &str) -> usize {
    let last = jvm.rsplit('/').next().unwrap_or(jvm);
    last.trim_end_matches('$')
        .bytes()
        .filter(|&b| b == b'$')
        .count()
}

fn classpath_nested_parent(st: &SymbolTable, jvm: &str) -> Option<SymbolId> {
    let last = jvm.rsplit('/').next().unwrap_or(jvm);
    let last_trim = last.trim_end_matches('$');
    let idx = last_trim.rfind('$')?;
    let outer_simple = &last_trim[..idx];
    let pkg = jvm.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    let module_jvm = if pkg.is_empty() {
        format!("{outer_simple}$")
    } else {
        format!("{pkg}/{outer_simple}$")
    };
    let class_jvm = if pkg.is_empty() {
        outer_simple.to_string()
    } else {
        format!("{pkg}/{outer_simple}")
    };
    find_by_jvm(st, &module_jvm).or_else(|| find_by_jvm(st, &class_jvm))
}

fn classpath_symbol_owner(st: &mut SymbolTable, jvm_name: &str) -> (SymbolId, String) {
    if nest_depth(jvm_name) > 0 {
        let simple = java_simple_name(jvm_name);
        if let Some(parent) = classpath_nested_parent(st, jvm_name) {
            return (parent, simple);
        }
        return (classpath_owner(st, jvm_name), simple);
    }
    (classpath_owner(st, jvm_name), simple_name(jvm_name))
}

fn classpath_owner(st: &mut SymbolTable, jvm_name: &str) -> SymbolId {
    let pkg = jvm_name.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    if pkg.is_empty() {
        st.root
    } else {
        ensure_package(st, pkg)
    }
}

fn copy_package_object_members(st: &mut SymbolTable, module_cls: SymbolId, c: &ClasspathClass) {
    if !c.is_module || simple_name(&c.jvm_name) != "package" {
        return;
    }
    let pkg = st.get(module_cls).owner;
    if pkg.is_none() {
        return;
    }
    let mems = st.get(module_cls).members.clone();
    for mem in mems {
        if !st.get(pkg).members.contains(&mem) {
            st.get_mut(pkg).members.push(mem);
        }
    }
}

fn add_method_types(
    st: &mut SymbolTable,
    owner: SymbolId,
    name: &str,
    param_names: Vec<String>,
    params: Vec<Type>,
    ret: Type,
) -> SymbolId {
    let flags = if name.contains("$default$") {
        Flags::SYNTHETIC
    } else {
        Flags::EMPTY
    };
    let id = st.alloc(name, owner, SymKind::Method, flags, "");
    let mut pids = Vec::new();
    for (i, (n, ty)) in param_names.iter().zip(params.iter()).enumerate() {
        let pname = if n.is_empty() {
            format!("x${i}")
        } else {
            n.clone()
        };
        let pid = st.alloc(&pname, id, SymKind::Term, Flags::PARAM, "");
        st.get_mut(pid).ty = ty.clone();
        pids.push(pid);
    }
    st.get_mut(id).params = pids.clone();
    st.get_mut(id).paramss = if pids.is_empty() { vec![] } else { vec![pids] };
    st.get_mut(id).ty = Type::Method {
        paramss: if params.is_empty() {
            vec![]
        } else {
            vec![params]
        },
        ret: Box::new(ret),
    };
    id
}

fn mark_defaults_from_getters(st: &mut SymbolTable, owner: SymbolId) {
    let members = st.get(owner).members.clone();
    let getters: Vec<(String, usize)> = members
        .iter()
        .filter_map(|&id| parse_default_getter(&st.get(id).name))
        .collect();
    for (meth, idx) in getters {
        let Some(mid) = st
            .lookup_member(owner, &meth)
            .into_iter()
            .find(|&id| st.get(id).kind == SymKind::Method)
        else {
            continue;
        };
        let params = st.get(mid).params.clone();
        if idx == 0 || idx > params.len() {
            continue;
        }
        let pid = params[idx - 1];
        let f = st.get(pid).flags.with(Flags::DEFAULTPARAM);
        st.get_mut(pid).flags = f;
    }
}

pub fn parse_default_getter(name: &str) -> Option<(String, usize)> {
    let i = name.rfind("$default$")?;
    let meth = &name[..i];
    let n: usize = name[i + "$default$".len()..].parse().ok()?;
    if meth.is_empty() || n == 0 {
        return None;
    }
    Some((meth.to_string(), n))
}

fn resolve_type_name(st: &SymbolTable, name: &str) -> Type {
    resolve_type_name_args(st, name, Vec::new())
}

fn resolve_type_name_args(st: &SymbolTable, name: &str, args: Vec<Type>) -> Type {
    // `FunctionN` / `TupleN` are structural in our `Type`, so they only become
    // themselves once their arguments are known.
    if let Some(n) = name.strip_prefix("Function") {
        if n.parse::<usize>().is_ok() && !args.is_empty() {
            let mut args = args;
            let ret = args.pop().unwrap_or(Type::Any);
            return Type::Function {
                params: args,
                ret: Box::new(ret),
            };
        }
    }
    if let Some(n) = name.strip_prefix("Tuple") {
        if n.parse::<usize>().is_ok() && args.len() > 1 {
            return Type::Tuple(args);
        }
    }
    if name == "Array" && args.len() == 1 {
        return Type::Array(Box::new(args.into_iter().next().unwrap_or(Type::Any)));
    }
    if name == "<byname>" && args.len() == 1 {
        return Type::ByName(Box::new(args.into_iter().next().unwrap_or(Type::Any)));
    }
    if name == "<repeated>" && args.len() == 1 {
        return Type::Repeated(Box::new(args.into_iter().next().unwrap_or(Type::Any)));
    }
    match resolve_bare_type_name(st, name) {
        Type::Class { sym, args: old } if old.is_empty() && !args.is_empty() => {
            Type::Class { sym, args }
        }
        Type::Named { name, args: old } if old.is_empty() && !args.is_empty() => {
            Type::Named { name, args }
        }
        t => t,
    }
}

fn resolve_bare_type_name(st: &SymbolTable, name: &str) -> Type {
    match name {
        "Unit" | "V" => Type::Unit,
        "Boolean" | "Z" => Type::Boolean,
        "Int" | "I" => Type::Int,
        "Long" | "J" => Type::Long,
        "Float" | "F" => Type::Float,
        "Double" | "D" => Type::Double,
        "Char" | "C" => Type::Char,
        "String" => Type::String,
        "Object" | "Any" | "AnyRef" => Type::Any,
        n if n.starts_with("Function") => Type::Function {
            params: vec![Type::Any],
            ret: Box::new(Type::Any),
        },
        n => {
            let found = st.lookup(n);
            if let Some(id) = found
                .iter()
                .copied()
                .find(|s| st.get(*s).kind == SymKind::Class)
            {
                Type::Class {
                    sym: id,
                    args: vec![],
                }
            } else if let Some(id) = found
                .iter()
                .copied()
                .find(|s| st.get(*s).is_class_like() || st.get(*s).kind == SymKind::Module)
            {
                match st.get(id).kind {
                    SymKind::Module | SymKind::ModuleClass => Type::ModuleRef(id),
                    _ => Type::Class {
                        sym: id,
                        args: vec![],
                    },
                }
            } else {
                Type::Named {
                    name: n.to_string(),
                    args: vec![],
                }
            }
        }
    }
}

pub fn parse_method_desc(st: &SymbolTable, desc: &str) -> (Vec<Type>, Type) {
    let rest = desc.strip_prefix('(').unwrap_or(desc);
    let (params_s, ret_s) = match rest.find(')') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => ("", rest),
    };
    let mut params = Vec::new();
    let mut s = params_s;
    while !s.is_empty() {
        let (t, n) = parse_field_ty(st, s);
        params.push(t);
        s = &s[n..];
        if n == 0 {
            break;
        }
    }
    let (ret, _) = parse_field_ty(st, ret_s);
    (params, ret)
}

fn parse_field_ty(st: &SymbolTable, s: &str) -> (Type, usize) {
    if s.is_empty() {
        return (Type::Any, 0);
    }
    match s.as_bytes()[0] {
        b'V' => (Type::Unit, 1),
        b'Z' => (Type::Boolean, 1),
        b'I' => (Type::Int, 1),
        b'J' => (Type::Long, 1),
        b'F' => (Type::Float, 1),
        b'D' => (Type::Double, 1),
        b'C' => (Type::Char, 1),
        b'B' => (Type::Byte, 1),
        b'S' => (Type::Short, 1),
        b'[' => {
            let (inner, n) = parse_field_ty(st, &s[1..]);
            (Type::Array(Box::new(inner)), n + 1)
        }
        b'L' => {
            let end = s.find(';').unwrap_or(s.len());
            let inner = &s[1..end];
            let name = inner.rsplit('/').next().unwrap_or(inner);
            let ty = if inner == "java/lang/String" || name == "String" {
                Type::String
            } else if inner == "java/lang/Object" {
                Type::Any
            } else if inner == "scala/runtime/BoxedUnit" {
                // `Unit` erases to `BoxedUnit` in every *value* position (a
                // parameter, a field, an array element), so reading a
                // descriptor back has to undo that -- otherwise a separately
                // compiled `case class K(k: Unit, n: Int)` came back as
                // `(BoxedUnit, Int)` and `K((), 2)` no longer type-checked
                // against our own classfile. nsc's own classfile reader makes
                // the same mapping.
                Type::Unit
            } else if inner == "scala/runtime/Nothing$" {
                Type::Nothing
            } else if name.starts_with("Function") {
                Type::Function {
                    params: vec![Type::Any],
                    ret: Box::new(Type::Any),
                }
            } else {
                let by_name = resolve_type_name(st, name);
                // A descriptor names one exact class. When the simple name is
                // not in scope -- `scala.reflect.api.JavaUniverse` is a member
                // of its package, not of any open scope -- resolving it by
                // internal name is still exact, and beats giving up on a
                // `Type::Named` that nothing can select a member from.
                match by_name {
                    Type::Named { .. } => match find_by_jvm(st, inner) {
                        Some(id) => Type::Class {
                            sym: id,
                            args: vec![],
                        },
                        None => by_name,
                    },
                    t => t,
                }
            };
            let consumed = if end < s.len() { end + 1 } else { end };
            (ty, consumed)
        }
        _ => (Type::Any, 1),
    }
}

pub fn ensure_package(st: &mut SymbolTable, jvm: &str) -> SymbolId {
    if jvm.is_empty() {
        return st.root;
    }
    let mut cur = st.root;
    let mut sofar = String::new();
    for part in jvm.split('/') {
        if !sofar.is_empty() {
            sofar.push('/');
        }
        sofar.push_str(part);
        if let Some(id) = st
            .lookup_member(cur, part)
            .into_iter()
            .find(|&s| st.get(s).kind == SymKind::Package)
        {
            cur = id;
            continue;
        }
        if cur == st.root {
            if let Some(id) = st
                .lookup(part)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::Package)
            {
                cur = id;
                continue;
            }
        }
        let id = st.alloc(part, cur, SymKind::Package, Flags::PACKAGE, &sofar);
        if cur == st.root {
            st.enter_in_current(part, id);
        }
        cur = id;
    }
    cur
}

pub fn install_java_class(st: &mut SymbolTable, c: &crate::javaclass::JavaClass) -> SymbolId {
    let owner = java_class_owner(st, &c.internal_name);
    install_java_class_in(st, c, owner)
}

pub fn install_java_class_in(
    st: &mut SymbolTable,
    c: &crate::javaclass::JavaClass,
    owner: SymbolId,
) -> SymbolId {
    let simple = java_simple_name(&c.internal_name);
    if simple.is_empty() {
        return find_or_stub_java_class(st, &c.internal_name);
    }
    if is_scala_module(c) {
        return install_java_module(st, c, owner);
    }
    if let Some(id) = st
        .lookup_member(owner, &simple)
        .into_iter()
        .find(|&s| st.get(s).kind == SymKind::Class)
    {
        apply_java_class_meta(st, id, c);
        fill_java_members(st, id, c);
        return id;
    }
    if let Some(id) = find_by_jvm(st, &c.internal_name) {
        apply_java_class_meta(st, id, c);
        fill_java_members(st, id, c);
        enter_in_companion_scope(st, id, owner, &c.internal_name);
        return id;
    }
    let flags = java_class_flags(c);
    let id = st.alloc(&simple, owner, SymKind::Class, flags, &c.internal_name);
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    if owner == st.root {
        st.enter_in_current(&simple, id);
    }
    apply_java_class_meta(st, id, c);
    fill_java_members(st, id, c);
    id
}

/// A nested class file `Outer$Inner` does not say whether `Inner` was declared
/// by `class Outer` or by `object Outer`, and [`java_class_owner`] always
/// answers the *class*. Whichever spelling reaches the class file first is
/// therefore the only one that can see it: with
/// `cats/effect/kernel/Resource$ExitCase` entered while reading some other
/// jar class's member descriptors (`fs2/Stream` mentions it), the owner is the
/// trait `Resource`, and the source's `Resource.ExitCase` — a path through the
/// `Resource` **object** — looked `ExitCase` up on `Resource$`, found nothing,
/// and reported "type ExitCase is not a member of Resource$". Compiling
/// `slick/basic/BasicBackend.scala` on its own got the other order and worked,
/// which is what made the failure look like it needed the whole program.
///
/// So when the owner that *asked* is the companion module class of the owner
/// that has it, enter the same symbol in its scope too. No second symbol is
/// created and no owner is rewritten: both spellings simply reach the one
/// class there is.
fn enter_in_companion_scope(st: &mut SymbolTable, id: SymbolId, owner: SymbolId, internal: &str) {
    if owner.is_none() || st.get(owner).kind != SymKind::ModuleClass {
        return;
    }
    let held_by = st.get(id).owner;
    if held_by == owner || held_by.is_none() || st.get(held_by).kind != SymKind::Class {
        return;
    }
    let module_jvm = st.get(owner).jvm_name.clone();
    let Some(outer) = module_jvm.strip_suffix('$') else {
        return;
    };
    if outer.is_empty()
        || st.get(held_by).jvm_name != outer
        || !internal.starts_with(&format!("{outer}$"))
    {
        return;
    }
    if st.get(owner).members.contains(&id) {
        return;
    }
    st.get_mut(owner).members.push(id);
}

fn java_class_flags(c: &crate::javaclass::JavaClass) -> Flags {
    let mut flags = if c.is_scala {
        Flags::EMPTY
    } else {
        Flags::JAVA
    };
    if crate::javaclass::is_java_interface(c.access) {
        flags = flags.with(Flags::INTERFACE).with(Flags::ABSTRACT);
    }
    if crate::javaclass::is_java_enum(c.access) {
        flags = flags.with(Flags::ENUM);
    }
    if c.nested_static {
        flags = flags.with(Flags::STATIC);
    }
    flags
}

fn is_scala_module(c: &crate::javaclass::JavaClass) -> bool {
    c.internal_name.ends_with('$') && (c.has_module_field || c.is_scala)
}

fn install_java_module(
    st: &mut SymbolTable,
    c: &crate::javaclass::JavaClass,
    owner: SymbolId,
) -> SymbolId {
    let simple = java_simple_name(&c.internal_name);
    if let Some(m) = st
        .lookup_member(owner, &simple)
        .into_iter()
        .find(|&s| st.get(s).kind == SymKind::Module)
    {
        let cls = st.module_class_of(m);
        apply_java_class_meta(st, cls, c);
        fill_java_members(st, cls, c);
        return cls;
    }
    if let Some(id) = find_by_jvm(st, &c.internal_name) {
        apply_java_class_meta(st, id, c);
        fill_java_members(st, id, c);
        return id;
    }
    let flags = Flags::MODULE.with(Flags::FINAL);
    let cls = st.alloc(
        &format!("{simple}$"),
        owner,
        SymKind::ModuleClass,
        flags,
        &c.internal_name,
    );
    let m = st.alloc(
        &simple,
        owner,
        SymKind::Module,
        Flags::MODULE,
        &c.internal_name,
    );
    st.get_mut(m).ty = Type::ModuleRef(cls);
    st.get_mut(cls).ty = Type::ModuleRef(cls);
    if owner == st.root {
        st.enter_in_current(&simple, m);
    }
    apply_java_class_meta(st, cls, c);
    fill_java_members(st, cls, c);
    cls
}

pub fn java_simple_name(internal: &str) -> String {
    let mut simple = internal.rsplit('/').next().unwrap_or(internal);
    if simple.ends_with('$') && simple.len() > 1 {
        simple = &simple[..simple.len() - 1];
    }
    if let Some(idx) = simple.rfind('$') {
        simple = &simple[idx + 1..];
    }
    simple.to_string()
}

fn java_class_owner(st: &mut SymbolTable, internal: &str) -> SymbolId {
    let trimmed = internal.trim_end_matches('$');
    if let Some((outer, _)) = trimmed.rsplit_once('$') {
        return find_or_stub_java_class(st, outer);
    }
    let pkg = trimmed.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    ensure_package(st, pkg)
}

/// The symbol *for* a JVM class.
///
/// The primitive value classes are deliberately excluded even though they
/// carry a `java/lang/...` `jvm_name`: that name is the box `scala.Int` erases
/// to, not `scala.Int`'s identity. Returning `scala.Int` here made
/// `install_java_class_in` treat the real `java.lang.Integer` classfile as
/// "already installed", pour its members into `scala.Int` and never enter
/// `Integer` into `java.lang` — so `java.lang.Integer.valueOf(3)` failed with
/// "value Integer is not a member of <notype>". nsc keeps the two apart, and
/// so do we.
pub fn find_by_jvm(st: &SymbolTable, jvm: &str) -> Option<SymbolId> {
    st.symbols.iter().find_map(|s| {
        if s.jvm_name == jvm && s.is_class_like() && !st.is_primitive_value_class(s.id) {
            Some(s.id)
        } else {
            None
        }
    })
}

pub fn find_or_stub_java_class(st: &mut SymbolTable, internal: &str) -> SymbolId {
    if let Some(id) = find_by_jvm(st, internal) {
        return id;
    }
    let simple = java_simple_name(internal);
    let owner = java_class_owner(st, internal);
    // `cats/effect/kernel/Ref$` is the *companion*, not the trait. Stubbing it
    // as a `SymKind::Class` called `Ref` -- with the companion's name in
    // `jvm_name` -- made one symbol stand for two things: the object's members
    // landed on the trait, and the trait could never get a symbol of its own
    // (`ensure_class` declines a name whose `jvm_name` is not the key it
    // asked for), so `Ref#update` was read from the class file's generic
    // signature. That cannot write `F[Unit]`; it writes `TF;`, and every
    // `ctx.update(…) >> …` became "value >> is not a member of F". A `$` name
    // gets the shape `install_java_module` builds for a class file it has
    // really read: a `ModuleClass` plus its `Module`.
    let module = internal.len() > 1 && internal.ends_with('$');
    // The owner's *own* declarations only. A nested class is always declared
    // by the class its JVM name names, never inherited into it, and searching
    // the parents too made every one of cats' `Foo.Ops` traits resolve to the
    // first one entered: `cats/FlatMap$Ops` asked `FlatMap` for `Ops`, whose
    // linearization reaches `Functor`, which by then had one.
    if module {
        if let Some(m) = st
            .get(owner)
            .members
            .iter()
            .copied()
            .find(|&s| st.get(s).name == simple && st.get(s).kind == SymKind::Module)
        {
            return st.module_class_of(m);
        }
    } else if let Some(id) = st
        .get(owner)
        .members
        .iter()
        .copied()
        .find(|&s| st.get(s).name == simple && st.get(s).kind == SymKind::Class)
    {
        return id;
    }
    if module {
        let flags = Flags::JAVA.with(Flags::MODULE).with(Flags::FINAL);
        let cls = st.alloc(
            format!("{simple}$"),
            owner,
            SymKind::ModuleClass,
            flags,
            internal,
        );
        let m = st.alloc(&simple, owner, SymKind::Module, Flags::MODULE, internal);
        st.get_mut(m).ty = Type::ModuleRef(cls);
        st.get_mut(cls).ty = Type::ModuleRef(cls);
        st.get_mut(cls).parents = vec![Type::AnyRef];
        if owner == st.root {
            st.enter_in_current(&simple, m);
        }
        return cls;
    }
    let id = st.alloc(&simple, owner, SymKind::Class, Flags::JAVA, internal);
    st.get_mut(id).ty = Type::Class {
        sym: id,
        args: vec![],
    };
    st.get_mut(id).parents = vec![Type::AnyRef];
    if owner == st.root {
        st.enter_in_current(&simple, id);
    }
    id
}

fn apply_java_class_meta(st: &mut SymbolTable, id: SymbolId, c: &crate::javaclass::JavaClass) {
    let mut flags = st.get(id).flags.with(java_class_flags(c));
    if st.get(id).kind == SymKind::ModuleClass {
        flags = flags.with(Flags::MODULE).with(Flags::FINAL);
    }
    st.get_mut(id).flags = flags;
    st.get_mut(id).jvm_name = c.internal_name.clone();
    if st.get(id).tparams.is_empty() {
        if let Some(sig) = &c.signature {
            if let Some(cs) = crate::javasign::parse_class_sig(sig) {
                install_java_tparams(st, id, &cs.tparams);
            }
        }
    }
    st.get_mut(id).parents = java_parents(st, id, c);
}

fn java_parents(
    st: &mut SymbolTable,
    class_id: SymbolId,
    c: &crate::javaclass::JavaClass,
) -> Vec<Type> {
    let env = tparam_env(st, class_id);
    if let Some(sig) = &c.signature {
        if let Some(cs) = crate::javasign::parse_class_sig(sig) {
            let mut ps = vec![Type::AnyRef];
            for sup in &cs.supers {
                let ty = jtype_to_type(st, sup, &env);
                if matches!(&ty, Type::Any | Type::AnyRef) {
                    continue;
                }
                if let Type::Class { sym, .. } = &ty {
                    if *sym == st.object_sym {
                        continue;
                    }
                }
                if !ps.iter().any(|p| same_class(p, &ty)) {
                    ps.push(ty);
                }
            }
            return ps;
        }
    }
    let mut ps = vec![Type::AnyRef];
    if let Some(sup) = &c.super_name {
        if sup != "java/lang/Object" {
            let ty = Type::Class {
                sym: find_or_stub_java_class(st, sup),
                args: vec![],
            };
            if !ps.iter().any(|p| same_class(p, &ty)) {
                ps.push(ty);
            }
        }
    }
    for iface in &c.interfaces {
        let ty = Type::Class {
            sym: find_or_stub_java_class(st, iface),
            args: vec![],
        };
        if !ps.iter().any(|p| same_class(p, &ty)) {
            ps.push(ty);
        }
    }
    ps
}

fn same_class(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Class { sym: x, .. }, Type::Class { sym: y, .. }) => x == y,
        _ => false,
    }
}

fn tparam_env(st: &SymbolTable, owner: SymbolId) -> std::collections::HashMap<String, SymbolId> {
    let mut env = std::collections::HashMap::new();
    for id in &st.get(owner).tparams {
        env.insert(st.get(*id).name.clone(), *id);
    }
    env
}

fn java_method_flags(m: &crate::javaclass::JavaMethod) -> Flags {
    let mut flags = Flags::JAVA;
    if crate::javaclass::is_java_static(m.access) {
        flags = flags.with(Flags::STATIC);
    }
    if crate::javaclass::is_java_abstract(m.access) {
        flags = flags.with(Flags::ABSTRACT);
    }
    if crate::javaclass::is_java_varargs(m.access) {
        flags = flags.with(Flags::VARARGS);
    }
    if crate::javaclass::is_java_protected(m.access) {
        flags = flags.with(Flags::PROTECTED);
    }
    if m.name == "<init>" {
        flags = flags.with(Flags::CONSTRUCTOR);
    }
    // scala.jdk.CollectionConverters implicit classes compile to 1-arg
    // `ListHasAsScala` / `SeqHasAsJava` methods. Mark them so extension
    // search can apply the real jar converters (no fake Buffer/List).
    if m.name.contains("HasAsScala") || m.name.contains("HasAsJava") {
        flags = flags.with(Flags::IMPLICIT);
    }
    flags
}

fn existing_java_method(
    st: &SymbolTable,
    owner: SymbolId,
    m: &crate::javaclass::JavaMethod,
) -> Option<SymbolId> {
    if let Some(id) = st.lookup_member(owner, &m.name).into_iter().find(|&id| {
        let s = st.get(id);
        s.kind == SymKind::Method && s.owner == owner && s.jvm_name == m.desc
    }) {
        return Some(id);
    }
    let arity = desc_param_count(&m.desc);
    st.lookup_member(owner, &m.name).into_iter().find(|&id| {
        let s = st.get(id);
        s.kind == SymKind::Method
            && s.owner == owner
            && s.jvm_name.is_empty()
            && method_arity(s) == arity
    })
}

/// A mixin forwarder or bridge in a *generic Scala* class file: a method with
/// no `Signature` attribute, where the erased descriptor is all there is.
///
/// scalac writes a `Signature` for every method whose Scala type mentions a
/// type parameter, so on a class that *has* type parameters an unsigned method
/// is a forwarder or bridge for a declaration that lives, properly typed,
/// somewhere the pickle describes. `scala.collection.immutable.HashMap` carries
/// `public Object filter(Function1)` and `public IterableOps map(Function1)`
/// -- forwarders for the `filter`/`map` its `MapOps` parent declares -- and
/// reading those as `(Any) => Any` / `(Any) => IterableOps` is what made
/// `foundRefs.filter(_._2._2.isEmpty).map { … }` (slick's
/// `compiler/RewriteJoins.scala`) report `value _2 is not a member of Any`.
/// Installing the forwarder hides the real declaration, which ordinary member
/// lookup would otherwise reach through the parent (or have
/// `PickleSupply::complete` supply from an ancestor's pickle on demand).
///
/// Restricted to a class with type parameters: on a monomorphic class an
/// unsigned descriptor is the whole truth, and an `Object` in it is a real
/// `Any`.
fn is_erased_scala_forwarder(
    st: &SymbolTable,
    owner: SymbolId,
    c: &crate::javaclass::JavaClass,
    m: &crate::javaclass::JavaMethod,
) -> bool {
    c.is_scala && m.signature.is_none() && m.name != "<init>" && !st.get(owner).tparams.is_empty()
}

fn fill_java_members(st: &mut SymbolTable, owner: SymbolId, c: &crate::javaclass::JavaClass) {
    for m in &c.methods {
        if is_erased_scala_forwarder(st, owner, c, m) {
            continue;
        }
        if let Some(id) = existing_java_method(st, owner, m) {
            st.get_mut(id).flags = java_method_flags(m);
            if st.get(id).jvm_name.is_empty() {
                st.get_mut(id).jvm_name = m.desc.clone();
            }
            continue;
        }
        let mut env = tparam_env(st, owner);
        let parsed = m
            .signature
            .as_deref()
            .and_then(crate::javasign::parse_method_sig);
        let (params, ret, mtparams) = if let Some(ms) = parsed {
            let mut tp_ids = Vec::new();
            for p in &ms.tparams {
                let tid = st.alloc(&p.name, owner, SymKind::TypeParam, Flags::EMPTY, "");
                st.get_mut(tid).ty = Type::TypeParam(tid);
                env.insert(p.name.clone(), tid);
                tp_ids.push(tid);
            }
            for (p, tid) in ms.tparams.iter().zip(&tp_ids) {
                let bounds: Vec<Type> = p
                    .bounds
                    .iter()
                    .map(|b| jtype_to_type(st, b, &env))
                    .filter(|t| !matches!(t, Type::Any | Type::AnyRef))
                    .collect();
                st.get_mut(*tid).parents = bounds;
            }
            let params: Vec<Type> = ms
                .params
                .iter()
                .map(|t| jtype_to_type(st, t, &env))
                .collect();
            let ret = java_result_obj(jtype_to_type(st, &ms.ret, &env));
            (params, ret, tp_ids)
        } else {
            let (p, r) = parse_method_desc_java(st, &m.desc);
            (p, java_result_obj(r), Vec::new())
        };
        let mut params = params;
        if crate::javaclass::is_java_varargs(m.access) {
            if let Some(last) = params.last_mut() {
                if let Type::Array(elem) = last {
                    *last = Type::Repeated(elem.clone());
                }
            }
        }
        let names: Vec<String> = (0..params.len()).map(|i| format!("x${i}")).collect();
        let flags = java_method_flags(m);
        let id = add_method_types(st, owner, &m.name, names, params, ret);
        st.get_mut(id).flags = flags;
        st.get_mut(id).jvm_name = m.desc.clone();
        if !mtparams.is_empty() {
            for tid in &mtparams {
                st.get_mut(*tid).owner = id;
            }
            st.get_mut(id).tparams = mtparams;
        }
    }
    for f in &c.fields {
        if f.name == "MODULE$" {
            continue;
        }
        if st
            .lookup_member(owner, &f.name)
            .iter()
            .any(|&id| st.get(id).kind == SymKind::Term)
        {
            continue;
        }
        let ty = parse_field_ty_java(st, &f.desc).0;
        let mut flags = Flags::JAVA;
        if crate::javaclass::is_java_static(f.access) {
            flags = flags.with(Flags::STATIC);
        }
        if crate::javaclass::is_java_protected(f.access) {
            flags = flags.with(Flags::PROTECTED);
        }
        if crate::javaclass::is_java_enum(f.access) {
            flags = flags.with(Flags::ENUM);
        }
        let id = add_term(st, owner, &f.name, ty);
        st.get_mut(id).flags = flags;
        st.get_mut(id).jvm_name = f.desc.clone();
    }
}

fn jtype_to_type(
    st: &mut SymbolTable,
    t: &crate::javasign::JType,
    env: &std::collections::HashMap<String, SymbolId>,
) -> Type {
    use crate::javasign::JType;
    match t {
        JType::Void => Type::Unit,
        JType::Boolean => Type::Boolean,
        // `byte` and `short` used to be read as `Int` because `scala.Byte`
        // and `scala.Short` had no usable JVM representation; they do now, so
        // a Java `byte[]` really is an `Array[Byte]` and `Byte.valueOf(byte)`
        // accepts a `Byte`.
        JType::Byte => Type::Byte,
        JType::Short => Type::Short,
        JType::Int => Type::Int,
        JType::Char => Type::Char,
        JType::Long => Type::Long,
        JType::Float => Type::Float,
        JType::Double => Type::Double,
        JType::Star => Type::Wildcard,
        JType::Extends(t) => Type::BoundedWildcard {
            lo: None,
            hi: Some(Box::new(jtype_to_type(st, t, env))),
        },
        JType::Super(t) => Type::BoundedWildcard {
            lo: Some(Box::new(jtype_to_type(st, t, env))),
            hi: None,
        },
        JType::Var(n) => env
            .get(n)
            .copied()
            .map(Type::TypeParam)
            .unwrap_or(Type::Any),
        JType::Array(e) => Type::Array(Box::new(jtype_to_type(st, e, env))),
        JType::Class { jvm, args } => {
            if jvm == "java/lang/Object" {
                return Type::Any;
            }
            if jvm == "java/lang/String" {
                if args.is_empty() {
                    return Type::String;
                }
            }
            // A generic signature cannot write the bottom types themselves,
            // so scalac stands in with the synthetic runtime placeholder
            // classes: `case object Canceled extends Outcome[Nothing]`'s own
            // class Signature reads `Outcome<Lscala/runtime/Nothing$;>`.
            // `parse_field_ty` (descriptor parsing, no generics) already
            // makes this substitution; a generic-signature parent left it as
            // an ordinary class stub named `Nothing$`, so `Outcome[Nothing]`
            // was not recognised as a subtype of `Outcome[Int]` -- the
            // covariance check compared `Nothing$` against `Int` and failed,
            // surfacing as "type mismatch; found: Canceled$ required:
            // Outcome[Int]" for every case object nested in a companion whose
            // trait is parameterized.
            if jvm == "scala/runtime/Nothing$" {
                return Type::Nothing;
            }
            if jvm == "scala/runtime/Null$" {
                return Type::Null;
            }
            let sym = find_or_stub_java_class(st, jvm);
            let as_ = args.iter().map(|a| jtype_to_type(st, a, env)).collect();
            Type::Class { sym, args: as_ }
        }
    }
}

/// nsc's `objToAny` widens a Java *parameter* of type `Object` to `Any`; a
/// **result** of type `Object` stays `AnyRef`, and that is what gives it `eq`,
/// `ne` and `synchronized`. Every `Object` was read as `Any` here, so
/// `cv.unwrapped eq null` (slick's `GlobalConfig`, on typesafe-config's
/// `ConfigValue.unwrapped(): Object`) was "value eq is not a member of Any".
///
/// Only the top level of the result is narrowed. Turning `Object` into
/// `AnyRef` *inside* a signature as well is what nsc does, but it also
/// rewrites every `Hashtable<Object, Object>` in sight, and that regressed
/// `IndexedSeq[Any] <: Int => Any` in slick's `HeapBackend`; the wider change
/// bought nothing on slick, so it is not made here.
fn java_result_obj(t: Type) -> Type {
    match t {
        Type::Any => Type::AnyRef,
        other => other,
    }
}

fn parse_method_desc_java(st: &mut SymbolTable, desc: &str) -> (Vec<Type>, Type) {
    let rest = desc.strip_prefix('(').unwrap_or(desc);
    let (params_s, ret_s) = match rest.find(')') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => ("", rest),
    };
    let mut params = Vec::new();
    let mut s = params_s;
    while !s.is_empty() {
        let (t, n) = parse_field_ty_java(st, s);
        params.push(t);
        s = &s[n..];
        if n == 0 {
            break;
        }
    }
    let (ret, _) = parse_field_ty_java(st, ret_s);
    (params, ret)
}

fn parse_field_ty_java(st: &mut SymbolTable, s: &str) -> (Type, usize) {
    if s.is_empty() {
        return (Type::Any, 0);
    }
    match s.as_bytes()[0] {
        b'V' => (Type::Unit, 1),
        b'Z' => (Type::Boolean, 1),
        b'I' => (Type::Int, 1),
        b'J' => (Type::Long, 1),
        b'F' => (Type::Float, 1),
        b'D' => (Type::Double, 1),
        b'C' => (Type::Char, 1),
        b'B' => (Type::Byte, 1),
        b'S' => (Type::Short, 1),
        b'[' => {
            let (inner, n) = parse_field_ty_java(st, &s[1..]);
            (Type::Array(Box::new(inner)), n + 1)
        }
        b'L' => {
            let end = s.find(';').unwrap_or(s.len());
            let inner = &s[1..end];
            let consumed = if end < s.len() { end + 1 } else { end };
            let ty = if inner == "java/lang/String" {
                Type::String
            } else if inner == "java/lang/Object" {
                Type::Any
            } else if inner.contains('/') && !inner.starts_with("scala/") {
                // A descriptor names one exact class. Looking the simple name
                // up in scope instead produced a second, unrelated symbol for
                // `org/slf4j/Logger` whenever it was not already in scope.
                Type::Class {
                    sym: find_or_stub_java_class(st, inner),
                    args: vec![],
                }
            } else {
                parse_field_ty(st, s).0
            };
            (ty, consumed)
        }
        _ => (Type::Any, 1),
    }
}

fn method_arity(s: &crate::symbol::Symbol) -> usize {
    let n = s.paramss.iter().flatten().count();
    if n > 0 {
        return n;
    }
    match &s.ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().count(),
        Type::Function { params, .. } => params.len(),
        _ => 0,
    }
}

fn desc_param_count(desc: &str) -> usize {
    let rest = desc.strip_prefix('(').unwrap_or(desc);
    let params = rest.split_once(')').map(|(p, _)| p).unwrap_or("");
    let b = params.as_bytes();
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' => {
                n += 1;
                i += 1;
            }
            b'[' => {
                while i < b.len() && b[i] == b'[' {
                    i += 1;
                }
                if i < b.len() && b[i] == b'L' {
                    while i < b.len() && b[i] != b';' {
                        i += 1;
                    }
                    i += 1;
                } else {
                    i += 1;
                }
                n += 1;
            }
            b'L' => {
                while i < b.len() && b[i] != b';' {
                    i += 1;
                }
                i += 1;
                n += 1;
            }
            _ => i += 1,
        }
    }
    n
}
