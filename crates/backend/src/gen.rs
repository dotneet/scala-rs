//! Walk a typed compilation unit and emit JVM classfiles (major 50).

use crate::classfile::{
    ClassEmit, Field, Method, Pool, ACC_ABSTRACT, ACC_FINAL, ACC_INTERFACE, ACC_PRIVATE,
    ACC_PUBLIC, ACC_STATIC, ACC_SUPER,
};
use crate::code::Assembler;
use scala_rs_parser::{Flags, Lit, SymbolId, Tree, TreeKind, Type};
use scala_rs_typer::{Intrinsic, SymKind, SymbolTable};
use std::collections::{HashMap, HashSet};

pub struct EmittedClass {
    /// e.g. `"Main"`, `"Main$"`, `"Point"`
    pub internal_name: String,
    pub bytes: Vec<u8>,
}

/// Walk a typed compilation unit and emit classes.
pub fn emit(tree: &Tree, st: &SymbolTable, source_name: &str) -> Vec<EmittedClass> {
    let mut g = Gen {
        st,
        source_name,
        out: Vec::new(),
    };
    g.walk(tree);
    g.out
}

struct Gen<'a> {
    st: &'a SymbolTable,
    source_name: &'a str,
    out: Vec<EmittedClass>,
}

struct EmitCtx<'a> {
    st: &'a SymbolTable,
    class_sym: SymbolId,
    class_name: &'a str,
    ret_ty: Type,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JvmSort {
    Int,
    Long,
    Float,
    Double,
    Ref,
    Void,
}

impl JvmSort {
    fn slots(self) -> u16 {
        match self {
            JvmSort::Long | JvmSort::Double => 2,
            JvmSort::Void => 0,
            _ => 1,
        }
    }
}

struct Frame {
    locals: HashMap<SymbolId, (u16, JvmSort)>,
    next_slot: u16,
}

impl Frame {
    fn instance() -> Self {
        Frame {
            locals: HashMap::new(),
            next_slot: 1,
        }
    }

    fn alloc(&mut self, id: SymbolId, sort: JvmSort) -> u16 {
        let slot = self.next_slot;
        if !id.is_none() {
            self.locals.insert(id, (slot, sort));
        }
        self.next_slot += sort.slots();
        slot
    }

    fn alloc_tmp(&mut self, sort: JvmSort) -> u16 {
        let slot = self.next_slot;
        self.next_slot += sort.slots();
        slot
    }

    fn get(&self, id: SymbolId) -> Option<(u16, JvmSort)> {
        self.locals.get(&id).copied()
    }
}

struct ClassBuilder {
    access: u16,
    this_name: String,
    super_name: String,
    interfaces: Vec<String>,
    fields: Vec<Field>,
    methods: Vec<Method>,
    pool: Pool,
    source: String,
}

impl ClassBuilder {
    fn new(this_name: String, source: &str) -> Self {
        ClassBuilder {
            access: ACC_PUBLIC | ACC_SUPER,
            this_name,
            super_name: "java/lang/Object".into(),
            interfaces: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            pool: Pool::new(),
            source: source.to_string(),
        }
    }

    fn add_code(
        &mut self,
        access: u16,
        name: &str,
        desc: &str,
        max_locals: u16,
        gen: impl FnOnce(&mut Assembler),
    ) {
        let mut asm = Assembler::with_pool(std::mem::take(&mut self.pool), max_locals.max(1));
        gen(&mut asm);
        let (code, pool) = asm.finish();
        self.pool = pool;
        self.methods.push(Method {
            access,
            name: name.to_string(),
            desc: desc.to_string(),
            code: Some(code),
        });
    }

    fn add_abstract(&mut self, access: u16, name: &str, desc: &str) {
        self.methods.push(Method {
            access,
            name: name.to_string(),
            desc: desc.to_string(),
            code: None,
        });
    }

    fn finish(self) -> EmittedClass {
        let this_name = self.this_name.clone();
        let class = ClassEmit {
            access: self.access,
            this_name: self.this_name,
            super_name: self.super_name,
            interfaces: self.interfaces,
            fields: self.fields,
            methods: self.methods,
            source: self.source,
        };
        let bytes = class.write_with_pool(self.pool).expect("classfile write");
        EmittedClass {
            internal_name: this_name,
            bytes,
        }
    }
}

// ---------------------------------------------------------------------------
// descriptors
// ---------------------------------------------------------------------------

fn jvm_sort(ty: &Type) -> JvmSort {
    match ty {
        Type::Unit | Type::NoType | Type::Nothing => JvmSort::Void,
        Type::Boolean | Type::Int | Type::Char => JvmSort::Int,
        Type::Long => JvmSort::Long,
        Type::Float => JvmSort::Float,
        Type::Double => JvmSort::Double,
        _ => JvmSort::Ref,
    }
}

fn is_unit_like(ty: &Type) -> bool {
    matches!(ty, Type::Unit | Type::NoType)
}

fn class_internal(st: &SymbolTable, id: SymbolId) -> String {
    st.jvm_internal(id)
}

fn jvm_desc(st: &SymbolTable, ty: &Type) -> String {
    match ty {
        Type::Unit | Type::NoType | Type::Nothing => "V".into(),
        Type::Boolean => "Z".into(),
        Type::Int => "I".into(),
        Type::Long => "J".into(),
        Type::Float => "F".into(),
        Type::Double => "D".into(),
        Type::Char => "C".into(),
        Type::String => "Ljava/lang/String;".into(),
        Type::Array(t) => format!("[{}", jvm_desc(st, t)),
        Type::Class { sym, .. } => format!("L{};", class_internal(st, *sym)),
        Type::ModuleRef(sym) => format!("L{};", class_internal(st, *sym)),
        Type::Any | Type::AnyRef | Type::AnyVal | Type::Null | Type::Error => {
            "Ljava/lang/Object;".into()
        }
        Type::Function { params, .. } => format!("Lscala/Function{};", params.len()),
        Type::Tuple(ts) => format!("Lscala/Tuple{};", ts.len()),
        Type::Method { ret, .. } => jvm_desc(st, ret),
        Type::ByName(t) => jvm_desc(st, t),
        Type::Named { name, args } if name == "Array" && args.len() == 1 => {
            format!("[{}", jvm_desc(st, &args[0]))
        }
        Type::Named { name, .. } => {
            let n = name.replace('.', "/");
            format!("L{n};")
        }
        Type::Overload(_) => "Ljava/lang/Object;".into(),
    }
}

fn jvm_method_desc(st: &SymbolTable, params: &[Type], ret: &Type) -> String {
    let mut s = String::from("(");
    for p in params {
        s.push_str(&jvm_desc(st, p));
    }
    s.push(')');
    s.push_str(&jvm_desc(st, ret));
    s
}

fn method_ret_ty(def: &Tree) -> Type {
    match &def.ty {
        Type::Method { ret, .. } => (**ret).clone(),
        Type::Function { ret, .. } => (**ret).clone(),
        t if !t.is_no_type() => t.clone(),
        _ => Type::Unit,
    }
}

