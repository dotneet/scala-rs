#![allow(dead_code)]
//! Namer + typer. Trees are mutated in place (`ty`, `sym`).

use crate::implicits::ImplicitSearch;
use crate::prelude::install_prelude;
use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::ast::*;
use scala_rs_span::{Diagnostic, Span};

pub struct TypecheckOptions {
    pub fatal_warnings: bool,
    /// Type Option/List `withFilter` as the scala-library 2.13 shape, StringOps
    /// via `augmentString`, and Iterator. The backend still needs `library_abi`.
    pub library_abi: bool,
}

impl Default for TypecheckOptions {
    fn default() -> Self {
        TypecheckOptions {
            fatal_warnings: false,
            library_abi: false,
        }
    }
}

pub struct Typer {
    pub st: SymbolTable,
    pub diags: Vec<Diagnostic>,
    file_index: usize,
    /// Counter for synthetic names.
    gensym: u32,
    fatal_warnings: bool,
    library_abi: bool,
}

pub fn typecheck(tree: &mut Tree, file_index: usize) -> (SymbolTable, Vec<Diagnostic>) {
    typecheck_opts(tree, file_index, &TypecheckOptions::default())
}

pub fn typecheck_opts(
    tree: &mut Tree,
    file_index: usize,
    opts: &TypecheckOptions,
) -> (SymbolTable, Vec<Diagnostic>) {
    let mut t = Typer::new(file_index, opts);
    t.fatal_warnings = opts.fatal_warnings;
    t.namer(tree);
    t.register_sealed_from_namer(tree);
    t.typer(tree);
    (t.st, t.diags)
}

impl Typer {
    pub fn new(file_index: usize, opts: &TypecheckOptions) -> Self {
        let mut st = SymbolTable::new();
        install_prelude(&mut st, opts.library_abi);
        Typer {
            st,
            diags: Vec::new(),
            file_index,
            gensym: 0,
            fatal_warnings: opts.fatal_warnings,
            library_abi: opts.library_abi,
        }
    }

    fn error(&mut self, span: Span, msg: impl Into<String>) {
        self.diags
            .push(Diagnostic::error(self.file_index, span, msg));
    }

    fn warning(&mut self, span: Span, msg: impl Into<String>) {
        if self.fatal_warnings {
            self.error(span, msg);
        } else {
            self.diags
                .push(Diagnostic::warning(self.file_index, span, msg));
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.gensym += 1;
        format!("{prefix}${}$", self.gensym)
    }

    // ------------------------------------------------------------------ namer
    fn namer(&mut self, tree: &mut Tree) {
        match &mut tree.kind {
            TreeKind::PackageDef { pid, stats } => {
                let pkg = self.enter_package_path(pid);
                let saved = self.st.owner;
                self.st.owner = pkg;
                self.st.push_scope();
                // First pass: enter classes/modules so they can forward-ref.
                for stt in stats.iter_mut() {
                    self.namer_enter_tmpl(stt);
                }
                for stt in stats.iter_mut() {
                    self.namer(stt);
                }
                self.st.pop_scope();
                self.st.owner = saved;
                tree.sym = pkg;
            }
            TreeKind::ClassDef { .. } => self.namer_class(tree),
            TreeKind::ModuleDef { .. } => self.namer_module(tree),
            TreeKind::Import { expr, .. } => self.namer_import(expr),
            TreeKind::ValDef { .. } | TreeKind::DefDef { .. } | TreeKind::TypeDef { .. } => {
                self.namer_member(tree);
            }
            _ => {}
        }
    }

    fn enter_package_path(&mut self, pid: &Tree) -> SymbolId {
        fn parts(t: &Tree, acc: &mut Vec<String>) {
            match &t.kind {
                TreeKind::Ident { name } if name != "<empty>" && name != "_root_" => {
                    acc.push(name.clone())
                }
                TreeKind::Select { qual, name } => {
                    parts(qual, acc);
                    acc.push(name.clone());
                }
                _ => {}
            }
        }
        let mut ps = Vec::new();
        parts(pid, &mut ps);
        let mut cur = self.st.root;
        let mut jvm = String::new();
        for p in ps {
            if !jvm.is_empty() {
                jvm.push('/');
            }
            jvm.push_str(&p);
            let existing = self
                .st
                .get(cur)
                .members
                .iter()
                .copied()
                .find(|&m| self.st.get(m).kind == SymKind::Package && self.st.get(m).name == p);
            cur = if let Some(e) = existing {
                e
            } else {
                self.st
                    .alloc(&p, cur, SymKind::Package, Flags::PACKAGE, &jvm)
            };
        }
        cur
    }

    fn namer_enter_tmpl(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ClassDef { name, mods, .. } => {
                let is_trait = mods.flags.contains(Flags::TRAIT);
                let flags = mods.flags.with(if is_trait {
                    Flags::ABSTRACT
                } else {
                    Flags::EMPTY
                });
                let jvm = self.jvm_for_current(name);
                let id = self
                    .st
                    .alloc(name, self.st.owner, SymKind::Class, flags, &jvm);
                self.st.enter_in_current(name, id);
                tree.sym = id;
                if mods.flags.contains(Flags::CASE) {
                    self.ensure_companion(name, id);
                }
            }
            TreeKind::ModuleDef { name, mods, .. } => {
                let jvm = format!("{}$", self.jvm_for_current(name));
                let cls = self.st.alloc(
                    &format!("{name}$"),
                    self.st.owner,
                    SymKind::ModuleClass,
                    mods.flags.with(Flags::MODULE).with(Flags::FINAL),
                    &jvm,
                );
                let m = self.st.alloc(
                    name,
                    self.st.owner,
                    SymKind::Module,
                    mods.flags.with(Flags::MODULE),
                    &jvm,
                );
                self.st.get_mut(m).ty = Type::ModuleRef(cls);
                self.st.get_mut(cls).ty = Type::ModuleRef(cls);
                self.st.enter_in_current(name, m);
                tree.sym = m;
            }
            TreeKind::PackageDef { .. } => {}
            _ => {}
        }
    }

    fn jvm_for_current(&self, name: &str) -> String {
        let ow = self.st.get(self.st.owner);
        if ow.kind == SymKind::Package
            && ow.name != "<_root_>"
            && !ow.jvm_name.is_empty()
            && ow.jvm_name != "scala/runtime"
        {
            format!("{}/{}", ow.jvm_name, name)
        } else if ow.kind == SymKind::Package {
            name.to_string()
        } else {
            let base = ow.jvm_name.trim_end_matches('$');
            if base.is_empty() {
                name.to_string()
            } else {
                format!("{}${}", base, name)
            }
        }
    }

    fn ensure_companion(&mut self, name: &str, class_id: SymbolId) -> SymbolId {
        let existing = self
            .st
            .lookup(name)
            .into_iter()
            .find(|&s| self.st.get(s).kind == SymKind::Module);
        if let Some(e) = existing {
            return e;
        }
        let jvm = format!("{}$", self.jvm_for_current(name));
        let cls = self.st.alloc(
            &format!("{name}$"),
            self.st.owner,
            SymKind::ModuleClass,
            Flags::MODULE
                .with(Flags::FINAL)
                .with(Flags::SYNTHETIC)
                .with(Flags::CASE),
            &jvm,
        );
        let m = self.st.alloc(
            name,
            self.st.owner,
            SymKind::Module,
            Flags::MODULE.with(Flags::SYNTHETIC).with(Flags::CASE),
            &jvm,
        );
        self.st.get_mut(m).ty = Type::ModuleRef(cls);
        self.st.get_mut(cls).ty = Type::ModuleRef(cls);
        self.st.enter_in_current(name, m);
        // apply / unapply filled after ctor params are known
        let _ = class_id;
        m
    }

    fn namer_class(&mut self, tree: &mut Tree) {
        let id = if tree.sym.is_none() {
            self.namer_enter_tmpl(tree);
            tree.sym
        } else {
            tree.sym
        };
        let (vparamss, body, parents, is_trait, is_case, name, tparams) = match &mut tree.kind {
            TreeKind::ClassDef {
                vparamss,
                impl_,
                mods,
                name,
                tparams,
                ..
            } => (
                vparamss,
                &mut impl_.body,
                impl_.parents.clone(),
                mods.flags.contains(Flags::TRAIT),
                mods.flags.contains(Flags::CASE),
                name.clone(),
                tparams,
            ),
            _ => return,
        };
        self.st.get_mut(id).parents = self.rough_parents(&parents, is_trait);
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        self.st.owner = id;
        self.st.this_class = id;
        self.st.push_scope();
        let tp_ids = self.enter_tparams(tparams, id);
        self.st.get_mut(id).tparams = tp_ids;
        // constructor params as fields
        let mut fields = Vec::new();
        for clause in vparamss.iter_mut() {
            for p in clause.iter_mut() {
                if let TreeKind::ValDef { name, mods, .. } = &p.kind {
                    let mut flags = mods.flags.with(Flags::PARAM);
                    if is_case {
                        flags.set(Flags::PRIVATE, false);
                    } else if !mods.flags.contains(Flags::MUTABLE) {
                        flags = flags.with(Flags::PRIVATE);
                    }
                    let fid = self.st.alloc(name, id, SymKind::Term, flags, "");
                    self.st.enter_in_current(name, fid);
                    p.sym = fid;
                    fields.push(fid);
                }
            }
        }
        self.st.get_mut(id).ctor_fields = fields.clone();
        // ctor method
        let ctor = self
            .st
            .alloc("<init>", id, SymKind::Method, Flags::CONSTRUCTOR, "");
        self.st.get_mut(ctor).params = fields.clone();
        self.st.enter_in_current("<init>", ctor);

        for stt in body.iter_mut() {
            self.namer_member(stt);
            if matches!(
                &stt.kind,
                TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
            ) {
                self.namer(stt);
            }
        }
        if is_case {
            self.synthesize_case_members(id, &name);
        }
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        tree.sym = id;
    }

    fn enter_tparams(&mut self, tparams: &mut [Tree], owner: SymbolId) -> Vec<SymbolId> {
        let mut ids = Vec::new();
        for tp in tparams {
            let name = tp.name().unwrap_or("_").to_string();
            let id = if tp.sym.is_none() {
                let id = self
                    .st
                    .alloc(&name, owner, SymKind::TypeParam, Flags::EMPTY, "");
                tp.sym = id;
                id
            } else {
                tp.sym
            };
            self.st.get_mut(id).ty = Type::TypeParam(id);
            self.st.enter_in_current(&name, id);
            ids.push(id);
        }
        ids
    }

    fn rough_parents(&self, parents: &[Tree], is_trait: bool) -> Vec<Type> {
        if parents.is_empty() {
            return vec![if is_trait { Type::AnyRef } else { Type::AnyRef }];
        }
        parents
            .iter()
            .map(|p| Type::Named {
                name: p.name().unwrap_or("AnyRef").to_string(),
                args: vec![],
            })
            .collect()
    }

    fn synthesize_case_members(&mut self, class_id: SymbolId, name: &str) {
        let fields = self.st.get(class_id).ctor_fields.clone();
        let class_ty = Type::Class {
            sym: class_id,
            args: vec![],
        };
        // copy, productArity, toString, equals, hashCode as methods (backend will emit)
        let copy = self
            .st
            .alloc("copy", class_id, SymKind::Method, Flags::SYNTHETIC, "");
        let ptys: Vec<Type> = fields.iter().map(|_| Type::NoType).collect();
        self.st.get_mut(copy).ty = Type::Method {
            paramss: vec![ptys],
            ret: Box::new(class_ty.clone()),
        };
        let _ = self.st.alloc(
            "productArity",
            class_id,
            SymKind::Method,
            Flags::SYNTHETIC,
            "",
        );
        // companion apply
        let companion = self
            .st
            .lookup(name)
            .into_iter()
            .find(|&s| self.st.get(s).kind == SymKind::Module);
        if let Some(m) = companion {
            let cls = match self.st.get(m).ty {
                Type::ModuleRef(c) => c,
                _ => m,
            };
            let apply = self.st.alloc(
                "apply",
                cls,
                SymKind::Method,
                Flags::SYNTHETIC.with(Flags::CASE),
                "",
            );
            self.st.get_mut(apply).params = fields.clone();
            self.st.get_mut(apply).ty = Type::Method {
                paramss: vec![fields.iter().map(|_| Type::NoType).collect()],
                ret: Box::new(class_ty),
            };
            self.st.enter_in_current("apply", apply);
            // also put apply on the module value for Point(1,2)
            self.st.get_mut(m).members.push(apply);
            let unapply = self.st.alloc(
                "unapply",
                cls,
                SymKind::Method,
                Flags::SYNTHETIC.with(Flags::CASE),
                "",
            );
            self.st.get_mut(m).members.push(unapply);
        }
    }

