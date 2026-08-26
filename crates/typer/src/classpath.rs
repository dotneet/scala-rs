//! Install symbols recovered from classpath classfiles / ScalaSignature pickles.

use scala_rs_parser::{Flags, SymbolId, Type};

use crate::check::ClasspathClass;
use crate::symbol::{SymKind, SymbolTable};

pub fn install_classpath(st: &mut SymbolTable, classes: &[ClasspathClass]) {
    let mut installed: Vec<(usize, SymbolId)> = Vec::new();
    for (i, c) in classes.iter().enumerate() {
        if c.jvm_name.contains("$anon") || c.jvm_name.ends_with("$class") {
            continue;
        }
        let simple = simple_name(&c.jvm_name);
        if simple.is_empty() {
            continue;
        }
        if is_forwarder_of_module(classes, c) {
            continue;
        }
        let owner = st.root;
        if c.is_module {
            let jvm = if c.jvm_name.ends_with('$') {
                c.jvm_name.clone()
            } else {
                format!("{}$", c.jvm_name)
            };
            let existing = st
                .lookup(&simple)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::Module);
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
            st.enter_in_current(&simple, m);
            installed.push((i, cls));
        } else {
            let existing = st
                .lookup(&simple)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::Class);
            if let Some(id) = existing {
                installed.push((i, id));
                continue;
            }
            let id = st.alloc(&simple, owner, SymKind::Class, Flags::EMPTY, &c.jvm_name);
            st.get_mut(id).ty = Type::Class {
                sym: id,
                args: vec![],
            };
            st.get_mut(id).parents = vec![Type::AnyRef];
            st.enter_in_current(&simple, id);
            installed.push((i, id));
        }
    }

    for (i, owner) in installed {
        let c = &classes[i];
        if let Some(p) = &c.pickle {
            for m in p {
                if has_method(st, owner, &m.name) {
                    continue;
                }
                add_method(
                    st,
                    owner,
                    &m.name,
                    m.param_names.clone(),
                    m.param_types
                        .iter()
                        .map(|n| resolve_type_name(st, n))
                        .collect(),
                    resolve_type_name(st, &m.ret),
                );
            }
        }
        for m in &c.methods {
            if has_method(st, owner, &m.name) {
                continue;
            }
            let (params, ret) = parse_method_desc(st, &m.desc);
            let names: Vec<String> = (0..params.len()).map(|i| format!("x${i}")).collect();
            add_method(st, owner, &m.name, names, params, ret);
        }
        mark_defaults_from_getters(st, owner);
    }
}

fn has_method(st: &SymbolTable, owner: SymbolId, name: &str) -> bool {
    st.lookup_member(owner, name)
        .iter()
        .any(|&id| st.get(id).kind == SymKind::Method)
}

fn is_forwarder_of_module(classes: &[ClasspathClass], c: &ClasspathClass) -> bool {
    if c.is_module {
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

fn add_method(
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
    st.get_mut(id).paramss = vec![pids];
    st.get_mut(id).ty = Type::Method {
        paramss: vec![params],
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
            if let Some(id) = st
                .lookup(n)
                .into_iter()
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
        b'B' | b'S' => (Type::Int, 1),
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
            } else if name.starts_with("Function") {
                Type::Function {
                    params: vec![Type::Any],
                    ret: Box::new(Type::Any),
                }
            } else {
                resolve_type_name(st, name)
            };
            let consumed = if end < s.len() { end + 1 } else { end };
            (ty, consumed)
        }
        _ => (Type::Any, 1),
    }
}