fn def_param_types(st: &SymbolTable, def: &Tree) -> Vec<Type> {
    match &def.kind {
        TreeKind::DefDef { vparamss, .. } => vparamss
            .iter()
            .flatten()
            .map(|p| {
                if !p.ty.is_no_type() && !p.ty.is_error() {
                    p.ty.clone()
                } else if !p.sym.is_none() {
                    st.get(p.sym).ty.clone()
                } else {
                    Type::Any
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn def_method_desc(st: &SymbolTable, def: &Tree) -> String {
    jvm_method_desc(st, &def_param_types(st, def), &method_ret_ty(def))
}

fn method_desc_from_sym(st: &SymbolTable, id: SymbolId) -> String {
    let s = st.get(id);
    match &s.ty {
        Type::Method { paramss, ret } => {
            let params: Vec<Type> = paramss.iter().flatten().cloned().collect();
            if params.iter().any(|p| p.is_no_type() || p.is_error()) {
                let params: Vec<Type> = s.params.iter().map(|p| st.get(*p).ty.clone()).collect();
                jvm_method_desc(st, &params, ret)
            } else {
                jvm_method_desc(st, &params, ret)
            }
        }
        Type::Function { params, ret } => jvm_method_desc(st, params, ret),
        _ => {
            let params: Vec<Type> = s.params.iter().map(|p| st.get(*p).ty.clone()).collect();
            jvm_method_desc(st, &params, &Type::Unit)
        }
    }
}

fn ctor_desc(st: &SymbolTable, class_id: SymbolId, args: &[Tree]) -> String {
    let fields = &st.get(class_id).ctor_fields;
    let mut d = String::from("(");
    if !fields.is_empty() {
        for f in fields {
            d.push_str(&jvm_desc(st, &st.get(*f).ty));
        }
    } else {
        for a in args {
            d.push_str(&jvm_desc(st, &a.ty));
        }
    }
    d.push_str(")V");
    d
}

fn is_interface_sym(st: &SymbolTable, id: SymbolId) -> bool {
    let s = st.get(id);
    s.flags.contains(Flags::TRAIT) || s.flags.contains(Flags::INTERFACE)
}

fn is_module_class(st: &SymbolTable, id: SymbolId) -> bool {
    let s = st.get(id);
    s.kind == SymKind::ModuleClass || s.kind == SymKind::Module || s.flags.contains(Flags::MODULE)
}

fn module_class_id(st: &SymbolTable, id: SymbolId) -> SymbolId {
    match st.get(id).ty {
        Type::ModuleRef(c) => c,
        _ => id,
    }
}

fn strip_module_dollar(name: &str) -> String {
    if let Some(rest) = name.strip_suffix('$') {
        rest.to_string()
    } else {
        name.to_string()
    }
}

fn split_parents(st: &SymbolTable, parents: &[Tree]) -> (String, Vec<String>) {
    let mut super_name = "java/lang/Object".to_string();
    let mut ifaces = Vec::new();
    let mut found_class = false;
    for p in parents {
        let id = st
            .class_sym_of(&p.ty)
            .or_else(|| if p.sym.is_none() { None } else { Some(p.sym) });
        let Some(id) = id else {
            continue;
        };
        let s = st.get(id);
        let jvm = class_internal(st, id);
        if jvm == "java/lang/Object"
            || s.name == "AnyRef"
            || s.name == "Any"
            || s.name == "AnyVal"
            || s.name == "Object"
        {
            continue;
        }
        if is_interface_sym(st, id) {
            ifaces.push(jvm);
        } else if !found_class {
            super_name = jvm;
            found_class = true;
        } else {
            ifaces.push(jvm);
        }
    }
    (super_name, ifaces)
}

fn field_access_flags(mods: Flags) -> u16 {
    let mut acc = if mods.contains(Flags::PRIVATE) {
        ACC_PRIVATE
    } else {
        ACC_PUBLIC
    };
    if !mods.contains(Flags::MUTABLE) {
        acc |= ACC_FINAL;
    }
    acc
}

fn method_access_flags(mods: Flags) -> u16 {
    if mods.contains(Flags::PRIVATE) {
        ACC_PRIVATE
    } else {
        ACC_PUBLIC
    }
}

fn peel_fun(tree: &Tree) -> &Tree {
    match &tree.kind {
        TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => peel_fun(fun),
        _ => tree,
    }
}

// ---------------------------------------------------------------------------
// walk
// ---------------------------------------------------------------------------

impl<'a> Gen<'a> {
    fn walk(&mut self, tree: &Tree) {
        match &tree.kind {
            TreeKind::PackageDef { stats, .. } => self.walk_stats(stats),
            TreeKind::ClassDef { .. } => {
                self.emit_class(tree, &HashSet::new());
            }
            TreeKind::ModuleDef { .. } => {
                self.emit_module(tree, &HashSet::new());
            }
            _ => {}
        }
    }

    fn walk_stats(&mut self, stats: &[Tree]) {
        let mut module_names = HashSet::new();
        let mut class_names = HashSet::new();
        for s in stats {
            match &s.kind {
                TreeKind::ModuleDef { name, .. } => {
                    module_names.insert(name.clone());
                }
                TreeKind::ClassDef { name, .. } => {
                    class_names.insert(name.clone());
                }
                _ => {}
            }
        }
        for s in stats {
            match &s.kind {
                TreeKind::PackageDef { .. } => self.walk(s),
                TreeKind::ClassDef { name, mods, .. } => {
                    self.emit_class(s, &module_names);
                    if mods.flags.contains(Flags::CASE) && !module_names.contains(name) {
                        self.emit_case_companion(s);
                    }
                }
                TreeKind::ModuleDef { .. } => self.emit_module(s, &class_names),
                _ => {}
            }
        }
    }

    fn emit_class(&mut self, tree: &Tree, _module_names: &HashSet<String>) {
        let (name, mods, vparamss, impl_) = match &tree.kind {
            TreeKind::ClassDef {
                name,
                mods,
                vparamss,
                impl_,
                ..
            } => (name, mods, vparamss, impl_),
            _ => return,
        };
        let class_id = tree.sym;
        let this_name = if class_id.is_none() {
            name.clone()
        } else {
            class_internal(self.st, class_id)
        };
        let is_trait = mods.flags.contains(Flags::TRAIT);
        let (super_name, interfaces) = split_parents(self.st, &impl_.parents);

        let concrete = impl_.body.iter().any(|s| match &s.kind {
            TreeKind::DefDef { rhs, .. } => !rhs.is_empty(),
            TreeKind::ValDef { rhs, .. } => !rhs.is_empty(),
            _ => false,
        });

        let mut b = ClassBuilder::new(this_name, self.source_name);
        b.super_name = super_name;
        b.interfaces = interfaces;

        if is_trait && !concrete {
            b.access = ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT;
            for stt in &impl_.body {
                if let TreeKind::DefDef { name, mods, .. } = &stt.kind {
                    let acc = method_access_flags(mods.flags) | ACC_ABSTRACT;
                    b.add_abstract(acc, name, &def_method_desc(self.st, stt));
                }
            }
            self.out.push(b.finish());
            return;
        }

        b.access = ACC_PUBLIC | ACC_SUPER;
        if mods.flags.contains(Flags::FINAL) {
            b.access |= ACC_FINAL;
        }
        if is_trait {
            b.access |= ACC_ABSTRACT;
        }

        // constructor / body fields
        for clause in vparamss {
            for p in clause {
                if let TreeKind::ValDef { name, mods, .. } = &p.kind {
                    let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                        self.st.get(p.sym).ty.clone()
                    } else {
                        p.ty.clone()
                    };
                    b.fields.push(Field {
                        access: field_access_flags(mods.flags),
                        name: name.clone(),
                        desc: jvm_desc(self.st, &ty),
                    });
                }
            }
        }
        for stt in &impl_.body {
            if let TreeKind::ValDef { name, mods, .. } = &stt.kind {
                let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                    self.st.get(stt.sym).ty.clone()
                } else {
                    stt.ty.clone()
                };
                b.fields.push(Field {
                    access: field_access_flags(mods.flags),
                    name: name.clone(),
                    desc: jvm_desc(self.st, &ty),
                });
            }
        }

        if !is_trait {
            self.emit_class_ctor(&mut b, class_id, vparamss, &impl_.body);
        }
        for stt in &impl_.body {
            if matches!(stt.kind, TreeKind::DefDef { .. }) {
                self.emit_def(&mut b, class_id, stt);
            }
        }
        self.out.push(b.finish());
    }

    fn emit_class_ctor(
        &self,
        b: &mut ClassBuilder,
        class_id: SymbolId,
        vparamss: &[Vec<Tree>],
        body: &[Tree],
    ) {
        let params: Vec<&Tree> = vparamss.iter().flatten().collect();
        let mut frame = Frame::instance();
        let mut param_info = Vec::new();
        for p in &params {
            let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                self.st.get(p.sym).ty.clone()
            } else {
                p.ty.clone()
            };
            let sort = jvm_sort(&ty);
            let slot = frame.alloc(p.sym, sort);
            let fname = p.name().unwrap_or("").to_string();
            param_info.push((slot, sort, fname, jvm_desc(self.st, &ty)));
        }
        let desc = if params.is_empty() {
            "()V".to_string()
        } else {
            let types: Vec<Type> = params
                .iter()
                .map(|p| {
                    if p.ty.is_no_type() && !p.sym.is_none() {
                        self.st.get(p.sym).ty.clone()
                    } else {
                        p.ty.clone()
                    }
                })
                .collect();
            jvm_method_desc(self.st, &types, &Type::Unit)
        };
        let super_name = b.super_name.clone();
        let class_name = b.this_name.clone();
        let st = self.st;
        let inits: Vec<&Tree> = body
            .iter()
            .filter(|t| matches!(t.kind, TreeKind::ValDef { .. }))
            .collect();
        let max_locals = frame.next_slot;
        b.add_code(ACC_PUBLIC, "<init>", &desc, max_locals, |asm| {
            let mut frame = frame;
            asm.aload(0);
            asm.invokespecial(&super_name, "<init>", "()V");
            for (slot, sort, fname, fdesc) in &param_info {
                if fname.is_empty() {
                    continue;
                }
                asm.aload(0);
                load(asm, *slot, *sort);
                asm.putfield(&class_name, fname, fdesc);
            }
            let ctx = EmitCtx {
                st,
                class_sym: class_id,
                class_name: &class_name,
                ret_ty: Type::Unit,
            };
            for vd in &inits {
                if let TreeKind::ValDef { name, rhs, .. } = &vd.kind {
                    if rhs.is_empty() {
                        continue;
                    }
                    asm.aload(0);
                    gen_expr(asm, &mut frame, &ctx, rhs);
                    let ty = if vd.ty.is_no_type() && !vd.sym.is_none() {
                        st.get(vd.sym).ty.clone()
                    } else {
                        vd.ty.clone()
                    };
                    asm.putfield(&class_name, name, &jvm_desc(st, &ty));
                }
            }
            asm.vreturn();
        });
    }

    fn emit_def(&self, b: &mut ClassBuilder, class_id: SymbolId, def: &Tree) {
        let (name, mods, vparamss, rhs) = match &def.kind {
            TreeKind::DefDef {
                name,
                mods,
                vparamss,
                rhs,
                ..
            } => (name, mods, vparamss, rhs),
            _ => return,
        };
        if name == "<init>" || name == "<clinit>" {
            return;
        }
        let desc = def_method_desc(self.st, def);
        let ret = method_ret_ty(def);
        let acc = method_access_flags(mods.flags);
        if rhs.is_empty() {
            b.add_abstract(acc | ACC_ABSTRACT, name, &desc);
            return;
        }
        let mut frame = Frame::instance();
        for clause in vparamss {
            for p in clause {
                let ty = if p.ty.is_no_type() && !p.sym.is_none() {
                    self.st.get(p.sym).ty.clone()
                } else {
                    p.ty.clone()
                };
                frame.alloc(p.sym, jvm_sort(&ty));
            }
        }
        let class_name = b.this_name.clone();
        let st = self.st;
        let max_locals = frame.next_slot;
        let ret_for_body = ret.clone();
        b.add_code(acc, name, &desc, max_locals, |asm| {
            let mut frame = frame;
            let ctx = EmitCtx {
                st,
                class_sym: class_id,
                class_name: &class_name,
                ret_ty: ret_for_body.clone(),
            };
            gen_expr(asm, &mut frame, &ctx, rhs);
            if is_unit_like(&ret_for_body) {
                pop_if_value(asm, &rhs.ty);
                asm.vreturn();
            } else {
                emit_return(asm, &ret_for_body);
            }
        });
    }

    fn emit_module(&mut self, tree: &Tree, class_names: &HashSet<String>) {
        let (name, impl_) = match &tree.kind {
            TreeKind::ModuleDef { name, impl_, .. } => (name, impl_),
            _ => return,
        };
        let m = tree.sym;
        let cls = if m.is_none() {
            m
        } else {
            module_class_id(self.st, m)
        };
        let this_name = if cls.is_none() {
            format!("{name}$")
        } else {
            class_internal(self.st, cls)
        };

        let mut b = ClassBuilder::new(this_name.clone(), self.source_name);
        b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
        b.fields.push(Field {
            access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
            name: "MODULE$".into(),
            desc: format!("L{this_name};"),
        });
        for stt in &impl_.body {
            if let TreeKind::ValDef { name, mods, .. } = &stt.kind {
                let ty = if stt.ty.is_no_type() && !stt.sym.is_none() {
                    self.st.get(stt.sym).ty.clone()
                } else {
                    stt.ty.clone()
                };
                b.fields.push(Field {
                    access: field_access_flags(mods.flags),
                    name: name.clone(),
                    desc: jvm_desc(self.st, &ty),
                });
            }
        }

        self.emit_module_init(&mut b, cls, &impl_.body);
        self.emit_module_clinit(&mut b);

        let mut forwarded: Vec<(String, String, Type, Vec<Type>)> = Vec::new();
        for stt in &impl_.body {
            if matches!(stt.kind, TreeKind::DefDef { .. }) {
                self.emit_def(&mut b, cls, stt);
                if let TreeKind::DefDef { name, mods, .. } = &stt.kind {
                    if !mods.flags.contains(Flags::PRIVATE) {
                        forwarded.push((
                            name.clone(),
                            def_method_desc(self.st, stt),
                            method_ret_ty(stt),
                            def_param_types(self.st, stt),
                        ));
                    }
                }
            }
        }

        // case-class companion: synthetic apply
        if let Some(class_id) = self.find_class_named(name) {
            if self.st.get(class_id).flags.contains(Flags::CASE)
                && !impl_.body.iter().any(|t| t.name() == Some("apply"))
            {
                emit_case_apply(&mut b, self.st, class_id);
                let fields = self.st.get(class_id).ctor_fields.clone();
                let pts: Vec<Type> = fields.iter().map(|f| self.st.get(*f).ty.clone()).collect();
                let ret = Type::Class {
                    sym: class_id,
                    args: vec![],
                };
                forwarded.push((
                    "apply".into(),
                    jvm_method_desc(self.st, &pts, &ret),
                    ret,
                    pts,
                ));
            }
        }

        self.out.push(b.finish());

        if !class_names.contains(name) {
            self.emit_forwarder(&this_name, &forwarded);
        }
    }

    fn emit_module_init(&self, b: &mut ClassBuilder, class_id: SymbolId, body: &[Tree]) {
        let class_name = b.this_name.clone();
        let st = self.st;
        let inits: Vec<&Tree> = body
            .iter()
            .filter(|t| matches!(t.kind, TreeKind::ValDef { .. }))
            .collect();
        b.add_code(ACC_PRIVATE, "<init>", "()V", 1, |asm| {
            let mut frame = Frame::instance();
            asm.aload(0);
            asm.invokespecial("java/lang/Object", "<init>", "()V");
            asm.aload(0);
            asm.putstatic(&class_name, "MODULE$", &format!("L{class_name};"));
            let ctx = EmitCtx {
                st,
                class_sym: class_id,
                class_name: &class_name,
                ret_ty: Type::Unit,
            };
            for vd in &inits {
                if let TreeKind::ValDef { name, rhs, .. } = &vd.kind {
                    if rhs.is_empty() {
                        continue;
                    }
                    asm.aload(0);
                    gen_expr(asm, &mut frame, &ctx, rhs);
                    let ty = if vd.ty.is_no_type() && !vd.sym.is_none() {
                        st.get(vd.sym).ty.clone()
                    } else {
                        vd.ty.clone()
                    };
                    asm.putfield(&class_name, name, &jvm_desc(st, &ty));
                }
            }
            asm.vreturn();
        });
    }

    fn emit_module_clinit(&self, b: &mut ClassBuilder) {
        let class_name = b.this_name.clone();
        b.add_code(ACC_STATIC, "<clinit>", "()V", 1, |asm| {
            asm.new_obj(&class_name);
            asm.dup();
            asm.invokespecial(&class_name, "<init>", "()V");
            asm.pop();
            asm.vreturn();
        });
    }

    fn emit_case_companion(&mut self, class_tree: &Tree) {
        let class_id = class_tree.sym;
        let class_jvm = if class_id.is_none() {
            class_tree.name().unwrap_or("X").to_string()
        } else {
            class_internal(self.st, class_id)
        };
        let this_name = format!("{class_jvm}$");
        let mut b = ClassBuilder::new(this_name.clone(), self.source_name);
        b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
        b.fields.push(Field {
            access: ACC_PUBLIC | ACC_STATIC | ACC_FINAL,
            name: "MODULE$".into(),
            desc: format!("L{this_name};"),
        });
        self.emit_module_init(&mut b, class_id, &[]);
        self.emit_module_clinit(&mut b);
        emit_case_apply(&mut b, self.st, class_id);
        self.out.push(b.finish());
    }

    fn emit_forwarder(&mut self, module_jvm: &str, methods: &[(String, String, Type, Vec<Type>)]) {
        let fwd_name = strip_module_dollar(module_jvm);
        let mut b = ClassBuilder::new(fwd_name, self.source_name);
        b.access = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;
        let module_desc = format!("L{module_jvm};");
        for (name, desc, ret, params) in methods {
            let mut locals = 0u16;
            let mut loads = Vec::new();
            for p in params {
                let sort = jvm_sort(p);
                loads.push((locals, sort));
                locals += sort.slots();
            }
            let max_locals = locals.max(1);
            let ret = ret.clone();
            let name = name.clone();
            let desc = desc.clone();
            let module_jvm = module_jvm.to_string();
            let module_desc = module_desc.clone();
            b.add_code(ACC_PUBLIC | ACC_STATIC, &name, &desc, max_locals, |asm| {
                asm.getstatic(&module_jvm, "MODULE$", &module_desc);
                for (slot, sort) in &loads {
                    load(asm, *slot, *sort);
                }
                asm.invokevirtual(&module_jvm, &name, &desc);
                emit_return(asm, &ret);
            });
        }
        self.out.push(b.finish());
    }

    fn find_class_named(&self, name: &str) -> Option<SymbolId> {
        self.st.symbols.iter().find_map(|s| {
            if s.kind == SymKind::Class && s.name == name && !s.flags.contains(Flags::TRAIT) {
                Some(s.id)
            } else {
                None
            }
        })
    }
}

fn emit_case_apply(b: &mut ClassBuilder, st: &SymbolTable, class_id: SymbolId) {
    let fields = st.get(class_id).ctor_fields.clone();
    let class_jvm = class_internal(st, class_id);
    let mut params = Vec::new();
    let mut locals = 1u16;
    let mut loads = Vec::new();
    for f in &fields {
        let ty = st.get(*f).ty.clone();
        let sort = jvm_sort(&ty);
        loads.push((locals, sort));
        locals += sort.slots();
        params.push(ty);
    }
    let ret = Type::Class {
        sym: class_id,
        args: vec![],
    };
    let desc = jvm_method_desc(st, &params, &ret);
    let ctor_d = jvm_method_desc(st, &params, &Type::Unit);
    b.add_code(ACC_PUBLIC, "apply", &desc, locals.max(1), |asm| {
        asm.new_obj(&class_jvm);
        asm.dup();
        for (slot, sort) in &loads {
            load(asm, *slot, *sort);
        }
        asm.invokespecial(&class_jvm, "<init>", &ctor_d);
        asm.areturn();
    });
}

// ---------------------------------------------------------------------------
// bytecode helpers
// ---------------------------------------------------------------------------

fn load(asm: &mut Assembler, slot: u16, sort: JvmSort) {
    match sort {
        JvmSort::Int => asm.iload(slot),
        JvmSort::Long => asm.lload(slot),
        JvmSort::Double => asm.dload(slot),
        JvmSort::Float => asm.iload(slot),
        JvmSort::Ref => asm.aload(slot),
        JvmSort::Void => {}
    }
}

fn store(asm: &mut Assembler, slot: u16, sort: JvmSort) {
    match sort {
        JvmSort::Int => asm.istore(slot),
        JvmSort::Long => asm.lstore(slot),
        JvmSort::Double => asm.dstore(slot),
        JvmSort::Float => asm.istore(slot),
        JvmSort::Ref => asm.astore(slot),
        JvmSort::Void => {}
    }
}

fn pop_if_value(asm: &mut Assembler, ty: &Type) {
    match jvm_sort(ty) {
        JvmSort::Void => {}
        JvmSort::Long | JvmSort::Double => asm.pop2(),
        _ => asm.pop(),
    }
}

fn emit_return(asm: &mut Assembler, ty: &Type) {
    match jvm_sort(ty) {
        JvmSort::Void => asm.vreturn(),
        JvmSort::Int => asm.ireturn(),
        JvmSort::Long => asm.lreturn(),
        JvmSort::Double => asm.dreturn(),
        JvmSort::Float => asm.ireturn(),
        JvmSort::Ref => asm.areturn(),
    }
}

fn throw_runtime(asm: &mut Assembler, msg: &str) {
    asm.new_obj("java/lang/RuntimeException");
    asm.dup();
    asm.ldc_string(msg);
    asm.invokespecial(
        "java/lang/RuntimeException",
        "<init>",
        "(Ljava/lang/String;)V",
    );
    asm.athrow();
}

fn push_default(asm: &mut Assembler, ty: &Type) {
    match jvm_sort(ty) {
        JvmSort::Void => {}
        JvmSort::Int => asm.iconst(0),
        JvmSort::Long => asm.lconst(0),
        JvmSort::Double => asm.dconst(0.0),
        JvmSort::Float => asm.iconst(0),
        JvmSort::Ref => asm.aconst_null(),
    }
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

fn gen_stat(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    match &tree.kind {
        TreeKind::ValDef { rhs, .. } => {
            let ty = if tree.ty.is_no_type() && !tree.sym.is_none() {
                ctx.st.get(tree.sym).ty.clone()
            } else {
                tree.ty.clone()
            };
            let sort = jvm_sort(&ty);
            if rhs.is_empty() {
                frame.alloc(tree.sym, sort);
                return;
            }
            if sort == JvmSort::Void {
                gen_stat(asm, frame, ctx, rhs);
                frame.alloc(tree.sym, sort);
                return;
            }
            gen_expr(asm, frame, ctx, rhs);
            let slot = frame.alloc(tree.sym, sort);
            store(asm, slot, sort);
        }
        TreeKind::DefDef { .. } | TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => {
            // nested member: not lifted in this pass
        }
        TreeKind::Empty => {}
        _ => {
            gen_expr(asm, frame, ctx, tree);
            pop_if_value(asm, &tree.ty);
        }
    }
}

fn gen_expr(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    match &tree.kind {
        TreeKind::Empty => {}
        TreeKind::Literal { lit } => gen_literal(asm, lit),
        TreeKind::This { .. } => asm.aload(0),
        TreeKind::Ident { .. } => gen_ident(asm, frame, ctx, tree),
        TreeKind::Select { qual, name } => gen_select(asm, frame, ctx, tree, qual, name),
        TreeKind::Apply { fun, args } => gen_apply(asm, frame, ctx, tree, fun, args),
        TreeKind::TypeApply { fun, .. } => gen_expr(asm, frame, ctx, fun),
        TreeKind::Typed { expr, .. } => {
            gen_expr(asm, frame, ctx, expr);
            // optional checkcast to a class type
            if let Type::Class { sym, .. } = &tree.ty {
                if !is_interface_sym(ctx.st, *sym) {
                    // skip; subclass assignment does not need checkcast
                }
            }
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                gen_stat(asm, frame, ctx, s);
            }
            gen_expr(asm, frame, ctx, expr);
        }
        TreeKind::If { cond, thenp, elsep } => {
            gen_if(asm, frame, ctx, cond, thenp, elsep, &tree.ty);
        }
        TreeKind::While { cond, body } => {
            let start = asm.fresh_label();
            let end = asm.fresh_label();
            asm.mark(start);
            gen_expr(asm, frame, ctx, cond);
            asm.ifeq(end);
            gen_stat(asm, frame, ctx, body);
            asm.goto(start);
            asm.mark(end);
        }
        TreeKind::DoWhile { cond, body } => {
            let start = asm.fresh_label();
            asm.mark(start);
            gen_stat(asm, frame, ctx, body);
            gen_expr(asm, frame, ctx, cond);
            asm.ifne(start);
        }
        TreeKind::Assign { lhs, rhs } => gen_assign(asm, frame, ctx, lhs, rhs),
        TreeKind::Match { selector, cases } => {
            gen_match(asm, frame, ctx, selector, cases, &tree.ty);
        }
        TreeKind::New { tpt } => gen_new(asm, frame, ctx, tpt, &[]),
        TreeKind::Return { expr } => {
            if !expr.is_empty() && !is_unit_like(&expr.ty) && !is_unit_like(&ctx.ret_ty) {
                gen_expr(asm, frame, ctx, expr);
            } else if !expr.is_empty() && !is_unit_like(&ctx.ret_ty) {
                gen_expr(asm, frame, ctx, expr);
            } else if !expr.is_empty() {
                gen_expr(asm, frame, ctx, expr);
                pop_if_value(asm, &expr.ty);
            }
            emit_return(asm, &ctx.ret_ty);
            // keep stack consistent for later (dead) code
            push_default(asm, &tree.ty);
        }
        TreeKind::Throw { expr } => {
            gen_expr(asm, frame, ctx, expr);
            asm.athrow();
            push_default(asm, &tree.ty);
        }
        TreeKind::InterpolatedString { parts, args, .. } => {
            gen_interpolated(asm, frame, ctx, parts, args);
        }
        TreeKind::ValDef { .. } => {
            gen_stat(asm, frame, ctx, tree);
        }
        _ => {
            throw_runtime(
                asm,
                &format!(
                    "unimplemented expression: {}",
                    tree.name().unwrap_or("<tree>")
                ),
            );
            push_default(asm, &tree.ty);
        }
    }
}

fn gen_literal(asm: &mut Assembler, lit: &Lit) {
    match lit {
        Lit::Unit => {}
        Lit::Boolean(b) => asm.iconst(if *b { 1 } else { 0 }),
        Lit::Int(n) => asm.iconst(*n),
        Lit::Long(n) => asm.lconst(*n),
        Lit::Float(_) => asm.iconst(0),
        Lit::Double(n) => asm.dconst(*n),
        Lit::Char(c) => asm.iconst(*c as i32),
        Lit::String(s) => asm.ldc_string(s),
        Lit::Null => asm.aconst_null(),
        Lit::Symbol(s) => asm.ldc_string(s),
    }
}

fn gen_ident(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tree: &Tree) {
    let id = tree.sym;
    if id.is_none() {
        throw_runtime(
            asm,
            &format!("unresolved ident {}", tree.name().unwrap_or("?")),
        );
        push_default(asm, &tree.ty);
        return;
    }
    if let Some((slot, sort)) = frame.get(id) {
        load(asm, slot, sort);
        return;
    }
    let sym = ctx.st.get(id);
    match sym.kind {
        SymKind::Term => {
            let owner = class_internal(ctx.st, sym.owner);
            let desc = jvm_desc(ctx.st, &sym.ty);
            asm.aload(0);
            asm.getfield(&owner, &sym.name, &desc);
        }
        SymKind::Module | SymKind::ModuleClass => {
            let jvm = class_internal(ctx.st, module_class_id(ctx.st, id));
            asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
        }
        SymKind::Class => {
            let jvm = format!("{}$", class_internal(ctx.st, id));
            asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
        }
        SymKind::Method => {
            // parameterless / empty-clause call on this
            asm.aload(0);
            invoke_method(asm, ctx, id);
        }
        _ => {
            throw_runtime(asm, &format!("cannot load {}", sym.name));
            push_default(asm, &tree.ty);
        }
    }
}

fn gen_select(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    tree: &Tree,
    qual: &Tree,
    name: &str,
) {
    if name == "length" && matches!(qual.ty, Type::Array(_)) {
        gen_expr(asm, frame, ctx, qual);
        asm.arraylength();
        return;
    }
    if !tree.sym.is_none() {
        let s = ctx.st.get(tree.sym);
        match s.kind {
            SymKind::Term => {
                gen_expr(asm, frame, ctx, qual);
                let owner = class_internal(ctx.st, s.owner);
                asm.getfield(&owner, &s.name, &jvm_desc(ctx.st, &s.ty));
                return;
            }
            SymKind::Method => {
                gen_expr(asm, frame, ctx, qual);
                invoke_method(asm, ctx, tree.sym);
                return;
            }
            SymKind::Module | SymKind::ModuleClass => {
                let jvm = class_internal(ctx.st, module_class_id(ctx.st, tree.sym));
                asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
                return;
            }
            _ => {}
        }
    }
    // field by name on qualifier's class
    if let Some(cid) = ctx.st.class_sym_of(&qual.ty) {
        gen_expr(asm, frame, ctx, qual);
        let owner = class_internal(ctx.st, cid);
        let desc = jvm_desc(ctx.st, &tree.ty);
        asm.getfield(&owner, name, &desc);
        return;
    }
    throw_runtime(asm, &format!("select {name}"));
    push_default(asm, &tree.ty);
}

fn gen_assign(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, lhs: &Tree, rhs: &Tree) {
    match &lhs.kind {
        TreeKind::Ident { .. } => {
            let id = lhs.sym;
            if let Some((slot, sort)) = frame.get(id) {
                gen_expr(asm, frame, ctx, rhs);
                store(asm, slot, sort);
                return;
            }
            if !id.is_none() {
                let s = ctx.st.get(id);
                asm.aload(0);
                gen_expr(asm, frame, ctx, rhs);
                asm.putfield(
                    &class_internal(ctx.st, s.owner),
                    &s.name,
                    &jvm_desc(ctx.st, &s.ty),
                );
                return;
            }
            gen_expr(asm, frame, ctx, rhs);
            pop_if_value(asm, &rhs.ty);
        }
        TreeKind::Select { qual, name } => {
            gen_expr(asm, frame, ctx, qual);
            gen_expr(asm, frame, ctx, rhs);
            let owner = if !lhs.sym.is_none() {
                class_internal(ctx.st, ctx.st.get(lhs.sym).owner)
            } else if let Some(cid) = ctx.st.class_sym_of(&qual.ty) {
                class_internal(ctx.st, cid)
            } else {
                ctx.class_name.to_string()
            };
            let desc = if !lhs.ty.is_no_type() {
                jvm_desc(ctx.st, &lhs.ty)
            } else {
                jvm_desc(ctx.st, &rhs.ty)
            };
            asm.putfield(&owner, name, &desc);
        }
        _ => {
            gen_expr(asm, frame, ctx, rhs);
            pop_if_value(asm, &rhs.ty);
        }
    }
}

fn gen_if(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    cond: &Tree,
    thenp: &Tree,
    elsep: &Tree,
    result_ty: &Type,
) {
    gen_expr(asm, frame, ctx, cond);
    let else_l = asm.fresh_label();
    let end_l = asm.fresh_label();
    asm.ifeq(else_l);
    if is_unit_like(result_ty) {
        gen_stat(asm, frame, ctx, thenp);
    } else {
        gen_expr(asm, frame, ctx, thenp);
    }
    asm.goto(end_l);
    asm.mark(else_l);
    if is_unit_like(result_ty) {
        gen_stat(asm, frame, ctx, elsep);
    } else {
        gen_expr(asm, frame, ctx, elsep);
    }
    asm.mark(end_l);
}

fn gen_new(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, tpt: &Tree, args: &[Tree]) {
    let class_id = tpt
        .sym
        .is_none()
        .then(|| ctx.st.class_sym_of(&tpt.ty))
        .flatten()
        .or(if tpt.sym.is_none() {
            None
        } else {
            Some(tpt.sym)
        })
        .or_else(|| ctx.st.class_sym_of(&tpt.ty))
        .unwrap_or(tpt.sym);
    let internal = if class_id.is_none() {
        tpt.name().unwrap_or("java/lang/Object").to_string()
    } else {
        class_internal(ctx.st, class_id)
    };
    let desc = if class_id.is_none() {
        let pts: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
        jvm_method_desc(ctx.st, &pts, &Type::Unit)
    } else {
        ctor_desc(ctx.st, class_id, args)
    };
    asm.new_obj(&internal);
    asm.dup();
    for a in args {
        gen_expr(asm, frame, ctx, a);
    }
    asm.invokespecial(&internal, "<init>", &desc);
}

fn gen_apply(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    tree: &Tree,
    fun: &Tree,
    args: &[Tree],
) {
    let fun = peel_fun(fun);

    if matches!(&fun.kind, TreeKind::New { .. }) {
        let tpt = match &fun.kind {
            TreeKind::New { tpt } => tpt,
            _ => unreachable!(),
        };
        gen_new(asm, frame, ctx, tpt, args);
        return;
    }

    let ic = if !fun.sym.is_none() {
        ctx.st.get(fun.sym).intrinsic
    } else {
        Intrinsic::None
    };

    if matches!(ic, Intrinsic::Println) || fun.name() == Some("println") {
        gen_println(asm, frame, ctx, args, true);
        return;
    }
    if matches!(ic, Intrinsic::Print) || fun.name() == Some("print") {
        gen_println(asm, frame, ctx, args, false);
        return;
    }

    if let TreeKind::Select { qual, name } = &fun.kind {
        match ic {
            Intrinsic::IntBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                } else {
                    asm.iconst(0);
                }
                emit_int_bin(asm, op);
                return;
            }
            Intrinsic::IntUn(op) => {
                gen_expr(asm, frame, ctx, qual);
                match op {
                    "-" => asm.ineg(),
                    "~" => {
                        asm.iconst(-1);
                        asm.ixor();
                    }
                    _ => {}
                }
                return;
            }
            Intrinsic::LongBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                }
                emit_long_bin(asm, op);
                return;
            }
            Intrinsic::LongUn("-") => {
                gen_expr(asm, frame, ctx, qual);
                asm.lneg();
                return;
            }
            Intrinsic::DoubleBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                }
                emit_double_bin(asm, op);
                return;
            }
            Intrinsic::DoubleUn("-") => {
                gen_expr(asm, frame, ctx, qual);
                asm.dneg();
                return;
            }
            Intrinsic::BoolBin("&&") => {
                gen_bool_and(asm, frame, ctx, qual, args.first());
                return;
            }
            Intrinsic::BoolBin("||") => {
                gen_bool_or(asm, frame, ctx, qual, args.first());
                return;
            }
            Intrinsic::BoolBin(op) => {
                gen_expr(asm, frame, ctx, qual);
                if let Some(r) = args.first() {
                    gen_expr(asm, frame, ctx, r);
                }
                emit_int_cmp(asm, op);
                return;
            }
            Intrinsic::BoolUn("!") => {
                gen_expr(asm, frame, ctx, qual);
                asm.iconst(1);
                asm.ixor();
                return;
            }
            Intrinsic::StringConcat => {
                if let Some(r) = args.first() {
                    gen_string_concat(asm, frame, ctx, qual, r);
                } else {
                    gen_expr(asm, frame, ctx, qual);
                }
                return;
            }
            Intrinsic::AnyToString => {
                gen_expr(asm, frame, ctx, qual);
                asm.invokevirtual("java/lang/Object", "toString", "()Ljava/lang/String;");
                return;
            }
            Intrinsic::Identity => {
                gen_expr(asm, frame, ctx, qual);
                return;
            }
            Intrinsic::IntToLong => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2l();
                return;
            }
            Intrinsic::IntToDouble => {
                gen_expr(asm, frame, ctx, qual);
                asm.i2d();
                return;
            }
            Intrinsic::LongToDouble => {
                gen_expr(asm, frame, ctx, qual);
                asm.l2d();
                return;
            }
            _ => {}
        }

        if name == "+" && matches!(tree.ty, Type::String) {
            if let Some(r) = args.first() {
                gen_string_concat(asm, frame, ctx, qual, r);
                return;
            }
        }

        // name-based int ops if typer did not attach an intrinsic
        if args.len() == 1 && matches!(qual.ty, Type::Int) && matches!(args[0].ty, Type::Int) {
            if matches!(
                name.as_str(),
                "+" | "-" | "*" | "/" | "%" | "==" | "!=" | "<" | "<=" | ">" | ">="
            ) {
                gen_expr(asm, frame, ctx, qual);
                gen_expr(asm, frame, ctx, &args[0]);
                emit_int_bin(asm, name);
                return;
            }
        }
    }

    // regular method / apply
    if fun.sym.is_none() {
        throw_runtime(asm, "unresolved apply");
        push_default(asm, &tree.ty);
        return;
    }

    gen_receiver(asm, frame, ctx, fun);
    for a in args {
        gen_expr(asm, frame, ctx, a);
    }
    invoke_method(asm, ctx, fun.sym);
}