    fn namer_module(&mut self, tree: &mut Tree) {
        if tree.sym.is_none() {
            self.namer_enter_tmpl(tree);
        }
        let m = tree.sym;
        let cls = match self.st.get(m).ty {
            Type::ModuleRef(c) => c,
            _ => m,
        };
        if let TreeKind::ModuleDef { impl_, .. } = &tree.kind {
            self.st.get_mut(cls).parents = self.rough_parents(&impl_.parents, false);
        }
        let body = match &mut tree.kind {
            TreeKind::ModuleDef { impl_, .. } => &mut impl_.body,
            _ => return,
        };
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        self.st.owner = cls;
        self.st.this_class = cls;
        self.st.push_scope();
        for stt in body.iter_mut() {
            self.namer_member(stt);
            if matches!(
                &stt.kind,
                TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
            ) {
                self.namer(stt);
            }
        }
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        if let TreeKind::ModuleDef { name, mods, .. } = &tree.kind {
            if name == "package" || mods.flags.contains(Flags::PACKAGE) {
                let pkg = saved_owner;
                let mems = self.st.get(cls).members.clone();
                for mem in mems {
                    if !self.st.get(pkg).members.contains(&mem) {
                        self.st.get_mut(pkg).members.push(mem);
                    }
                }
            }
        }
    }

