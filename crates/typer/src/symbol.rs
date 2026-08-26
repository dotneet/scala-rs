//! Symbols, scopes, and the compilation context.

use scala_rs_parser::{Flags, RefineDecl, SymbolId, Type};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymKind {
    NoSymbol,
    Package,
    Class,
    Module,
    ModuleClass,
    Method,
    Term,
    TypeParam,
    TypeMember,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intrinsic {
    None,
    Println,
    Print,
    IntBin(&'static str),
    IntUn(&'static str),
    LongBin(&'static str),
    LongUn(&'static str),
    DoubleBin(&'static str),
    DoubleUn(&'static str),
    FloatUn(&'static str),
    BoolBin(&'static str),
    BoolUn(&'static str),
    StringConcat,
    AnyToString,
    Identity,
    IntToLong,
    IntToDouble,
    LongToDouble,
    Assert,
    Require,
    NotImplemented,
    StringToInt,
    StringToLong,
    StringToDouble,
    WrapArrowAssoc,
    Locally,
    Any2StringAdd,
    Implicitly,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub owner: SymbolId,
    pub kind: SymKind,
    pub flags: Flags,
    pub ty: Type,
    pub members: Vec<SymbolId>,
    pub jvm_name: String,
    pub intrinsic: Intrinsic,
    /// Constructor / method parameter symbols (flat, first clause).
    pub params: Vec<SymbolId>,
    pub paramss: Vec<Vec<SymbolId>>,
    /// For case classes / classes: constructor parameter field names.
    pub ctor_fields: Vec<SymbolId>,
    pub parents: Vec<Type>,
    pub default_rhs: Option<scala_rs_parser::Tree>,
    /// Class or method type parameters, in order.
    pub tparams: Vec<SymbolId>,
    /// Direct subclasses / objects of a sealed parent (same compilation unit).
    pub children: Vec<SymbolId>,
    /// Self type (`trait T { self: Foo => }`).
    pub self_type: Option<Type>,
}

impl Symbol {
    pub fn is_class_like(&self) -> bool {
        matches!(self.kind, SymKind::Class | SymKind::ModuleClass)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Scope {
    map: HashMap<String, Vec<SymbolId>>,
}

impl Scope {
    pub fn enter(&mut self, name: &str, id: SymbolId) {
        self.map.entry(name.to_string()).or_default().push(id);
    }

    pub fn lookup(&self, name: &str) -> &[SymbolId] {
        self.map.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }
}

pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
    pub root: SymbolId,
    pub scala_pkg: SymbolId,
    pub predef: SymbolId,
    pub any_sym: SymbolId,
    pub anyref_sym: SymbolId,
    pub anyval_sym: SymbolId,
    pub int_sym: SymbolId,
    pub long_sym: SymbolId,
    pub float_sym: SymbolId,
    pub double_sym: SymbolId,
    pub boolean_sym: SymbolId,
    pub unit_sym: SymbolId,
    pub string_sym: SymbolId,
    pub array_sym: SymbolId,
    pub option_sym: SymbolId,
    pub some_sym: SymbolId,
    pub none_sym: SymbolId,
    pub list_sym: SymbolId,
    pub nil_sym: SymbolId,
    pub cons_sym: SymbolId,
    pub object_sym: SymbolId,
    /// Enclosing owner while naming/typing.
    pub owner: SymbolId,
    pub this_class: SymbolId,
}

impl SymbolTable {
    pub fn new() -> Self {
        let mut st = SymbolTable {
            symbols: vec![Symbol {
                id: SymbolId(0),
                name: "<none>".into(),
                owner: SymbolId(0),
                kind: SymKind::NoSymbol,
                flags: Flags::EMPTY,
                ty: Type::NoType,
                members: vec![],
                jvm_name: String::new(),
                intrinsic: Intrinsic::None,
                params: vec![],
                paramss: vec![],
                ctor_fields: vec![],
                parents: vec![],
                default_rhs: None,
                tparams: vec![],
                children: vec![],
                self_type: None,
            }],
            scopes: vec![Scope::default()],
            root: SymbolId(0),
            scala_pkg: SymbolId(0),
            predef: SymbolId(0),
            any_sym: SymbolId(0),
            anyref_sym: SymbolId(0),
            anyval_sym: SymbolId(0),
            int_sym: SymbolId(0),
            long_sym: SymbolId(0),
            float_sym: SymbolId(0),
            double_sym: SymbolId(0),
            boolean_sym: SymbolId(0),
            unit_sym: SymbolId(0),
            string_sym: SymbolId(0),
            array_sym: SymbolId(0),
            option_sym: SymbolId(0),
            some_sym: SymbolId(0),
            none_sym: SymbolId(0),
            list_sym: SymbolId(0),
            nil_sym: SymbolId(0),
            cons_sym: SymbolId(0),
            object_sym: SymbolId(0),
            owner: SymbolId(0),
            this_class: SymbolId(0),
        };
        st.root = st.alloc(
            "<_root_>",
            SymbolId(0),
            SymKind::Package,
            Flags::PACKAGE,
            "scala/runtime",
        );
        st.owner = st.root;
        st
    }

    pub fn alloc(
        &mut self,
        name: impl Into<String>,
        owner: SymbolId,
        kind: SymKind,
        flags: Flags,
        jvm_name: impl Into<String>,
    ) -> SymbolId {
        let id = SymbolId(self.symbols.len() as u32);
        self.symbols.push(Symbol {
            id,
            name: name.into(),
            owner,
            kind,
            flags,
            ty: Type::NoType,
            members: vec![],
            jvm_name: jvm_name.into(),
            intrinsic: Intrinsic::None,
            params: vec![],
            paramss: vec![],
            ctor_fields: vec![],
            parents: vec![],
            default_rhs: None,
            tparams: vec![],
            children: vec![],
            self_type: None,
        });
        if !owner.is_none() && owner.0 as usize <= self.symbols.len() {
            if let Some(ow) = self.symbols.get_mut(owner.0 as usize) {
                ow.members.push(id);
            }
        }
        id
    }

    pub fn get(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.0 as usize]
    }

    pub fn get_mut(&mut self, id: SymbolId) -> &mut Symbol {
        &mut self.symbols[id.0 as usize]
    }

    pub fn enter_in_current(&mut self, name: &str, id: SymbolId) {
        self.scopes.last_mut().unwrap().enter(name, id);
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn lookup(&self, name: &str) -> Vec<SymbolId> {
        for sc in self.scopes.iter().rev() {
            let found = sc.lookup(name);
            if !found.is_empty() {
                return found.to_vec();
            }
        }
        Vec::new()
    }

    pub fn lookup_member(&self, owner: SymbolId, name: &str) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut work = vec![owner];
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            let sym = self.get(id);
            for m in &sym.members {
                if self.get(*m).name == name {
                    out.push(*m);
                }
            }
            for m in &sym.parents.clone() {
                if let Some(ps) = self.class_sym_of(m) {
                    work.push(ps);
                }
            }
            if let Some(st) = &sym.self_type {
                if let Some(ps) = self.class_sym_of(st) {
                    work.push(ps);
                }
            }
        }
        out
    }

    pub fn class_sym_of(&self, ty: &Type) -> Option<SymbolId> {
        match ty {
            Type::Class { sym, .. } | Type::ModuleRef(sym) => Some(*sym),
            Type::Int => Some(self.int_sym),
            Type::Long => Some(self.long_sym),
            Type::Float => Some(self.float_sym),
            Type::Double => Some(self.double_sym),
            Type::Char => self.lookup("Char").into_iter().next(),
            Type::Boolean => Some(self.boolean_sym),
            Type::Unit => Some(self.unit_sym),
            Type::String => Some(self.string_sym),
            Type::Any => Some(self.any_sym),
            Type::AnyRef => Some(self.anyref_sym),
            Type::AnyVal => Some(self.anyval_sym),
            Type::Array(_) => Some(self.array_sym),
            Type::Named { name, .. } => self
                .lookup(name)
                .into_iter()
                .find(|s| self.get(*s).is_class_like()),
            Type::TypeParam(_) => None,
            Type::TypeMember(_) => None,
            Type::Wildcard => Some(self.any_sym),
            Type::Refined { parents, .. } => parents
                .iter()
                .find_map(|p| self.class_sym_of(p))
                .or(Some(self.anyref_sym)),
            _ => None,
        }
    }

    /// Companion module of a class (same name, `SymKind::Module`, same owner).
    pub fn companion_module(&self, class_id: SymbolId) -> Option<SymbolId> {
        let s = self.get(class_id);
        if s.kind == SymKind::Module {
            return Some(class_id);
        }
        let name = s.name.clone();
        let owner = s.owner;
        self.get(owner)
            .members
            .iter()
            .copied()
            .find(|&m| self.get(m).kind == SymKind::Module && self.get(m).name == name)
    }

    pub fn module_class_of(&self, id: SymbolId) -> SymbolId {
        match self.get(id).ty {
            Type::ModuleRef(c) => c,
            _ => id,
        }
    }

    /// Substitute class type arguments into a member type (`List[Int].head` → `Int`).
    pub fn subst_tparams(&self, owner: SymbolId, args: &[Type], ty: &Type) -> Type {
        let tps = self.get(owner).tparams.clone();
        if tps.is_empty() || args.is_empty() {
            return ty.clone();
        }
        subst_map(ty, &tps, args)
    }

    pub fn type_of_class(&self, id: SymbolId) -> Type {
        let s = self.get(id);
        match s.kind {
            SymKind::Module | SymKind::ModuleClass => Type::ModuleRef(id),
            _ => Type::Class {
                sym: id,
                args: vec![],
            },
        }
    }

    /// `class C(val x: T) extends AnyVal` — one ctor param, parent AnyVal.
    pub fn is_value_class(&self, id: SymbolId) -> bool {
        if id.is_none() {
            return false;
        }
        let s = self.get(id);
        if s.kind != SymKind::Class
            || s.flags.contains(Flags::TRAIT)
            || s.flags.contains(Flags::INTERFACE)
            || s.ctor_fields.len() != 1
        {
            return false;
        }
        s.parents.iter().any(|p| {
            matches!(p, Type::AnyVal) || self.class_sym_of(p).is_some_and(|c| c == self.anyval_sym)
        })
    }

    pub fn value_class_underlying(&self, id: SymbolId) -> Option<Type> {
        if !self.is_value_class(id) {
            return None;
        }
        let f = self.get(id).ctor_fields[0];
        Some(self.get(f).ty.clone())
    }

    pub fn is_sealed(&self, id: SymbolId) -> bool {
        !id.is_none() && self.get(id).flags.contains(Flags::SEALED)
    }

    /// Concrete leaves of a sealed hierarchy (case classes, objects, non-sealed classes).
    pub fn sealed_leaves(&self, id: SymbolId) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        fn rec(
            st: &SymbolTable,
            id: SymbolId,
            out: &mut Vec<SymbolId>,
            seen: &mut std::collections::HashSet<u32>,
        ) {
            if !seen.insert(id.0) {
                return;
            }
            let children = st.get(id).children.clone();
            if children.is_empty() {
                let s = st.get(id);
                if s.kind == SymKind::Class
                    && (s.flags.contains(Flags::TRAIT) || s.flags.contains(Flags::ABSTRACT))
                    && s.flags.contains(Flags::SEALED)
                {
                    return;
                }
                out.push(id);
                return;
            }
            for c in children {
                let cs = st.get(c);
                if cs.flags.contains(Flags::SEALED)
                    && (cs.flags.contains(Flags::TRAIT)
                        || cs.flags.contains(Flags::ABSTRACT)
                        || cs.kind == SymKind::Class)
                    && !cs.children.is_empty()
                {
                    rec(st, c, out, seen);
                } else {
                    out.push(c);
                }
            }
        }
        rec(self, id, &mut out, &mut seen);
        out
    }

    pub fn enclosing_class_named(&self, from: SymbolId, name: &str) -> Option<SymbolId> {
        let mut cur = from;
        while !cur.is_none() {
            let s = self.get(cur);
            let n = s.name.trim_end_matches('$');
            if n == name && s.is_class_like() {
                return Some(cur);
            }
            cur = s.owner;
        }
        None
    }

    pub fn is_sub_type(&self, a: &Type, b: &Type) -> bool {
        if a == b {
            return true;
        }
        match (a, b) {
            (Type::Error, _) | (_, Type::Error) => true,
            (Type::Nothing, _) => true,
            (_, Type::Any) => true,
            (
                Type::Null,
                Type::AnyRef
                | Type::String
                | Type::Array(_)
                | Type::Class { .. }
                | Type::ModuleRef(_)
                | Type::Refined { .. },
            ) => true,
            (
                Type::Int
                | Type::Long
                | Type::Double
                | Type::Boolean
                | Type::Unit
                | Type::Char
                | Type::Float,
                Type::AnyVal,
            ) => true,
            (
                Type::String
                | Type::Array(_)
                | Type::Class { .. }
                | Type::ModuleRef(_)
                | Type::Function { .. }
                | Type::Refined { .. },
                Type::AnyRef,
            ) => true,
            (Type::Class { sym: s1, .. }, Type::Class { sym: s2, .. }) if s1 == s2 => true,
            (Type::Class { sym: s1, .. }, b) => self
                .get(*s1)
                .parents
                .clone()
                .iter()
                .any(|p| self.is_sub_type(p, b)),
            (Type::Array(x), Type::Array(y)) => self.is_sub_type(x, y),
            (Type::ModuleRef(s), Type::Class { sym, .. }) if s == sym => true,
            (Type::ModuleRef(s), b) => self
                .get(*s)
                .parents
                .clone()
                .iter()
                .any(|p| self.is_sub_type(p, b)),
            (Type::TypeParam(a), Type::TypeParam(b)) if a == b => true,
            (Type::TypeMember(a), Type::TypeMember(b)) if a == b => true,
            (Type::TypeParam(_), Type::AnyRef | Type::AnyVal) => true,
            (Type::TypeMember(_), Type::AnyRef | Type::AnyVal) => true,
            (Type::Wildcard, Type::AnyRef | Type::AnyVal | Type::Wildcard) => true,
            (_, Type::Wildcard) => true,
            (
                Type::Function {
                    params: p1,
                    ret: r1,
                },
                Type::Function {
                    params: p2,
                    ret: r2,
                },
            ) if p1.len() == p2.len() => {
                p2.iter()
                    .zip(p1.iter())
                    .all(|(exp, act)| self.is_sub_type(exp, act))
                    && self.is_sub_type(r1, r2)
            }
            (Type::ByName(a), Type::ByName(b)) => self.is_sub_type(a, b),
            (Type::Repeated(a), Type::Repeated(b)) => self.is_sub_type(a, b),
            (a, Type::Refined { parents, decls }) => {
                parents.iter().all(|p| self.is_sub_type(a, p))
                    && self.conforms_to_refinement(a, decls)
            }
            (Type::Refined { parents, .. }, b) => {
                parents.iter().any(|p| self.is_sub_type(p, b))
            }
            _ => false,
        }
    }

    pub fn display_type(&self, ty: &Type) -> String {
        match ty {
            Type::Class { sym, args } => {
                let mut s = self.get(*sym).name.clone();
                if !args.is_empty() {
                    s.push('[');
                    s.push_str(
                        &args
                            .iter()
                            .map(|a| self.display_type(a))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    s.push(']');
                }
                s
            }
            Type::ModuleRef(id) => self.get(*id).name.clone(),
            Type::TypeParam(id) => self.get(*id).name.clone(),
            Type::TypeMember(id) => {
                let s = self.get(*id);
                format!("{}.{}", self.get(s.owner).name, s.name)
            }
            Type::Refined { parents, decls } => {
                let mut s = String::new();
                if parents.is_empty() {
                    s.push_str("{ ");
                } else {
                    for (i, p) in parents.iter().enumerate() {
                        if i > 0 {
                            s.push_str(" with ");
                        }
                        s.push_str(&self.display_type(p));
                    }
                    s.push_str(" { ");
                }
                for (i, d) in decls.iter().enumerate() {
                    if i > 0 {
                        s.push_str("; ");
                    }
                    s.push_str(&d.to_string());
                }
                s.push_str(" }");
                s
            }
            Type::Array(t) => format!("Array[{}]", self.display_type(t)),
            Type::Method { paramss, ret } => {
                let mut s = String::new();
                for ps in paramss {
                    s.push('(');
                    s.push_str(
                        &ps.iter()
                            .map(|p| self.display_type(p))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    s.push(')');
                }
                s.push_str(&self.display_type(ret));
                s
            }
            Type::Function { params, ret } => {
                let p = params
                    .iter()
                    .map(|p| self.display_type(p))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({}) => {}", p, self.display_type(ret))
            }
            other => other.to_string(),
        }
    }

    pub fn jvm_internal(&self, id: SymbolId) -> String {
        let s = self.get(id);
        if !s.jvm_name.is_empty() {
            return s.jvm_name.clone();
        }
        // walk owners
        let mut parts = vec![s.name.clone()];
        let mut o = s.owner;
        while !o.is_none() && self.get(o).kind == SymKind::Package && self.get(o).name != "<_root_>"
        {
            parts.push(self.get(o).name.clone());
            o = self.get(o).owner;
        }
        parts.reverse();
        parts.join("/")
    }

    /// Replace abstract type members with aliases defined on `from` (and parents).
    pub fn expand_type_members(&self, from: SymbolId, ty: &Type) -> Type {
        match ty {
            Type::TypeMember(id) => {
                let name = self.get(*id).name.clone();
                for m in self.lookup_member(from, &name) {
                    if self.get(m).kind == SymKind::TypeMember {
                        let t = self.get(m).ty.clone();
                        if matches!(t, Type::TypeMember(_) | Type::NoType | Type::Error) {
                            return Type::TypeMember(m);
                        }
                        return self.expand_type_members(from, &t);
                    }
                }
                ty.clone()
            }
            Type::Class { sym, args } => Type::Class {
                sym: *sym,
                args: args
                    .iter()
                    .map(|a| self.expand_type_members(from, a))
                    .collect(),
            },
            Type::Array(t) => Type::Array(Box::new(self.expand_type_members(from, t))),
            Type::Function { params, ret } => Type::Function {
                params: params
                    .iter()
                    .map(|p| self.expand_type_members(from, p))
                    .collect(),
                ret: Box::new(self.expand_type_members(from, ret)),
            },
            Type::Method { paramss, ret } => Type::Method {
                paramss: paramss
                    .iter()
                    .map(|ps| {
                        ps.iter()
                            .map(|p| self.expand_type_members(from, p))
                            .collect()
                    })
                    .collect(),
                ret: Box::new(self.expand_type_members(from, ret)),
            },
            Type::ByName(t) => Type::ByName(Box::new(self.expand_type_members(from, t))),
            Type::Repeated(t) => Type::Repeated(Box::new(self.expand_type_members(from, t))),
            Type::Tuple(ts) => Type::Tuple(
                ts.iter()
                    .map(|t| self.expand_type_members(from, t))
                    .collect(),
            ),
            Type::Refined { parents, decls } => Type::Refined {
                parents: parents
                    .iter()
                    .map(|p| self.expand_type_members(from, p))
                    .collect(),
                decls: decls
                    .iter()
                    .map(|d| expand_refine_decl(self, from, d))
                    .collect(),
            },
            other => other.clone(),
        }
    }

    /// Expand type members using aliases on a (possibly refined) prefix type.
    pub fn expand_in_type(&self, from: &Type, ty: &Type) -> Type {
        match from {
            Type::Refined { parents, decls } => {
                let mut t = subst_refine_aliases(self, decls, ty);
                for p in parents {
                    t = self.expand_in_type(p, &t);
                }
                t
            }
            Type::Class { sym, .. } | Type::ModuleRef(sym) => self.expand_type_members(*sym, ty),
            _ => {
                if let Some(c) = self.class_sym_of(from) {
                    self.expand_type_members(c, ty)
                } else {
                    ty.clone()
                }
            }
        }
    }

    pub fn refine_member_type(decls: &[RefineDecl], name: &str) -> Option<Type> {
        for d in decls {
            match d {
                RefineDecl::Def {
                    name: n,
                    paramss,
                    ret,
                } if n == name => {
                    return Some(Type::Method {
                        paramss: paramss.clone(),
                        ret: Box::new(ret.clone()),
                    });
                }
                RefineDecl::Val { name: n, ty } if n == name => return Some(ty.clone()),
                RefineDecl::Type { name: n, rhs } if n == name => {
                    return Some(rhs.clone().unwrap_or(Type::Named {
                        name: n.clone(),
                        args: vec![],
                    }));
                }
                _ => {}
            }
        }
        None
    }

    pub fn refined_has_term_members(decls: &[RefineDecl]) -> bool {
        decls
            .iter()
            .any(|d| matches!(d, RefineDecl::Def { .. } | RefineDecl::Val { .. }))
    }

    fn conforms_to_refinement(&self, a: &Type, decls: &[RefineDecl]) -> bool {
        for d in decls {
            match d {
                RefineDecl::Type { name, rhs } => {
                    let Some(have) = self.lookup_type_member_on(a, name) else {
                        return false;
                    };
                    if let Some(want) = rhs {
                        if !self.types_same_enough(&have, want) {
                            return false;
                        }
                    }
                }
                RefineDecl::Def { name, ret, .. } => {
                    let Some(have) = self.lookup_term_member_on(a, name) else {
                        return false;
                    };
                    if !self.is_sub_type(have.result(), ret) {
                        return false;
                    }
                }
                RefineDecl::Val { name, ty } => {
                    let Some(have) = self.lookup_term_member_on(a, name) else {
                        return false;
                    };
                    if !self.is_sub_type(have.result(), ty) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn types_same_enough(&self, a: &Type, b: &Type) -> bool {
        a == b || (self.is_sub_type(a, b) && self.is_sub_type(b, a))
    }

    pub(crate) fn lookup_type_member_on(&self, ty: &Type, name: &str) -> Option<Type> {
        if let Type::Refined { parents, decls } = ty {
            if let Some(t) = Self::refine_member_type(decls, name) {
                if decls.iter().any(|d| matches!(d, RefineDecl::Type { name: n, .. } if n == name)) {
                    return Some(t);
                }
            }
            for p in parents {
                if let Some(t) = self.lookup_type_member_on(p, name) {
                    return Some(t);
                }
            }
        }
        let cls = self.class_sym_of(ty)?;
        let found = self.lookup_member(cls, name);
        for m in found {
            if self.get(m).kind == SymKind::TypeMember {
                let rhs = self.get(m).ty.clone();
                return Some(match rhs {
                    Type::NoType | Type::Error | Type::TypeMember(_) => {
                        self.expand_in_type(ty, &Type::TypeMember(m))
                    }
                    other => self.expand_in_type(ty, &other),
                });
            }
        }
        None
    }

    fn lookup_term_member_on(&self, ty: &Type, name: &str) -> Option<Type> {
        if let Type::Refined { parents, decls } = ty {
            if decls.iter().any(|d| {
                matches!(
                    d,
                    RefineDecl::Def { name: n, .. } | RefineDecl::Val { name: n, .. } if n == name
                )
            }) {
                return Self::refine_member_type(decls, name);
            }
            for p in parents {
                if let Some(t) = self.lookup_term_member_on(p, name) {
                    return Some(t);
                }
            }
        }
        let cls = self.class_sym_of(ty)?;
        self.lookup_member(cls, name).into_iter().find_map(|m| {
            let s = self.get(m);
            match s.kind {
                SymKind::Method | SymKind::Term => Some(self.expand_in_type(ty, &s.ty)),
                _ => None,
            }
        })
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

fn subst_map(ty: &Type, tps: &[scala_rs_parser::SymbolId], args: &[Type]) -> Type {
    match ty {
        Type::TypeParam(id) => tps
            .iter()
            .position(|t| t == id)
            .and_then(|i| args.get(i).cloned())
            .unwrap_or_else(|| ty.clone()),
        Type::TypeMember(_) => ty.clone(),
        Type::Class { sym, args: as_ } => Type::Class {
            sym: *sym,
            args: as_.iter().map(|a| subst_map(a, tps, args)).collect(),
        },
        Type::Array(t) => Type::Array(Box::new(subst_map(t, tps, args))),
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(|p| subst_map(p, tps, args)).collect(),
            ret: Box::new(subst_map(ret, tps, args)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| subst_map(p, tps, args)).collect())
                .collect(),
            ret: Box::new(subst_map(ret, tps, args)),
        },
        Type::ByName(t) => Type::ByName(Box::new(subst_map(t, tps, args))),
        Type::Repeated(t) => Type::Repeated(Box::new(subst_map(t, tps, args))),
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(|t| subst_map(t, tps, args)).collect()),
        Type::Named { name, args: as_ } => Type::Named {
            name: name.clone(),
            args: as_.iter().map(|a| subst_map(a, tps, args)).collect(),
        },
        Type::Refined { parents, decls } => Type::Refined {
            parents: parents.iter().map(|p| subst_map(p, tps, args)).collect(),
            decls: decls
                .iter()
                .map(|d| subst_refine_decl(d, tps, args))
                .collect(),
        },
        other => other.clone(),
    }
}

fn expand_refine_decl(st: &SymbolTable, from: SymbolId, d: &RefineDecl) -> RefineDecl {
    match d {
        RefineDecl::Type { name, rhs } => RefineDecl::Type {
            name: name.clone(),
            rhs: rhs.as_ref().map(|t| st.expand_type_members(from, t)),
        },
        RefineDecl::Def {
            name,
            paramss,
            ret,
        } => RefineDecl::Def {
            name: name.clone(),
            paramss: paramss
                .iter()
                .map(|ps| {
                    ps.iter()
                        .map(|p| st.expand_type_members(from, p))
                        .collect()
                })
                .collect(),
            ret: st.expand_type_members(from, ret),
        },
        RefineDecl::Val { name, ty } => RefineDecl::Val {
            name: name.clone(),
            ty: st.expand_type_members(from, ty),
        },
    }
}

fn subst_refine_decl(d: &RefineDecl, tps: &[scala_rs_parser::SymbolId], args: &[Type]) -> RefineDecl {
    match d {
        RefineDecl::Type { name, rhs } => RefineDecl::Type {
            name: name.clone(),
            rhs: rhs.as_ref().map(|t| subst_map(t, tps, args)),
        },
        RefineDecl::Def {
            name,
            paramss,
            ret,
        } => RefineDecl::Def {
            name: name.clone(),
            paramss: paramss
                .iter()
                .map(|ps| ps.iter().map(|p| subst_map(p, tps, args)).collect())
                .collect(),
            ret: subst_map(ret, tps, args),
        },
        RefineDecl::Val { name, ty } => RefineDecl::Val {
            name: name.clone(),
            ty: subst_map(ty, tps, args),
        },
    }
}

fn subst_refine_aliases(st: &SymbolTable, decls: &[RefineDecl], ty: &Type) -> Type {
    match ty {
        Type::TypeMember(id) => {
            let name = st.get(*id).name.clone();
            for d in decls {
                if let RefineDecl::Type {
                    name: n,
                    rhs: Some(rhs),
                } = d
                {
                    if n == &name {
                        return subst_refine_aliases(st, decls, rhs);
                    }
                }
            }
            ty.clone()
        }
        Type::Class { sym, args } => Type::Class {
            sym: *sym,
            args: args
                .iter()
                .map(|a| subst_refine_aliases(st, decls, a))
                .collect(),
        },
        Type::Array(t) => Type::Array(Box::new(subst_refine_aliases(st, decls, t))),
        Type::Function { params, ret } => Type::Function {
            params: params
                .iter()
                .map(|p| subst_refine_aliases(st, decls, p))
                .collect(),
            ret: Box::new(subst_refine_aliases(st, decls, ret)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .iter()
                .map(|ps| {
                    ps.iter()
                        .map(|p| subst_refine_aliases(st, decls, p))
                        .collect()
                })
                .collect(),
            ret: Box::new(subst_refine_aliases(st, decls, ret)),
        },
        Type::ByName(t) => Type::ByName(Box::new(subst_refine_aliases(st, decls, t))),
        Type::Repeated(t) => Type::Repeated(Box::new(subst_refine_aliases(st, decls, t))),
        Type::Tuple(ts) => Type::Tuple(
            ts.iter()
                .map(|t| subst_refine_aliases(st, decls, t))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub(crate) fn subst_tparams_slice(tps: &[SymbolId], args: &[Type], ty: &Type) -> Type {
    subst_map(ty, tps, args)
}