fn gen_receiver(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, fun: &Tree) {
    match &fun.kind {
        TreeKind::Select { qual, .. } => {
            gen_expr(asm, frame, ctx, qual);
        }
        _ => {
            if fun.sym.is_none() {
                asm.aload(0);
                return;
            }
            let owner = ctx.st.get(fun.sym).owner;
            if owner == ctx.class_sym || owner.is_none() {
                asm.aload(0);
            } else if is_module_class(ctx.st, owner) {
                let jvm = class_internal(ctx.st, module_class_id(ctx.st, owner));
                asm.getstatic(&jvm, "MODULE$", &format!("L{jvm};"));
            } else {
                // method on current instance (nested owner mismatch)
                asm.aload(0);
            }
        }
    }
}

fn invoke_method(asm: &mut Assembler, ctx: &EmitCtx, id: SymbolId) {
    let s = ctx.st.get(id);
    let owner_id = s.owner;
    let owner = class_internal(ctx.st, owner_id);
    let desc = method_desc_from_sym(ctx.st, id);
    if is_interface_sym(ctx.st, owner_id) {
        asm.invokeinterface(&owner, &s.name, &desc);
    } else {
        asm.invokevirtual(&owner, &s.name, &desc);
    }
}

fn gen_println(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    args: &[Tree],
    newline: bool,
) {
    asm.getstatic("java/lang/System", "out", "Ljava/io/PrintStream;");
    let name = if newline { "println" } else { "print" };
    if args.is_empty() {
        asm.invokevirtual("java/io/PrintStream", name, "()V");
        return;
    }
    let arg = &args[0];
    match &arg.ty {
        Type::Unit | Type::NoType => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "()V");
        }
        Type::Int => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(I)V");
        }
        Type::Long => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(J)V");
        }
        Type::Double => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(D)V");
        }
        Type::Boolean => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(Z)V");
        }
        Type::String => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(Ljava/lang/String;)V");
        }
        Type::Char => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(C)V");
        }
        Type::Float => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(F)V");
        }
        _ => {
            gen_expr(asm, frame, ctx, arg);
            asm.invokevirtual("java/io/PrintStream", name, "(Ljava/lang/Object;)V");
        }
    }
}