    fn namer_member(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ValDef { name, mods, .. } => {
                let id = self
                    .st
                    .alloc(name, self.st.owner, SymKind::Term, mods.flags, "");
                self.st.enter_in_current(name, id);
                tree.sym = id;
            }
            TreeKind::DefDef { name, mods, .. } => {
                let id = self
                    .st
                    .alloc(name, self.st.owner, SymKind::Method, mods.flags, "");
                self.st.enter_in_current(name, id);
                tree.sym = id;
            }
            TreeKind::TypeDef { name, mods, .. } => {
                let id = self
                    .st
                    .alloc(name, self.st.owner, SymKind::Class, mods.flags, "");
                self.st.enter_in_current(name, id);
                tree.sym = id;
            }
            TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => {
                self.namer_enter_tmpl(tree);
            }
            _ => {}
        }
    }

    fn namer_import(&mut self, expr: &Tree) {
        // import a.b._  or import a.b.C  or import a.b.{C, D => E}
        match &expr.kind {
            TreeKind::Select { qual: _, name } if name == "_" => {
                // wildcard: we don't have a resolved prefix yet; typer will expand.
                // For the first cut, wildcard imports of known packages are handled
                // by looking up the prefix in typer. Record a marker.
            }
            TreeKind::Select { name, .. } if name.starts_with('{') => {
                // selectors encoded as `{A,B=>C}`
                let inner = name.trim_matches(|c| c == '{' || c == '}');
                for sel in inner.split(',') {
                    if sel == "_" {
                        continue;
                    }
                    let mut it = sel.split("=>");
                    let from = it.next().unwrap_or(sel).trim();
                    let to = it.next().unwrap_or(from).trim();
                    if to == "_" {
                        continue;
                    }
                    let found = self.st.lookup(from);
                    for f in found {
                        self.st.enter_in_current(to, f);
                    }
                }
            }
            TreeKind::Select { name, .. } | TreeKind::Ident { name } => {
                let found = self.st.lookup(name);
                for f in found {
                    self.st.enter_in_current(name, f);
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ typer
    fn typer(&mut self, tree: &mut Tree) {
        match &mut tree.kind {
            TreeKind::PackageDef { stats, .. } => {
                self.st.push_scope();
                if !tree.sym.is_none() {
                    for m in self.st.get(tree.sym).members.clone() {
                        let n = self.st.get(m).name.clone();
                        if n.ends_with('$') {
                            continue;
                        }
                        self.st.enter_in_current(&n, m);
                    }
                }
                for s in stats.iter_mut() {
                    self.typer(s);
                }
                self.st.pop_scope();
            }
            TreeKind::ClassDef { .. } => self.type_class(tree),
            TreeKind::ModuleDef { .. } => self.type_module(tree),
            TreeKind::Import { .. } => self.type_import(tree),
            _ => {
                self.type_stat(tree);
            }
        }
    }

    fn type_class(&mut self, tree: &mut Tree) {
        let id = tree.sym;
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        self.st.owner = id;
        self.st.this_class = id;
        self.st.push_scope();
        // re-enter members into local scope
        for m in self.st.get(id).members.clone() {
            let n = self.st.get(m).name.clone();
            self.st.enter_in_current(&n, m);
        }
        let (vparamss, body, parents) = match &mut tree.kind {
            TreeKind::ClassDef {
                vparamss, impl_, ..
            } => (vparamss, &mut impl_.body, &mut impl_.parents),
            _ => return,
        };
        let mut pts = Vec::new();
        for p in parents.iter_mut() {
            self.type_expr(p, &Type::NoType);
            pts.push(p.ty.clone());
        }
        if !pts.is_empty() {
            self.st.get_mut(id).parents = pts;
        }
        self.register_sealed_child(id);
        let mut ctor_param_tys = Vec::new();
        for clause in vparamss.iter_mut() {
            for p in clause.iter_mut() {
                self.type_val_sig(p);
                ctor_param_tys.push(p.ty.clone());
                if !p.sym.is_none() {
                    self.st.get_mut(p.sym).ty = p.ty.clone();
                }
            }
        }
        self.st.get_mut(id).ty = Type::Class {
            sym: id,
            args: vec![],
        };
        // type member signatures then bodies
        for stt in body.iter_mut() {
            self.type_member_sig(stt);
        }
        for stt in body.iter_mut() {
            self.type_member_body(stt);
        }
        // finish case apply types
        self.finish_case_apply(id, &ctor_param_tys);
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        tree.ty = Type::Class {
            sym: id,
            args: vec![],
        };
    }

    fn finish_case_apply(&mut self, class_id: SymbolId, ctor_param_tys: &[Type]) {
        let name = self.st.get(class_id).name.clone();
        let companion = self
            .st
            .lookup(&name)
            .into_iter()
            .find(|&s| self.st.get(s).kind == SymKind::Module);
        if let Some(m) = companion {
            let cls = match self.st.get(m).ty {
                Type::ModuleRef(c) => c,
                _ => m,
            };
            for mem in self.st.get(cls).members.clone() {
                if self.st.get(mem).name == "apply" {
                    self.st.get_mut(mem).ty = Type::Method {
                        paramss: vec![ctor_param_tys.to_vec()],
                        ret: Box::new(Type::Class {
                            sym: class_id,
                            args: vec![],
                        }),
                    };
                }
            }
            for f in self.st.get(class_id).ctor_fields.clone() {
                if self.st.get(f).ty.is_no_type() {
                    // already set
                }
                let _ = f;
            }
        }
        // set ctor type
        for mem in self.st.get(class_id).members.clone() {
            if self.st.get(mem).name == "<init>" {
                self.st.get_mut(mem).ty = Type::Method {
                    paramss: vec![ctor_param_tys.to_vec()],
                    ret: Box::new(Type::Unit),
                };
            }
        }
    }

    fn type_module(&mut self, tree: &mut Tree) {
        let m = tree.sym;
        let cls = match self.st.get(m).ty {
            Type::ModuleRef(c) => c,
            _ => m,
        };
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        self.st.owner = cls;
        self.st.this_class = cls;
        self.st.push_scope();
        for mem in self.st.get(cls).members.clone() {
            let n = self.st.get(mem).name.clone();
            self.st.enter_in_current(&n, mem);
        }
        let (body, parents) = match &mut tree.kind {
            TreeKind::ModuleDef { impl_, .. } => (&mut impl_.body, &mut impl_.parents),
            _ => return,
        };
        let mut pts = Vec::new();
        for p in parents.iter_mut() {
            self.type_expr(p, &Type::NoType);
            pts.push(p.ty.clone());
        }
        if !pts.is_empty() {
            self.st.get_mut(cls).parents = pts;
        }
        self.register_sealed_child(cls);
        for stt in body.iter_mut() {
            self.type_member_sig(stt);
        }
        for stt in body.iter_mut() {
            self.type_member_body(stt);
        }
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        tree.ty = Type::ModuleRef(cls);
    }

    fn type_member_sig(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ValDef { .. } => self.type_val_sig(tree),
            TreeKind::DefDef { .. } => self.type_def_sig(tree),
            TreeKind::ClassDef { .. } => self.type_class(tree),
            TreeKind::ModuleDef { .. } => self.type_module(tree),
            TreeKind::TypeDef { .. } => {
                tree.ty = Type::Any;
            }
            _ => {}
        }
    }

    fn type_member_body(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ValDef { .. } => self.type_val_body(tree),
            TreeKind::DefDef { .. } => self.type_def_body(tree),
            TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => {}
            TreeKind::Import { .. } => self.type_import(tree),
            _ => {
                self.type_stat(tree);
            }
        }
    }

    fn type_val_sig(&mut self, tree: &mut Tree) {
        let (tpt, name, flags) = match &tree.kind {
            TreeKind::ValDef {
                tpt, name, mods, ..
            } => (tpt.clone(), name.clone(), mods.flags),
            _ => return,
        };
        let ty = if tpt.is_empty() {
            Type::NoType
        } else {
            self.tree_to_type(&tpt)
        };
        let ty = if flags.contains(Flags::BYNAME) && !matches!(ty, Type::ByName(_) | Type::NoType) {
            Type::ByName(Box::new(ty))
        } else {
            ty
        };
        tree.ty = ty.clone();
        if tree.sym.is_none() {
            let id = self
                .st
                .alloc(name.clone(), self.st.owner, SymKind::Term, flags, "");
            tree.sym = id;
            self.st.enter_in_current(&name, id);
        }
        if !tree.sym.is_none() {
            self.st.get_mut(tree.sym).ty = ty.clone();
            if flags.contains(Flags::DEFAULTPARAM) {
                let f = self.st.get(tree.sym).flags.with(Flags::DEFAULTPARAM);
                self.st.get_mut(tree.sym).flags = f;
            }
            if flags.contains(Flags::IMPLICIT) {
                let f = self.st.get(tree.sym).flags.with(Flags::IMPLICIT);
                self.st.get_mut(tree.sym).flags = f;
            }
            if flags.contains(Flags::BYNAME) {
                let f = self.st.get(tree.sym).flags.with(Flags::BYNAME);
                self.st.get_mut(tree.sym).flags = f;
            }
        }
        let _ = name;
    }

    fn type_val_body(&mut self, tree: &mut Tree) {
        let (rhs, declared) = match &mut tree.kind {
            TreeKind::ValDef { rhs, .. } => (rhs, tree.ty.clone()),
            _ => return,
        };
        if rhs.is_empty() {
            if declared.is_no_type() {
                self.error(tree.span, "abstract value needs a type");
                tree.ty = Type::Error;
            }
            return;
        }
        let pt = if declared.is_no_type() {
            Type::NoType
        } else {
            declared.clone()
        };
        self.type_expr(rhs, &pt);
        if declared.is_no_type() {
            tree.ty = rhs.ty.clone();
            if !tree.sym.is_none() {
                self.st.get_mut(tree.sym).ty = tree.ty.clone();
            }
        } else {
            self.adapt(rhs, &declared);
            tree.ty = declared;
        }
    }

    fn type_def_sig(&mut self, tree: &mut Tree) {
        let (tparams, vparamss, tpt, name) = match &mut tree.kind {
            TreeKind::DefDef {
                tparams,
                vparamss,
                tpt,
                name,
                ..
            } => (tparams, vparamss, tpt.clone(), name.clone()),
            _ => return,
        };
        if tree.sym.is_none() {
            let id = self.st.alloc(
                name.clone(),
                self.st.owner,
                SymKind::Method,
                Flags::EMPTY,
                "",
            );
            tree.sym = id;
            self.st.enter_in_current(&name, id);
        }
        self.st.push_scope();
        let tp_ids = self.enter_tparams(tparams, tree.sym);
        self.st.get_mut(tree.sym).tparams = tp_ids;
        let saved_owner = self.st.owner;
        self.st.owner = tree.sym;
        let mut paramss_ty = Vec::new();
        let mut all_params = Vec::new();
        let mut paramss_ids: Vec<Vec<SymbolId>> = Vec::new();
        for clause in vparamss.iter_mut() {
            let mut ct = Vec::new();
            let mut ids = Vec::new();
            for p in clause.iter_mut() {
                self.type_val_sig(p);
                if p.ty.is_no_type() {
                    self.error(
                        p.span,
                        format!("missing parameter type for `{}`", p.name().unwrap_or("?")),
                    );
                    p.ty = Type::Error;
                }
                if p.sym.is_none() {
                    if let TreeKind::ValDef { name, mods, .. } = &p.kind {
                        let n = name.clone();
                        let flags = mods.flags.with(Flags::PARAM);
                        let id = self.st.alloc(n, tree.sym, SymKind::Term, flags, "");
                        p.sym = id;
                    }
                }
                if !p.sym.is_none() {
                    self.st.get_mut(p.sym).ty = p.ty.clone();
                    if let TreeKind::ValDef { mods, rhs, .. } = &p.kind {
                        if mods.flags.contains(Flags::DEFAULTPARAM) && !rhs.is_empty() {
                            self.st.get_mut(p.sym).default_rhs = Some((**rhs).clone());
                        }
                    }
                    all_params.push(p.sym);
                    ids.push(p.sym);
                }
                ct.push(p.ty.clone());
            }
            paramss_ty.push(ct);
            paramss_ids.push(ids);
        }
        self.st.owner = saved_owner;
        let ret = if tpt.is_empty() {
            Type::NoType
        } else {
            self.tree_to_type(&tpt)
        };
        self.st.pop_scope();
        let mty = Type::Method {
            paramss: paramss_ty,
            ret: Box::new(ret.clone()),
        };
        tree.ty = mty.clone();
        if !tree.sym.is_none() {
            self.st.get_mut(tree.sym).ty = mty;
            self.st.get_mut(tree.sym).params = all_params;
            self.st.get_mut(tree.sym).paramss = paramss_ids;
        }
        let _ = name;
    }

    fn type_def_body(&mut self, tree: &mut Tree) {
        let (vparamss, rhs, ret_pt) = match &mut tree.kind {
            TreeKind::DefDef { vparamss, rhs, .. } => {
                let ret = match &tree.ty {
                    Type::Method { ret, .. } => (**ret).clone(),
                    _ => Type::NoType,
                };
                (vparamss, rhs, ret)
            }
            _ => return,
        };
        if rhs.is_empty() {
            return;
        }
        self.st.push_scope();
        if !tree.sym.is_none() {
            for tp in self.st.get(tree.sym).tparams.clone() {
                let n = self.st.get(tp).name.clone();
                self.st.enter_in_current(&n, tp);
            }
        }
        for clause in vparamss.iter() {
            for p in clause {
                if !p.sym.is_none() {
                    self.st.enter_in_current(p.name().unwrap_or("?"), p.sym);
                }
            }
        }
        self.type_expr(rhs, &ret_pt);
        if !ret_pt.is_no_type() {
            self.adapt(rhs, &ret_pt);
        } else {
            // infer result type
            if let Type::Method { ret, .. } = &mut tree.ty {
                *ret = Box::new(rhs.ty.clone());
            }
            if !tree.sym.is_none() {
                self.st.get_mut(tree.sym).ty = tree.ty.clone();
            }
        }
        self.st.pop_scope();
    }

    fn type_stat(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ValDef { .. } => {
                self.type_val_sig(tree);
                self.type_val_body(tree);
            }
            TreeKind::DefDef { .. } => {
                self.type_def_sig(tree);
                self.type_def_body(tree);
            }
            TreeKind::Import { .. } => self.type_import(tree),
            _ => {
                self.type_expr(tree, &Type::NoType);
            }
        }
    }

    fn type_import(&mut self, tree: &mut Tree) {
        let expr = match &mut tree.kind {
            TreeKind::Import { expr, .. } => expr,
            _ => return,
        };
        match &mut expr.kind {
            TreeKind::Select { qual, name } if name == "_" => {
                self.type_expr(qual, &Type::NoType);
                let owner = if !qual.sym.is_none() {
                    let id = qual.sym;
                    match self.st.get(id).kind {
                        SymKind::Module | SymKind::ModuleClass => self.st.module_class_of(id),
                        _ => id,
                    }
                } else {
                    self.st
                        .class_sym_of(&qual.ty)
                        .map(|c| self.st.module_class_of(c))
                        .unwrap_or(SymbolId::NONE)
                };
                if owner.is_none() {
                    return;
                }
                let members = self.st.get(owner).members.clone();
                for m in members {
                    let n = self.st.get(m).name.clone();
                    if n.ends_with('$') || n == "<init>" {
                        continue;
                    }
                    self.st.enter_in_current(&n, m);
                }
            }
            TreeKind::Select { qual, name } => {
                let n = name.clone();
                self.type_expr(qual, &Type::NoType);
                let owner = if !qual.sym.is_none() {
                    let id = qual.sym;
                    match self.st.get(id).kind {
                        SymKind::Module | SymKind::ModuleClass => self.st.module_class_of(id),
                        _ => id,
                    }
                } else {
                    self.st.class_sym_of(&qual.ty).unwrap_or(SymbolId::NONE)
                };
                if owner.is_none() {
                    return;
                }
                for m in self.st.lookup_member(owner, &n) {
                    self.st.enter_in_current(&n, m);
                }
            }
            TreeKind::Ident { name } => {
                let n = name.clone();
                for f in self.st.lookup(&n) {
                    self.st.enter_in_current(&n, f);
                }
            }
            _ => {}
        }
        tree.ty = Type::NoType;
    }

    fn type_expr(&mut self, tree: &mut Tree, pt: &Type) {
        if matches!(&tree.kind, TreeKind::Ident { .. }) {
            let name = match &tree.kind {
                TreeKind::Ident { name } => name.clone(),
                _ => unreachable!(),
            };
            self.type_ident(tree, name, pt);
        } else if matches!(&tree.kind, TreeKind::Function { .. }) {
            let ty = {
                let (vparams, body) = match &mut tree.kind {
                    TreeKind::Function { vparams, body } => (vparams, body),
                    _ => unreachable!(),
                };
                self.type_function(vparams, body, pt)
            };
            tree.ty = ty;
        } else {
            self.type_expr_inner(tree, pt);
        }
        if !pt.is_no_type() && !tree.ty.is_no_type() && !tree.ty.is_error() {
            self.adapt(tree, pt);
        }
    }

    fn type_expr_inner(&mut self, tree: &mut Tree, pt: &Type) {
        match &mut tree.kind {
            TreeKind::Literal { lit } => {
                tree.ty = match lit {
                    Lit::Unit => Type::Unit,
                    Lit::Boolean(_) => Type::Boolean,
                    Lit::Int(_) => Type::Int,
                    Lit::Long(_) => Type::Long,
                    Lit::Float(_) => Type::Float,
                    Lit::Double(_) => Type::Double,
                    Lit::Char(_) => Type::Char,
                    Lit::String(_) => Type::String,
                    Lit::Null => Type::Null,
                    Lit::Symbol(_) => Type::Named {
                        name: "Symbol".into(),
                        args: vec![],
                    },
                };
            }
            TreeKind::This { qual } => {
                let q = qual.clone();
                let id = if let Some(name) = q {
                    self.st
                        .enclosing_class_named(self.st.this_class, &name)
                        .unwrap_or(self.st.this_class)
                } else {
                    self.st.this_class
                };
                if id.is_none() {
                    self.error(tree.span, "`this` is not allowed here");
                    tree.ty = Type::Error;
                } else {
                    tree.sym = id;
                    tree.ty = self.st.type_of_class(id);
                }
            }
            TreeKind::Select { .. } => self.type_select(tree, pt),
            TreeKind::Apply { .. } => self.type_apply(tree, pt),
            TreeKind::TypeApply { fun, args } => {
                self.type_expr(fun, &Type::NoType);
                let targs: Vec<Type> = args.iter().map(|a| self.tree_to_type(a)).collect();
                if !fun.sym.is_none() {
                    tree.sym = fun.sym;
                    tree.ty = self.st.subst_tparams(fun.sym, &targs, &fun.ty);
                } else {
                    tree.ty = fun.ty.clone();
                }
                self.adapt_implicit_apply(tree, pt);
            }
            TreeKind::Block { stats, expr } => {
                self.st.push_scope();
                for s in stats.iter_mut() {
                    self.type_stat(s);
                }
                self.type_expr(expr, pt);
                tree.ty = expr.ty.clone();
                self.st.pop_scope();
            }
            TreeKind::If { cond, thenp, elsep } => {
                self.type_expr(cond, &Type::Boolean);
                self.adapt(cond, &Type::Boolean);
                self.type_expr(thenp, pt);
                self.type_expr(elsep, pt);
                // `adapt` leaves the branch type as-is when it is a subtype of `pt`
                // (`Some` stays `Some`, not `Option`). Structural lub cannot walk
                // parents, so use the expected type when the typer has one.
                tree.ty = if !pt.is_no_type() && !matches!(pt, Type::Nothing) {
                    pt.clone()
                } else {
                    lub(&thenp.ty, &elsep.ty)
                };
            }
            TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
                self.type_expr(cond, &Type::Boolean);
                self.type_expr(body, &Type::Unit);
                tree.ty = Type::Unit;
            }
            TreeKind::Assign { lhs, rhs } => {
                self.type_expr(lhs, &Type::NoType);
                self.type_expr(rhs, &lhs.ty);
                self.adapt(rhs, &lhs.ty);
                tree.ty = Type::Unit;
            }
            TreeKind::Match { .. } => self.type_match(tree, pt),
            TreeKind::New { tpt } => {
                if let TreeKind::ClassDef { name, .. } = &tpt.kind {
                    if name == "$anon" {
                        self.error(
                            tree.span,
                            "unimplemented syntax: anonymous classes (`new T { ... }`)",
                        );
                        tree.ty = Type::Error;
                        return;
                    }
                }
                if let TreeKind::Ident { name } = &tpt.kind {
                    let n = name.clone();
                    let found = self.st.lookup(&n);
                    if let Some(id) = found
                        .into_iter()
                        .find(|s| self.st.get(*s).kind == SymKind::Class)
                    {
                        tpt.sym = id;
                        tpt.ty = Type::Class {
                            sym: id,
                            args: vec![],
                        };
                    } else {
                        self.type_expr(tpt, &Type::NoType);
                    }
                } else {
                    self.type_expr(tpt, &Type::NoType);
                }
                tree.ty = tpt.ty.clone();
                tree.sym = tpt.sym;
                if tree.sym.is_none() {
                    if let Some(id) = self.st.class_sym_of(&tpt.ty) {
                        tree.ty = Type::Class {
                            sym: id,
                            args: vec![],
                        };
                        tree.sym = id;
                    }
                }
            }
            TreeKind::Typed { expr, tpt } => {
                let ty = self.tree_to_type(tpt);
                self.type_expr(expr, &ty);
                self.adapt(expr, &ty);
                tree.ty = ty;
            }
            TreeKind::Return { expr } => {
                self.type_expr(expr, &Type::NoType);
                tree.ty = Type::Nothing;
            }
            TreeKind::Throw { expr } => {
                self.type_expr(expr, &Type::Any);
                tree.ty = Type::Nothing;
            }
            TreeKind::Try {
                block,
                catches,
                finalizer,
            } => {
                self.type_expr(block, pt);
                for c in catches.iter_mut() {
                    self.type_case(c, pt);
                }
                if !finalizer.is_empty() {
                    self.type_expr(finalizer, &Type::Unit);
                }
                tree.ty = block.ty.clone();
            }
            TreeKind::InterpolatedString {
                prefix,
                parts,
                args,
            } => {
                if prefix != "s" && prefix != "raw" {
                    self.error(
                        tree.span,
                        format!("unimplemented interpolator `{prefix}` (only s\"...\" / raw\"...\" in this pass)"),
                    );
                }
                for a in args.iter_mut() {
                    self.type_expr(a, &Type::Any);
                }
                let _ = parts;
                tree.ty = Type::String;
            }
            TreeKind::Wildcard => {
                self.error(
                    tree.span,
                    "unimplemented syntax: placeholder `_` in expressions",
                );
                tree.ty = Type::Error;
            }
            TreeKind::Unimplemented { what } => {
                self.error(tree.span, format!("unimplemented syntax: {what}"));
                tree.ty = Type::Error;
            }
            TreeKind::Empty => {
                tree.ty = Type::NoType;
            }
            TreeKind::Super { qual, mix } => {
                let q = qual.clone();
                let mix = mix.clone();
                let this_id = if let Some(name) = q {
                    self.st
                        .enclosing_class_named(self.st.this_class, &name)
                        .unwrap_or(self.st.this_class)
                } else {
                    self.st.this_class
                };
                let parent = self.super_target(this_id, mix.as_deref());
                if parent.is_none() {
                    self.error(tree.span, "`super` has no parent type");
                    tree.ty = Type::AnyRef;
                } else {
                    tree.sym = parent;
                    tree.ty = self.st.type_of_class(parent);
                }
            }
            TreeKind::AppliedTypeTree { .. } => {
                tree.ty = self.tree_to_type(tree);
            }
            TreeKind::DefDef { .. }
            | TreeKind::ValDef { .. }
            | TreeKind::ClassDef { .. }
            | TreeKind::ModuleDef { .. } => {
                // Nested defs typed as statements; `type_stat` needs the whole tree so we
                // set a marker and type after the match.
                tree.ty = Type::NoType;
            }
            _ => {
                tree.ty = Type::Error;
            }
        }
        if matches!(
            &tree.kind,
            TreeKind::DefDef { .. }
                | TreeKind::ValDef { .. }
                | TreeKind::ClassDef { .. }
                | TreeKind::ModuleDef { .. }
        ) {
            self.type_stat(tree);
        }
    }

    fn type_ident(&mut self, tree: &mut Tree, name: String, pt: &Type) {
        if name == "_" {
            tree.kind = TreeKind::Wildcard;
            tree.ty = Type::Error;
            return;
        }
        let found = self.st.lookup(&name);
        if found.is_empty() {
            self.error(tree.span, format!("not found: value {name}"));
            tree.ty = Type::Error;
            return;
        }
        // Term position prefers modules/methods/vals over the class of the same name.
        let terms: Vec<SymbolId> = found
            .iter()
            .copied()
            .filter(|s| {
                matches!(
                    self.st.get(*s).kind,
                    SymKind::Module | SymKind::Method | SymKind::Term
                )
            })
            .collect();
        let found = if terms.is_empty() { found } else { terms };
        self.bind_found(tree, found, pt);
    }

    fn bind_found(&mut self, tree: &mut Tree, mut found: Vec<SymbolId>, pt: &Type) {
        found.sort_by_key(|s| s.0);
        found.dedup();
        if found.len() == 1 {
            let s = found[0];
            tree.sym = s;
            let mut ty = self.st.get(s).ty.clone();
            ty = self.maybe_auto_apply(ty, pt);
            tree.ty = ty;
            return;
        }
        // Keep overloads intact so `println(1)` can still pick a 1-arg alternative.
        tree.ty = Type::Overload(found.iter().map(|s| self.st.get(*s).ty.clone()).collect());
        tree.sym = found[0];
    }

    fn maybe_auto_apply(&self, ty: Type, pt: &Type) -> Type {
        match &ty {
            Type::Method { paramss, ret } if paramss.is_empty() => {
                if matches!(pt, Type::Function { .. } | Type::Method { .. }) {
                    ty
                } else {
                    (**ret).clone()
                }
            }
            Type::ByName(inner) => {
                if matches!(
                    pt,
                    Type::Function { .. } | Type::ByName(_) | Type::Method { .. }
                ) {
                    ty
                } else {
                    (**inner).clone()
                }
            }
            _ => ty,
        }
    }

    /// `implicitly[Int]` is a TypeApply of a method whose remaining clause is
    /// implicit; rewrite to an Apply filled from implicit search.
    fn adapt_implicit_apply(&mut self, tree: &mut Tree, pt: &Type) {
        if matches!(pt, Type::Method { .. } | Type::Function { .. }) {
            return;
        }
        if tree.sym.is_none() {
            return;
        }
        let paramss = self.st.get(tree.sym).paramss.clone();
        let first = paramss.first().cloned().unwrap_or_default();
        if first.is_empty() {
            return;
        }
        if !first
            .iter()
            .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
        {
            return;
        }
        if !matches!(&tree.ty, Type::Method { .. }) {
            return;
        }
        let span = tree.span;
        let ret = match &tree.ty {
            Type::Method { ret, .. } => (**ret).clone(),
            _ => return,
        };
        let tys: Vec<Type> = first
            .iter()
            .map(|id| match &tree.ty {
                Type::Method { paramss, .. } => paramss
                    .first()
                    .and_then(|ps| ps.get(first.iter().position(|x| x == id).unwrap_or(0)))
                    .cloned()
                    .unwrap_or_else(|| self.st.get(*id).ty.clone()),
                _ => self.st.get(*id).ty.clone(),
            })
            .collect();
        let mut args = Vec::new();
        self.fill_implicit_params(span, &mut args, &tys, &first);
        let inner = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
        let id = inner.id;
        let sym = inner.sym;
        *tree = Tree {
            id,
            span,
            kind: TreeKind::Apply {
                fun: Box::new(inner),
                args,
            },
            ty: ret,
            sym,
        };
    }

    fn type_select(&mut self, tree: &mut Tree, pt: &Type) {
        let (qual, name) = match &mut tree.kind {
            TreeKind::Select { qual, name } => (qual, name.clone()),
            _ => return,
        };
        self.type_expr(qual, &Type::NoType);
        if name == "_" {
            self.error(
                tree.span,
                "unimplemented syntax: wildcard import/select in expression",
            );
            tree.ty = Type::Error;
            return;
        }
        // String concatenation via any2stringadd: handled at Apply of +
        let owner = self.st.class_sym_of(&qual.ty);
        let mut found = if let Some(o) = owner {
            self.st.lookup_member(o, &name)
        } else {
            Vec::new()
        };
        // Module: members of module class
        if found.is_empty() {
            if let Type::ModuleRef(id) = &qual.ty {
                found = self.st.lookup_member(*id, &name);
            }
        }
        if found.is_empty() && name == "toString" {
            found = self.st.lookup_member(self.st.any_sym, "toString");
        }
        if found.is_empty() {
            if let Some((conv, member, to)) = self.search_extension(&qual.ty, &name) {
                let span = qual.span;
                let old = std::mem::replace(qual.as_mut(), Tree::dummy(TreeKind::Empty));
                let fun = self.ref_implicit(conv, span);
                **qual = Tree {
                    id: old.id,
                    span,
                    kind: TreeKind::Apply {
                        fun: Box::new(fun),
                        args: vec![old],
                    },
                    ty: to,
                    sym: conv,
                };
                found = vec![member];
            }
        }
        if found.is_empty() {
            self.error(
                tree.span,
                format!(
                    "value {name} is not a member of {}",
                    self.st.display_type(&qual.ty)
                ),
            );
            tree.ty = Type::Error;
            return;
        }
        let subst = |ty: Type| -> Type {
            if let Type::Class { args, .. } = &qual.ty {
                if !args.is_empty() {
                    if let Some(owner) = found.first().map(|s| self.st.get(*s).owner) {
                        return self.st.subst_tparams(owner, args, &ty);
                    }
                }
            }
            ty
        };
        if found.len() == 1 {
            let s = found[0];
            tree.sym = s;
            let ty = subst(self.st.get(s).ty.clone());
            tree.ty = self.maybe_auto_apply(ty, pt);
        } else {
            tree.sym = found[0];
            let owner = self.st.get(found[0]).owner;
            let args = match &qual.ty {
                Type::Class { args, .. } => args.clone(),
                _ => vec![],
            };
            tree.ty = Type::Overload(
                found
                    .iter()
                    .map(|s| {
                        let t = self.st.get(*s).ty.clone();
                        if args.is_empty() {
                            t
                        } else {
                            self.st.subst_tparams(owner, &args, &t)
                        }
                    })
                    .collect(),
            );
        }
    }

    /// When a member exists on the receiver (e.g. `Int.+`) but the argument
    /// types do not match, try an implicit conversion that *does* have the
    /// method (`any2stringadd` for `1 + "x"`).
    fn rewrite_apply_extension(&mut self, fun: &mut Tree) -> bool {
        let TreeKind::Select { qual, name } = &mut fun.kind else {
            return false;
        };
        let Some((conv, member, to)) = self.search_extension(&qual.ty, name) else {
            return false;
        };
        let span = qual.span;
        let old = std::mem::replace(qual.as_mut(), Tree::dummy(TreeKind::Empty));
        let conv_fun = self.ref_implicit(conv, span);
        **qual = Tree {
            id: old.id,
            span,
            kind: TreeKind::Apply {
                fun: Box::new(conv_fun),
                args: vec![old],
            },
            ty: to,
            sym: conv,
        };
        fun.sym = member;
        fun.ty = self.st.get(member).ty.clone();
        true
    }

    fn type_apply(&mut self, tree: &mut Tree, pt: &Type) {
        let (fun, args) = match &mut tree.kind {
            TreeKind::Apply { fun, args } => (fun, args),
            _ => return,
        };
        // new C(args)
        if matches!(&fun.kind, TreeKind::New { .. }) {
            self.type_expr(fun, &Type::NoType);
            let class_id = fun
                .sym
                .is_none()
                .then(|| self.st.class_sym_of(&fun.ty))
                .flatten()
                .or(Some(fun.sym))
                .filter(|s| !s.is_none());
            let class_id = class_id.or_else(|| self.st.class_sym_of(&fun.ty));
            let ctor_params = class_id
                .map(|c| {
                    self.st
                        .get(c)
                        .ctor_fields
                        .iter()
                        .map(|f| self.st.get(*f).ty.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for (i, a) in args.iter_mut().enumerate() {
                let p = ctor_params.get(i).cloned().unwrap_or(Type::NoType);
                self.type_expr(a, &p);
                if !p.is_no_type() {
                    self.adapt(a, &p);
                }
            }
            tree.ty = fun.ty.clone();
            tree.sym = class_id.unwrap_or(SymbolId::NONE);
            return;
        }

        let dummy_method = Type::Method {
            paramss: vec![],
            ret: Box::new(Type::NoType),
        };
        // Expected type Method so nullary methods (`unary_-`, `def f: Int` called as `f()`)
        // are not auto-applied before this Apply is typed.
        self.type_expr(fun, &dummy_method);
        self.reorder_named_args(args, fun);

        let recv_ty = match &fun.kind {
            TreeKind::Select { qual, .. } => Some(qual.ty.clone()),
            _ => None,
        };
        let fun_name = fun.name().unwrap_or("").to_string();

        // Type non-lambda args first so overload resolution has info; lambdas
        // wait for an expected Function type (for-comprehension desugaring).
        let mut arg_tys = Vec::new();
        for a in args.iter_mut() {
            if matches!(a.kind, TreeKind::Function { .. }) {
                arg_tys.push(Type::Function {
                    params: vec![Type::NoType],
                    ret: Box::new(Type::NoType),
                });
            } else {
                self.type_expr(a, &Type::NoType);
                arg_tys.push(a.ty.clone());
            }
        }

        if !self.library_abi {
            if let TreeKind::Select { name, qual } = &fun.kind {
                if name == "+"
                    && (matches!(qual.ty, Type::String)
                        || arg_tys.first().is_some_and(|t| matches!(t, Type::String)))
                {
                    tree.ty = Type::String;
                    fun.sym = SymbolId::NONE;
                    return;
                }
            }
        }

        let fun_ty = fun.ty.clone();
        let chosen = self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt);
        match chosen {
            Some((sym, mut param_tys, mut ret)) => {
                if !sym.is_none() {
                    fun.sym = sym;
                    tree.sym = sym;
                    if !self.st.get(sym).tparams.is_empty() {
                        let inst = self.infer_method_tparams(sym, &param_tys, &arg_tys);
                        if !inst.is_empty() {
                            let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                            let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
                            param_tys = param_tys
                                .iter()
                                .map(|p| crate::symbol::subst_tparams_slice(&tps, &args_t, p))
                                .collect();
                            ret = crate::symbol::subst_tparams_slice(&tps, &args_t, &ret);
                        }
                    }
                }
                if let Some(elem) = recv_ty.as_ref().and_then(|t| self.elem_type(t)) {
                    if matches!(
                        fun_name.as_str(),
                        "map" | "flatMap" | "foreach" | "withFilter"
                    ) && !param_tys.is_empty()
                    {
                        if let Type::Function { ret: fr, .. } = &param_tys[0] {
                            param_tys[0] = Type::Function {
                                params: vec![elem],
                                ret: fr.clone(),
                            };
                        }
                    }
                }
                for (i, a) in args.iter_mut().enumerate() {
                    let p = param_tys.get(i).cloned().unwrap_or(Type::NoType);
                    if matches!(a.kind, TreeKind::Function { .. }) || a.ty.is_no_type() {
                        self.type_expr(a, &p);
                    }
                    if !p.is_no_type() {
                        self.adapt(a, &p);
                    }
                }
                let nparams = param_tys.len();
                if args.len() > nparams {
                    self.error(
                        tree.span,
                        format!(
                            "too many arguments: expected {}, found {}",
                            nparams,
                            args.len()
                        ),
                    );
                }
                let leftover =
                    self.fill_defaults_and_implicits(tree.span, args, &param_tys, sym, &fun.ty, pt);
                let method_name = if !sym.is_none() {
                    self.st.get(sym).name.clone()
                } else {
                    fun_name.clone()
                };
                if method_name == "::" {
                    if let Some(a0) = args.first() {
                        ret = Type::Class {
                            sym: self.st.list_sym,
                            args: vec![a0.ty.clone()],
                        };
                    }
                } else if method_name == "apply"
                    && !sym.is_none()
                    && self.st.get(self.st.get(sym).owner).name.starts_with("Some")
                {
                    if let Some(a0) = args.first() {
                        ret = Type::Class {
                            sym: self.st.some_sym,
                            args: vec![a0.ty.clone()],
                        };
                    }
                } else if method_name == "map" {
                    if !self.is_with_filter_ty(recv_ty.as_ref()) {
                        if let Some(a0) = args.first() {
                            if let Type::Function { ret: fr, .. } = &a0.ty {
                                if let Some(cls) = recv_ty
                                    .as_ref()
                                    .and_then(|t| self.st.class_sym_of(t))
                                    .map(|c| self.collection_root(c))
                                {
                                    ret = Type::Class {
                                        sym: cls,
                                        args: vec![(**fr).clone()],
                                    };
                                }
                            }
                        }
                    }
                } else if method_name == "flatMap" {
                    if let Some(a0) = args.first() {
                        if let Type::Function { ret: fr, .. } = &a0.ty {
                            ret = (**fr).clone();
                        }
                    }
                } else if method_name == "withFilter" {
                    if !self.is_with_filter_ty(Some(&ret)) {
                        if let Some(r) = recv_ty {
                            ret = r;
                        }
                    }
                } else if method_name == "updated" {
                    if let Some(cls) = recv_ty.as_ref().and_then(|t| self.st.class_sym_of(t)) {
                        let n = self.st.get(cls).name.as_str();
                        if n == "Map" && args.len() >= 2 {
                            ret = Type::Class {
                                sym: cls,
                                args: vec![args[0].ty.clone(), args[1].ty.clone()],
                            };
                        } else if n == "Vector" && args.len() >= 2 {
                            ret = Type::Class {
                                sym: cls,
                                args: vec![args[1].ty.clone()],
                            };
                        }
                    }
                } else if method_name == ":+" {
                    if let Some(cls) = recv_ty.as_ref().and_then(|t| self.st.class_sym_of(t)) {
                        if self.st.get(cls).name == "Vector" {
                            if let Some(a0) = args.first() {
                                ret = Type::Class {
                                    sym: cls,
                                    args: vec![a0.ty.clone()],
                                };
                            }
                        }
                    }
                }
                tree.ty = leftover.unwrap_or(ret);
            }
            None => {
                if self.rewrite_apply_extension(fun) {
                    let fun_ty = fun.ty.clone();
                    if let Some((sym, param_tys, ret)) =
                        self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt)
                    {
                        fun.sym = sym;
                        tree.sym = sym;
                        for (i, a) in args.iter_mut().enumerate() {
                            let p = param_tys.get(i).cloned().unwrap_or(Type::NoType);
                            if matches!(a.kind, TreeKind::Function { .. }) || a.ty.is_no_type() {
                                self.type_expr(a, &p);
                            }
                            if !p.is_no_type() {
                                self.adapt(a, &p);
                            }
                        }
                        tree.ty = ret;
                        return;
                    }
                }
                self.error(
                    tree.span,
                    format!(
                        "no matching overload for {} with arguments ({})",
                        self.st.display_type(&fun_ty),
                        arg_tys
                            .iter()
                            .map(|t| self.st.display_type(t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
                tree.ty = Type::Error;
            }
        }
    }

    fn collection_root(&self, id: SymbolId) -> SymbolId {
        let n = self.st.get(id).name.as_str();
        if n == "Some" || n == "None$" || n == "None" {
            self.st.option_sym
        } else if n == "$colon$colon" || n == "Nil$" || n == "Nil" || n == "::" {
            self.st.list_sym
        } else {
            id
        }
    }

    fn is_with_filter_ty(&self, ty: Option<&Type>) -> bool {
        let Some(ty) = ty else {
            return false;
        };
        let Some(id) = self.st.class_sym_of(ty) else {
            return false;
        };
        let n = self.st.get(id).name.as_str();
        n == "WithFilter" || n == "Option$WithFilter"
    }

    fn elem_type(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Class { sym, args } if args.len() == 2 && self.st.get(*sym).name == "Map" => {
                Some(Type::Class {
                    sym: self.tuple2_sym(),
                    args: args.clone(),
                })
            }
            Type::Class { args, .. } if !args.is_empty() => Some(args[0].clone()),
            Type::ModuleRef(id) => {
                let name = self.st.get(*id).name.as_str();
                if name == "Nil$" || name == "None$" {
                    Some(Type::Nothing)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn tuple2_sym(&self) -> SymbolId {
        self.st
            .lookup("Tuple2")
            .into_iter()
            .find(|id| self.st.get(*id).kind == crate::symbol::SymKind::Class)
            .unwrap_or(SymbolId::NONE)
    }

    fn infer_method_tparams(
        &self,
        method: SymbolId,
        param_tys: &[Type],
        arg_tys: &[Type],
    ) -> Vec<(SymbolId, Type)> {
        let tps = self.st.get(method).tparams.clone();
        let mut out = Vec::new();
        for tp in tps {
            if let Some(t) = unify_tparam(tp, param_tys, arg_tys) {
                out.push((tp, t));
            }
        }
        out
    }

    fn first_clause_ids(&self, fun: &Tree) -> Vec<SymbolId> {
        if fun.sym.is_none() {
            return Vec::new();
        }
        let s = self.st.get(fun.sym);
        if s.paramss.is_empty() {
            return s.params.clone();
        }
        match &fun.ty {
            Type::Method { paramss, .. } if paramss.len() < s.paramss.len() => {
                let drop = s.paramss.len() - paramss.len();
                s.paramss.get(drop).cloned().unwrap_or_default()
            }
            _ => s.paramss.first().cloned().unwrap_or_default(),
        }
    }

    fn named_arg_parts(arg: &Tree) -> Option<(String, Tree)> {
        if let TreeKind::Assign { lhs, rhs } = &arg.kind {
            if let TreeKind::Ident { name } = &lhs.kind {
                return Some((name.clone(), (**rhs).clone()));
            }
        }
        None
    }

    fn reorder_named_args(&mut self, args: &mut Vec<Tree>, fun: &Tree) {
        if !args.iter().any(|a| Self::named_arg_parts(a).is_some()) {
            return;
        }
        let ids = self.first_clause_ids(fun);
        if ids.is_empty() {
            self.error(
                args.first().map(|a| a.span).unwrap_or(fun.span),
                "unimplemented syntax: named arguments (method parameters not resolved)",
            );
            return;
        }
        let names: Vec<String> = ids.iter().map(|id| self.st.get(*id).name.clone()).collect();
        let mut slots: Vec<Option<Tree>> = names.iter().map(|_| None).collect();
        let mut positional = 0usize;
        let taken = std::mem::take(args);
        for a in taken {
            if let Some((n, rhs)) = Self::named_arg_parts(&a) {
                match names.iter().position(|p| p == &n) {
                    Some(i) => {
                        if slots[i].is_some() {
                            self.error(a.span, format!("parameter `{n}` is already specified"));
                        }
                        slots[i] = Some(rhs);
                    }
                    None => {
                        self.error(a.span, format!("no parameter named `{n}`"));
                    }
                }
            } else {
                if positional >= slots.len() {
                    self.error(a.span, "too many arguments");
                    args.push(a);
                    continue;
                }
                if slots[positional].is_some() {
                    self.error(
                        a.span,
                        format!(
                            "positional argument overlaps named parameter `{}`",
                            names[positional]
                        ),
                    );
                }
                slots[positional] = Some(a);
                positional += 1;
            }
        }
        let mut out = Vec::new();
        for (i, slot) in slots.into_iter().enumerate() {
            match slot {
                Some(t) => out.push(t),
                None => {
                    let pid = ids[i];
                    let ps = self.st.get(pid);
                    if ps.flags.contains(Flags::DEFAULTPARAM) {
                        if let Some(rhs) = ps.default_rhs.clone() {
                            out.push(rhs);
                        }
                    } else if ps.flags.contains(Flags::IMPLICIT) {
                        // leave a hole; fill_defaults will search
                        break;
                    } else {
                        self.error(
                            fun.span,
                            format!("missing argument for parameter `{}`", names[i]),
                        );
                    }
                }
            }
        }
        *args = out;
    }

    fn fill_defaults_and_implicits(
        &mut self,
        span: Span,
        args: &mut Vec<Tree>,
        param_tys: &[Type],
        sym: SymbolId,
        fun_ty: &Type,
        pt: &Type,
    ) -> Option<Type> {
        if sym.is_none() {
            return None;
        }
        let s_paramss = self.st.get(sym).paramss.clone();
        let s_params = self.st.get(sym).params.clone();
        let paramss_ids: Vec<Vec<SymbolId>> = if !s_paramss.is_empty() {
            match fun_ty {
                Type::Method { paramss, .. } if paramss.len() < s_paramss.len() => {
                    let drop = s_paramss.len() - paramss.len();
                    s_paramss[drop..].to_vec()
                }
                _ => s_paramss.clone(),
            }
        } else if !s_params.is_empty() {
            vec![s_params]
        } else {
            return None;
        };
        let first = paramss_ids.first().cloned().unwrap_or_default();
        if args.len() < first.len() {
            let rest = first[args.len()..].to_vec();
            let all_implicit = rest
                .iter()
                .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT));
            let all_default = rest
                .iter()
                .all(|p| self.st.get(*p).flags.contains(Flags::DEFAULTPARAM));
            if all_implicit && !matches!(pt, Type::Method { .. } | Type::Function { .. }) {
                let off = args.len().min(param_tys.len());
                self.fill_implicit_params(span, args, &param_tys[off..], &rest);
            } else if all_default {
                for pid in rest {
                    if let Some(mut rhs) = self.st.get(pid).default_rhs.clone() {
                        let pty = self.st.get(pid).ty.clone();
                        self.type_expr(&mut rhs, &pty);
                        self.adapt(&mut rhs, &pty);
                        args.push(rhs);
                    }
                }
            } else if !matches!(pt, Type::Method { .. } | Type::Function { .. }) {
                self.error(
                    span,
                    format!(
                        "not enough arguments: expected {}, found {}",
                        first.len(),
                        args.len()
                    ),
                );
            }
        }
        if paramss_ids.len() > 1 {
            let rest_ids: Vec<SymbolId> = paramss_ids[1..].iter().flatten().copied().collect();
            let all_impl = !rest_ids.is_empty()
                && rest_ids
                    .iter()
                    .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT));
            if all_impl && !matches!(pt, Type::Method { .. } | Type::Function { .. }) {
                let rest_tys: Vec<Type> = rest_ids
                    .iter()
                    .map(|id| self.st.get(*id).ty.clone())
                    .collect();
                self.fill_implicit_params(span, args, &rest_tys, &rest_ids);
                return None;
            }
            let rest_tys: Vec<Vec<Type>> = paramss_ids[1..]
                .iter()
                .map(|clause| {
                    clause
                        .iter()
                        .map(|id| self.st.get(*id).ty.clone())
                        .collect()
                })
                .collect();
            let ret = match fun_ty {
                Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
                _ => match &self.st.get(sym).ty {
                    Type::Method { ret, .. } => (**ret).clone(),
                    _ => Type::NoType,
                },
            };
            return Some(Type::Method {
                paramss: rest_tys,
                ret: Box::new(ret),
            });
        }
        None
    }

    fn fill_implicit_params(
        &mut self,
        span: Span,
        args: &mut Vec<Tree>,
        param_tys: &[Type],
        rest: &[SymbolId],
    ) {
        for (i, pid) in rest.iter().enumerate() {
            let pty = param_tys
                .get(i)
                .cloned()
                .unwrap_or_else(|| self.st.get(*pid).ty.clone());
            match self.search_implicit(&pty) {
                ImplicitSearch::Found(id) => {
                    let mut r = self.ref_implicit(id, span);
                    self.adapt(&mut r, &pty);
                    args.push(r);
                }
                ImplicitSearch::None => {
                    self.error(
                        span,
                        format!(
                            "no implicit: could not find implicit value of type {}",
                            self.st.display_type(&pty)
                        ),
                    );
                }
                ImplicitSearch::Ambiguous(ids) => {
                    self.error(
                        span,
                        format!("ambiguous implicit: {}", self.describe_implicits(&ids)),
                    );
                }
            }
        }
    }

    fn resolve_overload(
        &self,
        fun_ty: &Type,
        fun_sym: SymbolId,
        arg_tys: &[Type],
        _pt: &Type,
    ) -> Option<(SymbolId, Vec<Type>, Type)> {
        let mut cands: Vec<(SymbolId, Vec<Type>, Type)> = Vec::new();
        match fun_ty {
            Type::Method { paramss, ret } => {
                let ps = paramss.first().cloned().unwrap_or_default();
                cands.push((fun_sym, ps, (**ret).clone()));
            }
            Type::Function { params, ret } => {
                cands.push((fun_sym, params.clone(), (**ret).clone()));
            }
            Type::Overload(alts) => {
                for a in alts {
                    if let Type::Method { paramss, ret } = a {
                        cands.push((
                            fun_sym,
                            paramss.first().cloned().unwrap_or_default(),
                            (**ret).clone(),
                        ));
                    }
                }
                // recover real symbols from owner
                if !fun_sym.is_none() {
                    let name = self.st.get(fun_sym).name.clone();
                    let owner = self.st.get(fun_sym).owner;
                    cands.clear();
                    for m in self.st.lookup_member(owner, &name) {
                        if let Type::Method { paramss, ret } = &self.st.get(m).ty {
                            cands.push((
                                m,
                                paramss.first().cloned().unwrap_or_default(),
                                (**ret).clone(),
                            ));
                        }
                    }
                    // also same-scope overloads
                    if cands.is_empty() {
                        for m in self.st.lookup(&name) {
                            if let Type::Method { paramss, ret } = &self.st.get(m).ty {
                                cands.push((
                                    m,
                                    paramss.first().cloned().unwrap_or_default(),
                                    (**ret).clone(),
                                ));
                            }
                        }
                    }
                }
            }
            Type::Class { sym, .. } => {
                // apply on companion or ctor
                let apply = self.st.lookup_member(*sym, "apply");
                for m in apply {
                    if let Type::Method { paramss, ret } = &self.st.get(m).ty {
                        cands.push((
                            m,
                            paramss.first().cloned().unwrap_or_default(),
                            (**ret).clone(),
                        ));
                    }
                }
            }
            Type::ModuleRef(id) => {
                for m in self.st.lookup_member(*id, "apply") {
                    if let Type::Method { paramss, ret } = &self.st.get(m).ty {
                        cands.push((
                            m,
                            paramss.first().cloned().unwrap_or_default(),
                            (**ret).clone(),
                        ));
                    }
                }
            }
            _ => return None,
        }
        // score
        let mut best: Option<(i32, (SymbolId, Vec<Type>, Type))> = None;
        for (sym, ps, ret) in cands {
            if let Some(score) = self.compat_score(&ps, arg_tys) {
                match &best {
                    None => best = Some((score, (sym, ps, ret))),
                    Some((b, _)) if score > *b => best = Some((score, (sym, ps, ret))),
                    Some((b, _)) if score == *b => {
                        // ambiguous — keep first (scalac would error; we pick more specific later)
                    }
                    _ => {}
                }
            }
        }
        best.map(|(_, v)| v)
    }

    fn compat_score(&self, params: &[Type], args: &[Type]) -> Option<i32> {
        if args.len() > params.len() {
            return None;
        }
        // fewer args ok only if we don't know defaults — require equal for scoring
        if args.len() != params.len() {
            // still allow if extra params might have defaults; score lower
            if args.len() < params.len() {
                let mut s = 0;
                for (a, p) in args.iter().zip(params) {
                    s += self.arg_score(a, p)?;
                }
                return Some(s - 10 * (params.len() as i32 - args.len() as i32));
            }
            return None;
        }
        let mut s = 0;
        for (a, p) in args.iter().zip(params) {
            s += self.arg_score(a, p)?;
        }
        Some(s)
    }

    fn arg_score(&self, arg: &Type, param: &Type) -> Option<i32> {
        if let Type::ByName(inner) = param {
            return self.arg_score(arg, inner);
        }
        if self.st.is_sub_type(arg, param) {
            return Some(if arg == param { 10 } else { 5 });
        }
        if matches!(arg, Type::Function { .. }) && matches!(param, Type::Function { .. }) {
            return Some(8);
        }
        if matches!(param, Type::TypeParam(_)) || matches!(arg, Type::TypeParam(_)) {
            return Some(2);
        }
        if numeric_widen(arg, param).is_some() {
            return Some(3);
        }
        if matches!(param, Type::Any | Type::AnyRef | Type::AnyVal) {
            return Some(1);
        }
        None
    }

    fn type_function(&mut self, vparams: &mut Vec<Tree>, body: &mut Tree, pt: &Type) -> Type {
        let (pts, ret_pt) = match pt {
            Type::Function { params, ret } => (params.clone(), (**ret).clone()),
            Type::Named { name, args } if name.starts_with("Function") => {
                if args.is_empty() {
                    (vec![Type::NoType; vparams.len()], Type::NoType)
                } else {
                    let ret = args.last().cloned().unwrap_or(Type::NoType);
                    (args[..args.len() - 1].to_vec(), ret)
                }
            }
            _ => (vec![Type::NoType; vparams.len()], Type::NoType),
        };
        self.st.push_scope();
        let mut param_tys = Vec::new();
        for (i, p) in vparams.iter_mut().enumerate() {
            self.type_val_sig(p);
            if p.ty.is_no_type() {
                p.ty = pts.get(i).cloned().unwrap_or(Type::NoType);
                if p.ty.is_no_type() {
                    self.error(p.span, "missing parameter type for lambda (specify it or provide an expected function type)");
                    p.ty = Type::Error;
                }
            }
            if p.sym.is_none() {
                if let TreeKind::ValDef { name, mods, .. } = &p.kind {
                    let n = name.clone();
                    let flags = mods.flags.with(Flags::PARAM);
                    let id = self
                        .st
                        .alloc(n.clone(), self.st.owner, SymKind::Term, flags, "");
                    p.sym = id;
                    let _ = n;
                }
            }
            if !p.sym.is_none() {
                self.st.get_mut(p.sym).ty = p.ty.clone();
                self.st.enter_in_current(p.name().unwrap_or("_"), p.sym);
            }
            param_tys.push(p.ty.clone());
        }
        self.type_expr(body, &ret_pt);
        let ret = if ret_pt.is_no_type() {
            body.ty.clone()
        } else {
            self.adapt(body, &ret_pt);
            ret_pt
        };
        self.st.pop_scope();
        Type::Function {
            params: param_tys,
            ret: Box::new(ret),
        }
    }

    fn type_match(&mut self, tree: &mut Tree, pt: &Type) {
        let (sel, cases) = match &mut tree.kind {
            TreeKind::Match { selector, cases } => (selector, cases),
            _ => return,
        };
        self.type_expr(sel, &Type::NoType);
        let sel_ty = sel.ty.clone();
        let mut res = Type::Nothing;
        for c in cases.iter_mut() {
            self.st.push_scope();
            self.type_pattern(&mut c.pat, &sel_ty);
            if !c.guard.is_empty() {
                self.type_expr(&mut c.guard, &Type::Boolean);
            }
            self.type_expr(&mut c.body, pt);
            res = lub(&res, &c.body.ty);
            self.st.pop_scope();
        }
        let span = tree.span;
        tree.ty = if pt.is_no_type() { res } else { pt.clone() };
        if let TreeKind::Match { cases, .. } = &tree.kind {
            self.check_match_exhaustive(span, &sel_ty, cases);
        }
    }

    fn type_case(&mut self, c: &mut CaseDef, pt: &Type) {
        self.st.push_scope();
        self.type_pattern(&mut c.pat, &Type::Any);
        if !c.guard.is_empty() {
            self.type_expr(&mut c.guard, &Type::Boolean);
        }
        self.type_expr(&mut c.body, pt);
        self.st.pop_scope();
    }

    fn type_pattern(&mut self, pat: &mut Tree, sel_ty: &Type) {
        match &mut pat.kind {
            TreeKind::Wildcard => {
                pat.ty = sel_ty.clone();
            }
            TreeKind::Literal { lit } => {
                pat.ty = match lit {
                    Lit::Int(_) => Type::Int,
                    Lit::Boolean(_) => Type::Boolean,
                    Lit::String(_) => Type::String,
                    Lit::Long(_) => Type::Long,
                    Lit::Double(_) => Type::Double,
                    Lit::Char(_) => Type::Char,
                    Lit::Null => Type::Null,
                    Lit::Unit => Type::Unit,
                    _ => Type::Any,
                };
            }
            TreeKind::Ident { name } => {
                // Stable id vs variable: if name is a known module/val, treat as stable.
                let found = self.st.lookup(name);
                let is_varid = name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_lowercase() || c == '_');
                if !found.is_empty() && !is_varid {
                    pat.sym = found[0];
                    pat.ty = self.st.get(found[0]).ty.clone();
                } else {
                    let n = name.clone();
                    let id =
                        self.st
                            .alloc(n.clone(), self.st.owner, SymKind::Term, Flags::PARAM, "");
                    self.st.get_mut(id).ty = sel_ty.clone();
                    self.st.enter_in_current(&n, id);
                    pat.sym = id;
                    pat.ty = sel_ty.clone();
                }
            }
            TreeKind::Bind { name, body } => {
                self.type_pattern(body, sel_ty);
                let n = name.clone();
                let id = self
                    .st
                    .alloc(n.clone(), self.st.owner, SymKind::Term, Flags::PARAM, "");
                self.st.get_mut(id).ty = body.ty.clone();
                self.st.enter_in_current(&n, id);
                pat.sym = id;
                pat.ty = body.ty.clone();
            }
            TreeKind::Apply { fun, args } => {
                self.type_expr(fun, &Type::NoType);
                let class_id = fun.name().and_then(|n| {
                    self.st
                        .lookup(n)
                        .into_iter()
                        .find(|s| self.st.get(*s).kind == SymKind::Class)
                });
                if class_id.is_some() {
                    self.reorder_named_pattern_args(args, class_id.unwrap());
                }
                let has_star = args.iter().any(pattern_has_star);
                if has_star {
                    if let Some(last) = args.last() {
                        if !pattern_has_star(last) {
                            self.error(pat.span, "`_*` must be the last pattern argument");
                        }
                    }
                    for a in args.iter().take(args.len().saturating_sub(1)) {
                        if pattern_has_star(a) {
                            self.error(a.span, "`_*` must be the last pattern argument");
                        }
                    }
                }
                let unapply = self.find_unapply(fun);
                let unapply_seq = self.find_unapply_seq(fun);
                let use_ctor = !has_star
                    && class_id.is_some_and(|c| {
                        let s = self.st.get(c);
                        s.flags.contains(Flags::CASE) || !s.ctor_fields.is_empty()
                    });
                if use_ctor {
                    let class_id = class_id.unwrap();
                    let fields = self.st.get(class_id).ctor_fields.clone();
                    let class_ty = Type::Class {
                        sym: class_id,
                        args: vec![],
                    };
                    for (i, a) in args.iter_mut().enumerate() {
                        let ft = fields
                            .get(i)
                            .map(|f| self.st.get(*f).ty.clone())
                            .unwrap_or(Type::Any);
                        self.type_pattern(a, &ft);
                    }
                    pat.ty = class_ty;
                    pat.sym = class_id;
                } else if let Some(u) = unapply.filter(|_| !has_star) {
                    let extracted = self.unapply_extracted_types(u);
                    if args.len() != extracted.len() && !extracted.is_empty() {
                        self.error(
                            pat.span,
                            format!(
                                "extractor {} expects {} argument(s), found {}",
                                self.st.get(u).name,
                                extracted.len(),
                                args.len()
                            ),
                        );
                    }
                    for (i, a) in args.iter_mut().enumerate() {
                        let ft = extracted.get(i).cloned().unwrap_or(Type::Any);
                        self.type_pattern(a, &ft);
                    }
                    let fun = std::mem::replace(fun, Box::new(Tree::dummy(TreeKind::Empty)));
                    let args = std::mem::take(args);
                    pat.kind = TreeKind::UnApply { fun, args };
                    pat.sym = u;
                    pat.ty = sel_ty.clone();
                } else if let Some(u) = unapply_seq {
                    let elem = match sel_ty {
                        Type::Class { sym, args }
                            if *sym == self.st.list_sym && !args.is_empty() =>
                        {
                            args[0].clone()
                        }
                        _ => self.unapply_seq_elem_type(u),
                    };
                    let n = args.len();
                    for (i, a) in args.iter_mut().enumerate() {
                        if pattern_has_star(a) {
                            let list_ty = Type::Class {
                                sym: self.st.list_sym,
                                args: vec![elem.clone()],
                            };
                            self.type_pattern(a, &list_ty);
                        } else if has_star && i + 1 == n {
                            self.type_pattern(a, sel_ty);
                        } else {
                            self.type_pattern(a, &elem);
                        }
                    }
                    let fun = std::mem::replace(fun, Box::new(Tree::dummy(TreeKind::Empty)));
                    let args = std::mem::take(args);
                    pat.kind = TreeKind::UnApply { fun, args };
                    pat.sym = u;
                    pat.ty = sel_ty.clone();
                } else if let Some(c) = class_id {
                    let fields = self.st.get(c).ctor_fields.clone();
                    for (i, a) in args.iter_mut().enumerate() {
                        let ft = fields
                            .get(i)
                            .map(|f| self.st.get(*f).ty.clone())
                            .unwrap_or(Type::Any);
                        self.type_pattern(a, &ft);
                    }
                    pat.ty = Type::Class {
                        sym: c,
                        args: vec![],
                    };
                    pat.sym = c;
                } else {
                    self.error(
                        pat.span,
                        format!("not found: extractor {}", fun.name().unwrap_or("<pattern>")),
                    );
                    pat.ty = sel_ty.clone();
                }
            }
            TreeKind::Star { elem } => {
                self.type_pattern(elem, sel_ty);
                pat.ty = sel_ty.clone();
            }
            TreeKind::UnApply { fun, args } => {
                self.type_expr(fun, &Type::NoType);
                let u = if pat.sym.is_none() {
                    self.find_unapply(fun).unwrap_or(SymbolId::NONE)
                } else {
                    pat.sym
                };
                let extracted = if u.is_none() {
                    vec![Type::Any; args.len()]
                } else {
                    self.unapply_extracted_types(u)
                };
                for (i, a) in args.iter_mut().enumerate() {
                    let ft = extracted.get(i).cloned().unwrap_or(Type::Any);
                    self.type_pattern(a, &ft);
                }
                pat.sym = u;
                pat.ty = sel_ty.clone();
            }
            TreeKind::Typed { expr, tpt } => {
                let ty = self.tree_to_type(tpt);
                self.type_pattern(expr, &ty);
                pat.ty = ty;
            }
            TreeKind::Alternative { trees } => {
                for t in trees {
                    self.type_pattern(t, sel_ty);
                }
                pat.ty = sel_ty.clone();
            }
            _ => {
                pat.ty = sel_ty.clone();
            }
        }
    }

    fn register_sealed_from_namer(&mut self, tree: &Tree) {
        match &tree.kind {
            TreeKind::PackageDef { stats, .. } => {
                for s in stats {
                    self.register_sealed_from_namer(s);
                }
            }
            TreeKind::ClassDef { .. } => {
                if !tree.sym.is_none() {
                    self.register_sealed_child(tree.sym);
                }
                if let TreeKind::ClassDef { impl_, .. } = &tree.kind {
                    for s in &impl_.body {
                        self.register_sealed_from_namer(s);
                    }
                }
            }
            TreeKind::ModuleDef { .. } => {
                if !tree.sym.is_none() {
                    let cls = self.st.module_class_of(tree.sym);
                    self.register_sealed_child(cls);
                }
                if let TreeKind::ModuleDef { impl_, .. } = &tree.kind {
                    for s in &impl_.body {
                        self.register_sealed_from_namer(s);
                    }
                }
            }
            _ => {}
        }
    }

    fn find_class_named(&self, child: SymbolId, name: &str) -> Option<SymbolId> {
        let owner = self.st.get(child).owner;
        if !owner.is_none() {
            if let Some(id) = self.st.get(owner).members.iter().copied().find(|&m| {
                let s = self.st.get(m);
                s.name == name && s.is_class_like()
            }) {
                return Some(id);
            }
        }
        self.st
            .lookup(name)
            .into_iter()
            .find(|s| self.st.get(*s).is_class_like())
            .or_else(|| {
                self.st
                    .symbols
                    .iter()
                    .find(|s| s.name == name && s.is_class_like())
                    .map(|s| s.id)
            })
    }

    fn register_sealed_child(&mut self, child: SymbolId) {
        let parents = self.st.get(child).parents.clone();
        for p in parents {
            let pid = match &p {
                Type::Named { name, .. } => self.find_class_named(child, name),
                other => self.st.class_sym_of(other),
            };
            if let Some(pid) = pid {
                if self.st.is_sealed(pid) && !self.st.get(pid).children.contains(&child) {
                    self.st.get_mut(pid).children.push(child);
                }
            }
        }
    }

    fn super_target(&self, this_id: SymbolId, mix: Option<&str>) -> SymbolId {
        if this_id.is_none() {
            return SymbolId::NONE;
        }
        let parents: Vec<SymbolId> = self
            .st
            .get(this_id)
            .parents
            .iter()
            .filter_map(|p| self.st.class_sym_of(p))
            .filter(|p| {
                let n = self.st.get(*p).name.as_str();
                n != "AnyRef" && n != "Any" && n != "AnyVal" && n != "Object"
            })
            .collect();
        if let Some(name) = mix {
            parents
                .iter()
                .copied()
                .find(|p| {
                    let n = self.st.get(*p).name.as_str();
                    n == name || n.trim_end_matches('$') == name
                })
                .unwrap_or(SymbolId::NONE)
        } else {
            parents.last().copied().unwrap_or(SymbolId::NONE)
        }
    }

    fn find_unapply(&self, fun: &Tree) -> Option<SymbolId> {
        let owner = if !fun.sym.is_none() {
            let s = self.st.get(fun.sym);
            match s.kind {
                SymKind::Module | SymKind::ModuleClass => self.st.module_class_of(fun.sym),
                SymKind::Class => self
                    .st
                    .companion_module(fun.sym)
                    .map(|m| self.st.module_class_of(m))
                    .unwrap_or(SymbolId::NONE),
                _ => SymbolId::NONE,
            }
        } else if let Some(n) = fun.name() {
            let found = self.st.lookup(n);
            found
                .into_iter()
                .find(|s| matches!(self.st.get(*s).kind, SymKind::Module | SymKind::ModuleClass))
                .map(|m| self.st.module_class_of(m))
                .unwrap_or(SymbolId::NONE)
        } else {
            self.st.class_sym_of(&fun.ty).unwrap_or(SymbolId::NONE)
        };
        if owner.is_none() {
            return None;
        }
        self.st
            .lookup_member(owner, "unapply")
            .into_iter()
            .find(|m| self.st.get(*m).kind == SymKind::Method)
    }

    fn find_unapply_seq(&self, fun: &Tree) -> Option<SymbolId> {
        let owner = if !fun.sym.is_none() {
            let s = self.st.get(fun.sym);
            match s.kind {
                SymKind::Module | SymKind::ModuleClass => self.st.module_class_of(fun.sym),
                SymKind::Class => self
                    .st
                    .companion_module(fun.sym)
                    .map(|m| self.st.module_class_of(m))
                    .unwrap_or(SymbolId::NONE),
                _ => SymbolId::NONE,
            }
        } else if let Some(n) = fun.name() {
            let found = self.st.lookup(n);
            found
                .into_iter()
                .find(|s| matches!(self.st.get(*s).kind, SymKind::Module | SymKind::ModuleClass))
                .map(|m| self.st.module_class_of(m))
                .unwrap_or(SymbolId::NONE)
        } else {
            self.st.class_sym_of(&fun.ty).unwrap_or(SymbolId::NONE)
        };
        if owner.is_none() {
            return None;
        }
        self.st
            .lookup_member(owner, "unapplySeq")
            .into_iter()
            .find(|m| self.st.get(*m).kind == SymKind::Method)
    }

    fn unapply_seq_elem_type(&self, unapply: SymbolId) -> Type {
        let extracted = self.unapply_extracted_types(unapply);
        let inner = extracted.into_iter().next().unwrap_or(Type::Any);
        match inner {
            Type::Class { sym, args }
                if sym == self.st.list_sym || self.st.get(sym).name == "List" =>
            {
                args.first().cloned().unwrap_or(Type::Any)
            }
            Type::Class { args, .. } if !args.is_empty() => args[0].clone(),
            other => other,
        }
    }

    fn reorder_named_pattern_args(&mut self, args: &mut Vec<Tree>, class_id: SymbolId) {
        if !args
            .iter()
            .any(|a| matches!(a.kind, TreeKind::Assign { .. }))
        {
            return;
        }
        let fields = self.st.get(class_id).ctor_fields.clone();
        if fields.is_empty() {
            self.error(
                args.first().map(|a| a.span).unwrap_or(Span::DUMMY),
                "named extractor arguments require constructor parameter names",
            );
            return;
        }
        let names: Vec<String> = fields
            .iter()
            .map(|f| self.st.get(*f).name.clone())
            .collect();
        let mut slots: Vec<Option<Tree>> = names.iter().map(|_| None).collect();
        let taken = std::mem::take(args);
        for a in taken {
            if let TreeKind::Assign { lhs, rhs } = &a.kind {
                if let TreeKind::Ident { name } = &lhs.kind {
                    match names.iter().position(|p| p == name) {
                        Some(i) => {
                            if slots[i].is_some() {
                                self.error(
                                    a.span,
                                    format!("parameter `{name}` is already specified"),
                                );
                            }
                            slots[i] = Some((**rhs).clone());
                        }
                        None => self.error(a.span, format!("no parameter named `{name}`")),
                    }
                    continue;
                }
            }
            self.error(
                a.span,
                "positional and named extractor arguments cannot be mixed",
            );
        }
        *args = slots
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                s.unwrap_or_else(|| {
                    self.error(
                        Span::DUMMY,
                        format!("missing extractor argument `{}`", names[i]),
                    );
                    Tree::dummy(TreeKind::Wildcard)
                })
            })
            .collect();
    }

    fn unapply_extracted_types(&self, unapply: SymbolId) -> Vec<Type> {
        let ret = match &self.st.get(unapply).ty {
            Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
            t => t.clone(),
        };
        if matches!(ret, Type::Boolean) {
            return vec![];
        }
        if let Type::Class { sym, args } = &ret {
            let name = self.st.get(*sym).name.as_str();
            if *sym == self.st.option_sym || name == "Option" || name == "Some" {
                let inner = args.first().cloned().unwrap_or(Type::Any);
                return self.flatten_extract(inner);
            }
        }
        self.flatten_extract(ret)
    }

    fn flatten_extract(&self, inner: Type) -> Vec<Type> {
        match inner {
            Type::Tuple(ts) => ts,
            Type::Class { sym, args } => {
                let n = self.st.get(sym).name.as_str();
                if n == "Tuple2" || n.starts_with("Tuple") {
                    if args.is_empty() {
                        vec![Type::Any, Type::Any]
                    } else {
                        args
                    }
                } else {
                    vec![Type::Class { sym, args }]
                }
            }
            other => vec![other],
        }
    }

    fn check_match_exhaustive(&mut self, span: Span, sel_ty: &Type, cases: &[CaseDef]) {
        let Some(cls) = self.st.class_sym_of(sel_ty) else {
            return;
        };
        if !self.st.is_sealed(cls) {
            return;
        }
        if cases
            .iter()
            .any(|c| c.guard.is_empty() && self.pattern_is_catchall(&c.pat))
        {
            return;
        }
        let leaves = self.st.sealed_leaves(cls);
        if leaves.is_empty() {
            return;
        }
        let mut missing = Vec::new();
        for leaf in &leaves {
            if !cases
                .iter()
                .any(|c| c.guard.is_empty() && self.pattern_covers(&c.pat, *leaf))
            {
                missing.push(self.st.get(*leaf).name.trim_end_matches('$').to_string());
            }
        }
        if !missing.is_empty() {
            self.warning(
                span,
                format!(
                    "match may not be exhaustive. It would fail on the following input: {}",
                    missing.join(", ")
                ),
            );
        }
    }

    fn pattern_is_catchall(&self, pat: &Tree) -> bool {
        match &pat.kind {
            TreeKind::Wildcard | TreeKind::Empty => true,
            TreeKind::Bind { body, .. } => self.pattern_is_catchall(body),
            TreeKind::Ident { name } => {
                let is_varid = name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_lowercase() || c == '_');
                is_varid && (pat.sym.is_none() || self.st.get(pat.sym).kind == SymKind::Term)
            }
            TreeKind::Typed { expr, .. } => self.pattern_is_catchall(expr),
            _ => false,
        }
    }

    fn pattern_covers(&self, pat: &Tree, leaf: SymbolId) -> bool {
        if self.pattern_is_catchall(pat) {
            return true;
        }
        match &pat.kind {
            TreeKind::Typed { .. } => {
                if let Some(ps) = self.st.class_sym_of(&pat.ty) {
                    ps == leaf || self.st.is_sub_type(&self.st.type_of_class(leaf), &pat.ty)
                } else {
                    false
                }
            }
            TreeKind::Ident { .. } => {
                if pat.sym.is_none() {
                    return false;
                }
                let s = self.st.get(pat.sym);
                match s.kind {
                    SymKind::Module | SymKind::ModuleClass => {
                        self.st.module_class_of(pat.sym) == leaf
                    }
                    SymKind::Class => pat.sym == leaf,
                    _ => false,
                }
            }
            TreeKind::Apply { .. } | TreeKind::UnApply { .. } => {
                if pat.sym.is_none() {
                    return false;
                }
                let s = self.st.get(pat.sym);
                if s.kind == SymKind::Class {
                    pat.sym == leaf
                } else if s.name == "unapply" {
                    let owner = s.owner;
                    let name = self.st.get(owner).name.trim_end_matches('$').to_string();
                    self.st.get(leaf).name.trim_end_matches('$') == name
                } else {
                    false
                }
            }
            TreeKind::Bind { body, .. } => self.pattern_covers(body, leaf),
            TreeKind::Alternative { trees } => trees.iter().any(|t| self.pattern_covers(t, leaf)),
            _ => false,
        }
    }

    fn tree_to_type(&self, tpt: &Tree) -> Type {
        match &tpt.kind {
            TreeKind::Empty => Type::NoType,
            TreeKind::Ident { name } => self.resolve_type_name(name, &[]),
            TreeKind::Select { name, qual: _ } => {
                // java.lang.String etc.
                if name == "String" {
                    Type::String
                } else {
                    self.resolve_type_name(name, &[])
                }
            }
            TreeKind::AppliedTypeTree { tpt, args } => {
                let as_: Vec<Type> = args.iter().map(|a| self.tree_to_type(a)).collect();
                match tpt.name() {
                    Some("Array") => {
                        Type::Array(Box::new(as_.first().cloned().unwrap_or(Type::Any)))
                    }
                    Some("Option") => Type::Class {
                        sym: self.st.option_sym,
                        args: as_,
                    },
                    Some("List") => Type::Class {
                        sym: self.st.list_sym,
                        args: as_,
                    },
                    Some("Some") => Type::Class {
                        sym: self.st.some_sym,
                        args: as_,
                    },
                    Some(n) if n.starts_with("Function") => {
                        if as_.is_empty() {
                            Type::Function {
                                params: vec![],
                                ret: Box::new(Type::Any),
                            }
                        } else {
                            let ret = Box::new(as_.last().cloned().unwrap());
                            Type::Function {
                                params: as_[..as_.len() - 1].to_vec(),
                                ret,
                            }
                        }
                    }
                    Some(n) if n.starts_with("Tuple") => Type::Tuple(as_),
                    Some(n) => self.resolve_type_name(n, &as_),
                    None => Type::Error,
                }
            }
            TreeKind::Literal { lit: Lit::Unit } => Type::Unit,
            _ => Type::Named {
                name: tpt.name().unwrap_or("?").to_string(),
                args: vec![],
            },
        }
    }

    fn resolve_type_name(&self, name: &str, args: &[Type]) -> Type {
        match name {
            "Int" => Type::Int,
            "Long" => Type::Long,
            "Double" => Type::Double,
            "Float" => Type::Float,
            "Boolean" => Type::Boolean,
            "Unit" => Type::Unit,
            "Char" => Type::Char,
            "String" => Type::String,
            "Any" => Type::Any,
            "AnyRef" => Type::AnyRef,
            "AnyVal" => Type::AnyVal,
            "Nothing" => Type::Nothing,
            "Null" => Type::Null,
            "Object" => Type::AnyRef,
            _ => {
                let found = self.st.lookup(name);
                if let Some(id) = found.into_iter().find(|s| {
                    matches!(
                        self.st.get(*s).kind,
                        SymKind::Class
                            | SymKind::ModuleClass
                            | SymKind::Module
                            | SymKind::TypeParam
                    )
                }) {
                    match self.st.get(id).kind {
                        SymKind::Module | SymKind::ModuleClass => Type::ModuleRef(id),
                        SymKind::TypeParam => Type::TypeParam(id),
                        _ => Type::Class {
                            sym: id,
                            args: args.to_vec(),
                        },
                    }
                } else {
                    Type::Named {
                        name: name.into(),
                        args: args.to_vec(),
                    }
                }
            }
        }
    }

    fn adapt(&mut self, tree: &mut Tree, pt: &Type) {
        if matches!(pt, Type::Method { .. }) {
            return;
        }
        if pt.is_no_type() || tree.ty.is_error() || pt.is_error() {
            return;
        }
        if self.st.is_sub_type(&tree.ty, pt) {
            return;
        }
        if let Some(w) = numeric_widen(&tree.ty, pt) {
            tree.ty = w;
            return;
        }
        if matches!(pt, Type::Unit) {
            // value discarded
            return;
        }
        if matches!(pt, Type::String) && !matches!(tree.ty, Type::String) {
            // allow via toString in concat contexts only — not general
        }
        if matches!(pt, Type::Any | Type::AnyRef | Type::AnyVal) {
            return;
        }
        if let Type::ByName(inner) = pt {
            if !matches!(&tree.kind, TreeKind::Function { .. }) {
                let span = tree.span;
                let inner_tree = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
                *tree = Tree {
                    id: inner_tree.id,
                    span,
                    kind: TreeKind::Function {
                        vparams: vec![],
                        body: Box::new(inner_tree),
                    },
                    ty: Type::Function {
                        params: vec![],
                        ret: inner.clone(),
                    },
                    sym: SymbolId::NONE,
                };
            }
            return;
        }
        match self.search_conversion(&tree.ty, pt) {
            ImplicitSearch::Found(id) => {
                let span = tree.span;
                let arg = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
                let fun = self.ref_implicit(id, span);
                *tree = Tree {
                    id: arg.id,
                    span,
                    kind: TreeKind::Apply {
                        fun: Box::new(fun),
                        args: vec![arg],
                    },
                    ty: pt.clone(),
                    sym: id,
                };
                return;
            }
            ImplicitSearch::Ambiguous(ids) => {
                self.error(
                    tree.span,
                    format!("ambiguous implicit: {}", self.describe_implicits(&ids)),
                );
                tree.ty = Type::Error;
                return;
            }
            ImplicitSearch::None => {}
        }
        self.error(
            tree.span,
            format!(
                "type mismatch; found: {}  required: {}",
                self.st.display_type(&tree.ty),
                self.st.display_type(pt)
            ),
        );
        tree.ty = Type::Error;
    }
}