fn emit_int_bin(asm: &mut Assembler, op: &str) {
    match op {
        "+" => asm.iadd(),
        "-" => asm.isub(),
        "*" => asm.imul(),
        "/" => asm.idiv(),
        "%" => asm.irem(),
        "&" => asm.iand(),
        "|" => asm.ior(),
        "^" => asm.ixor(),
        "<<" => asm.ishl(),
        ">>" => asm.ishr(),
        ">>>" => asm.iushr(),
        "==" | "!=" | "<" | "<=" | ">" | ">=" => emit_int_cmp(asm, op),
        _ => {}
    }
}

fn emit_int_cmp(asm: &mut Assembler, op: &str) {
    let t = asm.fresh_label();
    let e = asm.fresh_label();
    match op {
        "==" => asm.if_icmpeq(t),
        "!=" => asm.if_icmpne(t),
        "<" => asm.if_icmplt(t),
        "<=" => asm.if_icmple(t),
        ">" => asm.if_icmpgt(t),
        ">=" => asm.if_icmpge(t),
        _ => asm.if_icmpeq(t),
    }
    asm.iconst(0);
    asm.goto(e);
    asm.mark(t);
    asm.iconst(1);
    asm.mark(e);
}

fn emit_long_bin(asm: &mut Assembler, op: &str) {
    match op {
        "+" => asm.ladd(),
        "-" => asm.lsub(),
        "*" => asm.lmul(),
        "/" => asm.ldiv(),
        _ => {}
    }
}

fn emit_double_bin(asm: &mut Assembler, op: &str) {
    match op {
        "+" => asm.dadd(),
        "-" => asm.dsub(),
        "*" => asm.dmul(),
        "/" => asm.ddiv(),
        _ => {}
    }
}

fn gen_bool_and(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    left: &Tree,
    right: Option<&Tree>,
) {
    gen_expr(asm, frame, ctx, left);
    let skip = asm.fresh_label();
    asm.dup();
    asm.ifeq(skip);
    asm.pop();
    if let Some(r) = right {
        gen_expr(asm, frame, ctx, r);
    } else {
        asm.iconst(0);
    }
    asm.mark(skip);
}

fn gen_bool_or(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    left: &Tree,
    right: Option<&Tree>,
) {
    gen_expr(asm, frame, ctx, left);
    let skip = asm.fresh_label();
    asm.dup();
    asm.ifne(skip);
    asm.pop();
    if let Some(r) = right {
        gen_expr(asm, frame, ctx, r);
    } else {
        asm.iconst(1);
    }
    asm.mark(skip);
}

fn gen_string_concat(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    left: &Tree,
    right: &Tree,
) {
    asm.new_obj("java/lang/StringBuilder");
    asm.dup();
    asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
    gen_sb_append(asm, frame, ctx, left);
    gen_sb_append(asm, frame, ctx, right);
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "toString",
        "()Ljava/lang/String;",
    );
}