fn pattern_has_star(pat: &Tree) -> bool {
    match &pat.kind {
        TreeKind::Star { .. } => true,
        TreeKind::Bind { body, .. } => pattern_has_star(body),
        TreeKind::Typed { expr, .. } => pattern_has_star(expr),
        _ => false,
    }
}

fn is_sub_type(a: &Type, b: &Type) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (Type::Error, _) | (_, Type::Error) => true,
        (Type::Nothing, _) => true,
        (_, Type::Any) => true,
        (
            Type::Null,
            Type::AnyRef | Type::String | Type::Array(_) | Type::Class { .. } | Type::ModuleRef(_),
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
            | Type::Function { .. },
            Type::AnyRef,
        ) => true,
        (Type::Class { sym: s1, .. }, Type::Class { sym: s2, .. }) if s1 == s2 => true,
        (Type::Array(x), Type::Array(y)) => is_sub_type(x, y),
        (Type::ModuleRef(s), Type::Class { sym, .. }) if s == sym => true,
        _ => false,
    }
}

fn numeric_widen(a: &Type, b: &Type) -> Option<Type> {
    match (a, b) {
        (Type::Int, Type::Long) => Some(Type::Long),
        (Type::Int, Type::Double) => Some(Type::Double),
        (Type::Long, Type::Double) => Some(Type::Double),
        (Type::Float, Type::Double) => Some(Type::Double),
        (Type::Int, Type::Float) => Some(Type::Float),
        _ => None,
    }
}

fn lub(a: &Type, b: &Type) -> Type {
    if is_sub_type(a, b) {
        return b.clone();
    }
    if is_sub_type(b, a) {
        return a.clone();
    }
    if matches!(a, Type::Nothing) {
        return b.clone();
    }
    if matches!(b, Type::Nothing) {
        return a.clone();
    }
    Type::Any
}

fn unify_tparam(tp: SymbolId, params: &[Type], args: &[Type]) -> Option<Type> {
    for (p, a) in params.iter().zip(args) {
        if let Some(t) = unify_one(tp, p, a) {
            return Some(t);
        }
    }
    None
}

fn unify_one(tp: SymbolId, pattern: &Type, actual: &Type) -> Option<Type> {
    match pattern {
        Type::TypeParam(id) if *id == tp => Some(actual.clone()),
        Type::Class { args: pas, .. } => {
            if let Type::Class { args: aas, .. } = actual {
                for (p, a) in pas.iter().zip(aas) {
                    if let Some(t) = unify_one(tp, p, a) {
                        return Some(t);
                    }
                }
            }
            None
        }
        Type::Function { params, ret } => {
            if let Type::Function {
                params: aps,
                ret: ar,
            } = actual
            {
                for (p, a) in params.iter().zip(aps) {
                    if let Some(t) = unify_one(tp, p, a) {
                        return Some(t);
                    }
                }
                unify_one(tp, ret, ar)
            } else {
                None
            }
        }
        Type::Array(p) => match actual {
            Type::Array(a) => unify_one(tp, p, a),
            _ => None,
        },
        Type::ByName(p) => match actual {
            Type::ByName(a) => unify_one(tp, p, a),
            _ => None,
        },
        _ => None,
    }
}

impl Typer {
    pub fn dump_typed(&self, tree: &Tree) -> String {
        scala_rs_parser::dump_tree(tree)
    }
}

/// Whether a compilation unit defines `def main(args: Array[String])` on an object.
pub fn find_mains(st: &SymbolTable, tree: &Tree) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(st: &SymbolTable, t: &Tree, out: &mut Vec<String>) {
        match &t.kind {
            TreeKind::PackageDef { stats, .. } => {
                for s in stats {
                    walk(st, s, out);
                }
            }
            TreeKind::ModuleDef { name, impl_, .. } => {
                for b in &impl_.body {
                    if let TreeKind::DefDef { name: mn, .. } = &b.kind {
                        if mn == "main" {
                            out.push(name.clone());
                        }
                    }
                }
            }
            TreeKind::ClassDef { impl_, .. } => {
                for b in &impl_.body {
                    walk(st, b, out);
                }
            }
            _ => {}
        }
    }
    walk(st, tree, &mut out);
    out
}

pub fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.level == scala_rs_span::Level::Error)
}