fn gen_interpolated(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    parts: &[String],
    args: &[Tree],
) {
    asm.new_obj("java/lang/StringBuilder");
    asm.dup();
    asm.invokespecial("java/lang/StringBuilder", "<init>", "()V");
    for i in 0..args.len() {
        if i < parts.len() {
            sb_append_string(asm, &parts[i]);
        }
        gen_sb_append(asm, frame, ctx, &args[i]);
    }
    if parts.len() > args.len() {
        sb_append_string(asm, &parts[args.len()]);
    }
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "toString",
        "()Ljava/lang/String;",
    );
}

fn sb_append_string(asm: &mut Assembler, s: &str) {
    if s.is_empty() {
        return;
    }
    asm.ldc_string(s);
    asm.invokevirtual(
        "java/lang/StringBuilder",
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
    );
}

fn gen_sb_append(asm: &mut Assembler, frame: &mut Frame, ctx: &EmitCtx, value: &Tree) {
    let desc = match &value.ty {
        Type::Unit | Type::NoType => {
            asm.ldc_string("()");
            "(Ljava/lang/String;)Ljava/lang/StringBuilder;"
        }
        Type::Int => {
            gen_expr(asm, frame, ctx, value);
            "(I)Ljava/lang/StringBuilder;"
        }
        Type::Long => {
            gen_expr(asm, frame, ctx, value);
            "(J)Ljava/lang/StringBuilder;"
        }
        Type::Double => {
            gen_expr(asm, frame, ctx, value);
            "(D)Ljava/lang/StringBuilder;"
        }
        Type::Float => {
            gen_expr(asm, frame, ctx, value);
            "(F)Ljava/lang/StringBuilder;"
        }
        Type::Boolean => {
            gen_expr(asm, frame, ctx, value);
            "(Z)Ljava/lang/StringBuilder;"
        }
        Type::Char => {
            gen_expr(asm, frame, ctx, value);
            "(C)Ljava/lang/StringBuilder;"
        }
        Type::String => {
            gen_expr(asm, frame, ctx, value);
            "(Ljava/lang/String;)Ljava/lang/StringBuilder;"
        }
        _ => {
            gen_expr(asm, frame, ctx, value);
            "(Ljava/lang/Object;)Ljava/lang/StringBuilder;"
        }
    };
    asm.invokevirtual("java/lang/StringBuilder", "append", desc);
}

fn gen_match(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    selector: &Tree,
    cases: &[scala_rs_parser::CaseDef],
    result_ty: &Type,
) {
    gen_expr(asm, frame, ctx, selector);
    let sel_sort = jvm_sort(&selector.ty);
    let tmp = frame.alloc_tmp(sel_sort);
    store(asm, tmp, sel_sort);
    let end = asm.fresh_label();
    for c in cases {
        let fail = asm.fresh_label();
        gen_pattern(asm, frame, ctx, &c.pat, tmp, sel_sort, fail);
        if !c.guard.is_empty() {
            gen_expr(asm, frame, ctx, &c.guard);
            asm.ifeq(fail);
        }
        if is_unit_like(result_ty) {
            gen_stat(asm, frame, ctx, &c.body);
        } else {
            gen_expr(asm, frame, ctx, &c.body);
        }
        asm.goto(end);
        asm.mark(fail);
    }
    throw_runtime(asm, "match error");
    if !is_unit_like(result_ty) {
        push_default(asm, result_ty);
    }
    asm.mark(end);
}

fn gen_pattern(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    pat: &Tree,
    tmp: u16,
    sel_sort: JvmSort,
    fail: crate::code::Label,
) {
    match &pat.kind {
        TreeKind::Wildcard | TreeKind::Empty => {}
        TreeKind::Ident { name } => {
            let is_varid = name
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_');
            if is_varid || pat.sym.is_none() || ctx.st.get(pat.sym).kind == SymKind::Term {
                load(asm, tmp, sel_sort);
                let sort = jvm_sort(&pat.ty);
                let slot = if pat.sym.is_none() {
                    frame.alloc_tmp(sort)
                } else if let Some((s, _)) = frame.get(pat.sym) {
                    s
                } else {
                    frame.alloc(pat.sym, sort)
                };
                store(asm, slot, sort);
            } else {
                load(asm, tmp, sel_sort);
                gen_ident(asm, frame, ctx, pat);
                match sel_sort {
                    JvmSort::Int => asm.if_icmpne(fail),
                    JvmSort::Ref => {
                        let ok = asm.fresh_label();
                        // reference equality then equals
                        // stack: tmp, ident — use Object.equals
                        asm.invokevirtual("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
                        asm.ifne(ok);
                        asm.goto(fail);
                        asm.mark(ok);
                    }
                    _ => {
                        pop_if_value(asm, &pat.ty);
                        pop_if_value(asm, &pat.ty);
                    }
                }
            }
        }
        TreeKind::Literal { lit } => {
            load(asm, tmp, sel_sort);
            gen_literal(asm, lit);
            match sel_sort {
                JvmSort::Int => asm.if_icmpne(fail),
                JvmSort::Ref => {
                    asm.invokevirtual("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
                    asm.ifeq(fail);
                }
                _ => {
                    asm.pop();
                    asm.pop();
                }
            }
        }
        TreeKind::Apply { args, .. } => {
            let class_id = if pat.sym.is_none() {
                ctx.st.class_sym_of(&pat.ty).unwrap_or(SymbolId::NONE)
            } else {
                pat.sym
            };
            let jvm = if class_id.is_none() {
                pat.name().unwrap_or("java/lang/Object").to_string()
            } else {
                class_internal(ctx.st, class_id)
            };
            load(asm, tmp, JvmSort::Ref);
            asm.instanceof(&jvm);
            asm.ifeq(fail);
            let fields = if class_id.is_none() {
                Vec::new()
            } else {
                ctx.st.get(class_id).ctor_fields.clone()
            };
            for (i, a) in args.iter().enumerate() {
                if let Some(fid) = fields.get(i) {
                    let fs = ctx.st.get(*fid);
                    let fname = fs.name.clone();
                    let fty = fs.ty.clone();
                    let fdesc = jvm_desc(ctx.st, &fty);
                    load(asm, tmp, JvmSort::Ref);
                    asm.checkcast(&jvm);
                    asm.getfield(&jvm, &fname, &fdesc);
                    bind_subpattern(asm, frame, ctx, a, fail);
                } else {
                    throw_runtime(asm, "pattern arity");
                }
            }
        }
        TreeKind::Bind { body, .. } => {
            load(asm, tmp, sel_sort);
            let sort = jvm_sort(&pat.ty);
            let slot = if pat.sym.is_none() {
                frame.alloc_tmp(sort)
            } else {
                frame.alloc(pat.sym, sort)
            };
            store(asm, slot, sort);
            gen_pattern(asm, frame, ctx, body, tmp, sel_sort, fail);
        }
        TreeKind::Typed { expr, .. } => {
            gen_pattern(asm, frame, ctx, expr, tmp, sel_sort, fail);
        }
        _ => {}
    }
}

fn bind_subpattern(
    asm: &mut Assembler,
    frame: &mut Frame,
    ctx: &EmitCtx,
    pat: &Tree,
    fail: crate::code::Label,
) {
    // field value is on the stack
    match &pat.kind {
        TreeKind::Wildcard | TreeKind::Empty => {
            pop_if_value(asm, &pat.ty);
        }
        TreeKind::Ident { .. } | TreeKind::Bind { .. } => {
            let sort = jvm_sort(&pat.ty);
            let slot = if pat.sym.is_none() {
                frame.alloc_tmp(sort)
            } else if let Some((s, _)) = frame.get(pat.sym) {
                s
            } else {
                frame.alloc(pat.sym, sort)
            };
            store(asm, slot, sort);
        }
        TreeKind::Literal { lit } => {
            gen_literal(asm, lit);
            match jvm_sort(&pat.ty) {
                JvmSort::Int => asm.if_icmpne(fail),
                JvmSort::Ref => {
                    asm.invokevirtual("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
                    asm.ifeq(fail);
                }
                _ => {
                    asm.pop();
                    asm.pop();
                }
            }
        }
        TreeKind::Typed { expr, .. } => bind_subpattern(asm, frame, ctx, expr, fail),
        _ => {
            pop_if_value(asm, &pat.ty);
        }
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classfile::write_class_file;
    use scala_rs_typer::{has_errors, typecheck_str};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fresh_dir() -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "scala-rs-backend-{}-{}-{}",
            std::process::id(),
            n,
            nanos
        ));
        std::fs::create_dir_all(&p).expect("temp dir");
        TempDir(p)
    }

    fn java_available() -> bool {
        Command::new("java")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn write_classes(dir: &Path, classes: &[EmittedClass]) {
        for c in classes {
            let mut path = dir.to_path_buf();
            let parts: Vec<&str> = c
                .internal_name
                .split('/')
                .filter(|p| !p.is_empty())
                .collect();
            match parts.split_last() {
                Some((file, dirs)) => {
                    for d in dirs {
                        path.push(d);
                    }
                    path.push(format!("{file}.class"));
                }
                None => path.push(".class"),
            }
            write_class_file(&path, &c.bytes).expect("write class");
        }
    }

    fn compile_src(src: &str) -> Vec<EmittedClass> {
        let (tree, st, diags) = typecheck_str(src);
        assert!(
            !has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        emit(&tree, &st, "Test.scala")
    }

    fn run_main(src: &str) -> Option<String> {
        if !java_available() {
            return None;
        }
        let classes = compile_src(src);
        assert!(!classes.is_empty(), "no classes emitted");
        let tmp = fresh_dir();
        write_classes(&tmp.0, &classes);
        let output = Command::new("java")
            .arg("-cp")
            .arg(&tmp.0)
            .arg("Main")
            .output()
            .expect("java");
        if !output.status.success() {
            let _ = Command::new("javap")
                .args(["-c", "-p", "-classpath"])
                .arg(&tmp.0)
                .arg("Main")
                .arg("Main$")
                .status();
            panic!(
                "java failed: status={:?}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    #[test]
    fn emit_hello_class_names_and_magic() {
        let classes = compile_src(
            r#"
object Main {
  def main(args: Array[String]): Unit = println(1 + 2)
}
"#,
        );
        let names: Vec<&str> = classes.iter().map(|c| c.internal_name.as_str()).collect();
        assert!(names.contains(&"Main$"), "missing Main$ in {names:?}");
        assert!(names.contains(&"Main"), "missing Main in {names:?}");
        for c in &classes {
            assert!(
                c.bytes.len() >= 4 && c.bytes[0..4] == [0xCA, 0xFE, 0xBA, 0xBE],
                "{} is not a classfile",
                c.internal_name
            );
        }
    }

    #[test]
    fn hello_world_prints_3() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = println(1 + 2)
}
"#,
        ) else {
            return;
        };
        assert!(
            out.contains('3') || out.to_lowercase().contains("hello"),
            "stdout: {out:?}"
        );
    }

    #[test]
    fn factorial_5_prints_120() {
        let Some(out) = run_main(
            r#"
object Main {
  def fact(n: Int): Int =
    if (n <= 1) 1 else n * fact(n - 1)
  def main(args: Array[String]): Unit = println(fact(5))
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("120"), "stdout: {out:?}");
    }

    #[test]
    fn hello_fixture_string() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    println("hello, scala-rs")
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("hello, scala-rs"), "stdout: {out:?}");
    }

    #[test]
    fn arithmetic_fixture() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    println(1 + 2 * 3)
    println(10 - 4)
    println(20 / 4)
    println(7 % 3)
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains('7'), "stdout: {out:?}");
        assert!(out.contains('6'), "stdout: {out:?}");
        assert!(out.contains('5'), "stdout: {out:?}");
        assert!(out.contains('1'), "stdout: {out:?}");
    }

    #[test]
    fn counter_class() {
        let Some(out) = run_main(
            r#"
class Counter(start: Int) {
  var n: Int = start
  def inc(): Unit = { n = n + 1 }
  def get(): Int = n
}
object Main {
  def main(args: Array[String]): Unit = {
    val c = new Counter(10)
    c.inc()
    c.inc()
    println(c.get())
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("12"), "stdout: {out:?}");
    }

    #[test]
    fn case_class_match() {
        let Some(out) = run_main(
            r#"
case class Point(x: Int, y: Int)
object Main {
  def main(args: Array[String]): Unit = {
    val p = Point(3, 4)
    println(p match {
      case Point(a, b) => a + b
    })
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains('7'), "stdout: {out:?}");
    }

    #[test]
    fn trait_impl() {
        let Some(out) = run_main(
            r#"
trait Greeter {
  def greet(name: String): String
}
class HelloGreeter extends Greeter {
  def greet(name: String): String = "Hello, " + name
}
object Main {
  def main(args: Array[String]): Unit = {
    val g: Greeter = new HelloGreeter()
    println(g.greet("Scala"))
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("Hello, Scala"), "stdout: {out:?}");
    }

    #[test]
    fn while_loop() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    var i: Int = 0
    while (i < 3) {
      i = i + 1
    }
    println(i)
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains('3'), "stdout: {out:?}");
    }

    #[test]
    fn string_interpolation() {
        let Some(out) = run_main(
            r#"
object Main {
  def main(args: Array[String]): Unit = {
    val name = "world"
    println(s"hello $name")
  }
}
"#,
        ) else {
            return;
        };
        assert!(out.contains("hello world"), "stdout: {out:?}");
    }
}
