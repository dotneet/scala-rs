#![allow(dead_code)]
//! Namer + typer. Trees are mutated in place (`ty`, `sym`).

use crate::prelude::install_prelude;
use crate::symbol::{SymbolTable, SymKind};
use crate::implicits::ImplicitSearch;
use scala_rs_parser::ast::*;
use scala_rs_span::{Diagnostic, Span};

pub struct Typer {
    pub st: SymbolTable,
    pub diags: Vec<Diagnostic>,
    file_index: usize,
    /// Counter for synthetic names.
    gensym: u32,
}

pub fn typecheck(tree: &mut Tree, file_index: usize) -> (SymbolTable, Vec<Diagnostic>) {
    let mut t = Typer::new(file_index);
    t.namer(tree);
    t.typer(tree);
    (t.st, t.diags)
}

impl Typer {
    pub fn new(file_index: usize) -> Self {
        let mut st = SymbolTable::new();
        install_prelude(&mut st);
        Typer {
            st,
            diags: Vec::new(),
            file_index,
            gensym: 0,
        }
    }

    fn error(&mut self, span: Span, msg: impl Into<String>) {
        self.diags
            .push(Diagnostic::error(self.file_index, span, msg));
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
                TreeKind::Ident { name } if name != "<empty>" && name != "_root_" => acc.push(name.clone()),
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
            let existing = self.st.get(cur).members.iter().copied().find(|&m| {
                self.st.get(m).kind == SymKind::Package && self.st.get(m).name == p
            });
            cur = if let Some(e) = existing {
                e
            } else {
                self.st.alloc(&p, cur, SymKind::Package, Flags::PACKAGE, &jvm)
            };
        }
        cur
    }

    fn namer_enter_tmpl(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ClassDef { name, mods, .. } => {
                let is_trait = mods.flags.contains(Flags::TRAIT);
                let flags = mods.flags.with(if is_trait { Flags::ABSTRACT } else { Flags::EMPTY });
                let jvm = self.jvm_for_current(name);
                let id = self.st.alloc(name, self.st.owner, SymKind::Class, flags, &jvm);
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
                let m = self.st.alloc(name, self.st.owner, SymKind::Module, mods.flags.with(Flags::MODULE), &jvm);
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
        if ow.kind == SymKind::Package && ow.name != "<_root_>" && !ow.jvm_name.is_empty() && ow.jvm_name != "scala/runtime" {
            format!("{}/{}", ow.jvm_name, name)
        } else if ow.kind == SymKind::Package {
            name.to_string()
        } else {
            format!("{}${}", ow.jvm_name, name)
        }
    }

    fn ensure_companion(&mut self, name: &str, class_id: SymbolId) -> SymbolId {
        let existing = self.st.lookup(name).into_iter().find(|&s| self.st.get(s).kind == SymKind::Module);
        if let Some(e) = existing {
            return e;
        }
        let jvm = format!("{}$", self.jvm_for_current(name));
        let cls = self.st.alloc(
            &format!("{name}$"),
            self.st.owner,
            SymKind::ModuleClass,
            Flags::MODULE.with(Flags::FINAL).with(Flags::SYNTHETIC).with(Flags::CASE),
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
        let ctor = self.st.alloc("<init>", id, SymKind::Method, Flags::CONSTRUCTOR, "");
        self.st.get_mut(ctor).params = fields.clone();
        self.st.enter_in_current("<init>", ctor);

        for stt in body.iter_mut() {
            self.namer_member(stt);
            if matches!(&stt.kind, TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }) {
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
        let copy = self.st.alloc("copy", class_id, SymKind::Method, Flags::SYNTHETIC, "");
        let ptys: Vec<Type> = fields.iter().map(|_| Type::NoType).collect();
        self.st.get_mut(copy).ty = Type::Method {
            paramss: vec![ptys],
            ret: Box::new(class_ty.clone()),
        };
        let _ = self.st.alloc("productArity", class_id, SymKind::Method, Flags::SYNTHETIC, "");
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
            let apply = self.st.alloc("apply", cls, SymKind::Method, Flags::SYNTHETIC.with(Flags::CASE), "");
            self.st.get_mut(apply).params = fields.clone();
            self.st.get_mut(apply).ty = Type::Method {
                paramss: vec![fields.iter().map(|_| Type::NoType).collect()],
                ret: Box::new(class_ty),
            };
            self.st.enter_in_current("apply", apply);
            // also put apply on the module value for Point(1,2)
            self.st.get_mut(m).members.push(apply);
            let unapply = self.st.alloc("unapply", cls, SymKind::Method, Flags::SYNTHETIC.with(Flags::CASE), "");
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
            if matches!(&stt.kind, TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }) {
                self.namer(stt);
            }
        }
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
    }

    fn namer_member(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ValDef { name, mods, .. } => {
                let id = self.st.alloc(name, self.st.owner, SymKind::Term, mods.flags, "");
                self.st.enter_in_current(name, id);
                tree.sym = id;
            }
            TreeKind::DefDef { name, mods, .. } => {
                let id = self.st.alloc(name, self.st.owner, SymKind::Method, mods.flags, "");
                self.st.enter_in_current(name, id);
                tree.sym = id;
            }
            TreeKind::TypeDef { name, mods, .. } => {
                let id = self.st.alloc(name, self.st.owner, SymKind::Class, mods.flags, "");
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
            TreeKind::Import { .. } => {}
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
        let body = match &mut tree.kind {
            TreeKind::ModuleDef { impl_, .. } => &mut impl_.body,
            _ => return,
        };
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
            _ => {
                self.type_stat(tree);
            }
        }
    }

    fn type_val_sig(&mut self, tree: &mut Tree) {
        let (tpt, name, flags) = match &tree.kind {
            TreeKind::ValDef { tpt, name, mods, .. } => (tpt.clone(), name.clone(), mods.flags),
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
            let id = self.st.alloc(name.clone(), self.st.owner, SymKind::Term, flags, "");
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
            let id = self.st.alloc(name.clone(), self.st.owner, SymKind::Method, Flags::EMPTY, "");
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
                    self.error(p.span, format!("missing parameter type for `{}`", p.name().unwrap_or("?")));
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
            _ => {
                self.type_expr(tree, &Type::NoType);
            }
        }
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
            TreeKind::This { .. } => {
                tree.sym = self.st.this_class;
                tree.ty = self.st.type_of_class(self.st.this_class);
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
                tree.ty = lub(&thenp.ty, &elsep.ty);
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
                        self.error(tree.span, "unimplemented syntax: anonymous classes (`new T { ... }`)");
                        tree.ty = Type::Error;
                        return;
                    }
                }
                if let TreeKind::Ident { name } = &tpt.kind {
                    let n = name.clone();
                    let found = self.st.lookup(&n);
                    if let Some(id) = found.into_iter().find(|s| self.st.get(*s).kind == SymKind::Class)
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
            TreeKind::InterpolatedString { prefix, parts, args } => {
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
                self.error(tree.span, "unimplemented syntax: placeholder `_` in expressions");
                tree.ty = Type::Error;
            }
            TreeKind::Unimplemented { what } => {
                self.error(tree.span, format!("unimplemented syntax: {what}"));
                tree.ty = Type::Error;
            }
            TreeKind::Empty => {
                tree.ty = Type::NoType;
            }
            TreeKind::Super { .. } => {
                tree.ty = Type::AnyRef;
            }
            TreeKind::AppliedTypeTree { .. } => {
                tree.ty = self.tree_to_type(tree);
            }
            TreeKind::DefDef { .. } | TreeKind::ValDef { .. } | TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => {
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
            TreeKind::DefDef { .. } | TreeKind::ValDef { .. } | TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
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
                if matches!(pt, Type::Function { .. } | Type::ByName(_) | Type::Method { .. }) {
                    ty
                } else {
                    (**inner).clone()
                }
            }
            _ => ty,
        }
    }

    fn type_select(&mut self, tree: &mut Tree, pt: &Type) {
        let (qual, name) = match &mut tree.kind {
            TreeKind::Select { qual, name } => (qual, name.clone()),
            _ => return,
        };
        self.type_expr(qual, &Type::NoType);
        if name == "_" {
            self.error(tree.span, "unimplemented syntax: wildcard import/select in expression");
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
                    if let Some(owner) = found
                        .first()
                        .map(|s| self.st.get(*s).owner)
                    {
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

    fn type_apply(&mut self, tree: &mut Tree, pt: &Type) {
        let (fun, args) = match &mut tree.kind {
            TreeKind::Apply { fun, args } => (fun, args),
            _ => return,
        };
        // new C(args)
        if matches!(&fun.kind, TreeKind::New { .. }) {
            self.type_expr(fun, &Type::NoType);
            let class_id = fun.sym.is_none().then(|| self.st.class_sym_of(&fun.ty)).flatten().or(Some(fun.sym)).filter(|s| !s.is_none());
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

        self.type_expr(fun, &Type::NoType);

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

        if let TreeKind::Select { name, qual } = &fun.kind {
            if name == "+"
                && (matches!(qual.ty, Type::String) || arg_tys.first().is_some_and(|t| matches!(t, Type::String)))
            {
                tree.ty = Type::String;
                fun.sym = SymbolId::NONE;
                return;
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
                    if matches!(fun_name.as_str(), "map" | "flatMap" | "foreach" | "withFilter")
                        && !param_tys.is_empty()
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
                self.fill_defaults_and_implicits(tree.span, args, &param_tys, sym, pt);
                let nparams = if sym.is_none() {
                    param_tys.len()
                } else {
                    self.st.get(sym).params.len().max(param_tys.len())
                };
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
                } else if method_name == "flatMap" {
                    if let Some(a0) = args.first() {
                        if let Type::Function { ret: fr, .. } = &a0.ty {
                            ret = (**fr).clone();
                        }
                    }
                } else if method_name == "withFilter" {
                    if let Some(r) = recv_ty {
                        ret = r;
                    }
                }
                tree.ty = ret;
            }
            None => {
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

    fn elem_type(&self, ty: &Type) -> Option<Type> {
        match ty {
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

    fn fill_defaults_and_implicits(
        &mut self,
        span: Span,
        args: &mut Vec<Tree>,
        param_tys: &[Type],
        sym: SymbolId,
        pt: &Type,
    ) {
        if sym.is_none() {
            return;
        }
        let params = self.st.get(sym).params.clone();
        if args.len() >= params.len() {
            return;
        }
        let rest = if args.len() < params.len() {
            params[args.len()..].to_vec()
        } else {
            return;
        };
        let all_implicit = rest.iter().all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT));
        let all_default = rest
            .iter()
            .all(|p| self.st.get(*p).flags.contains(Flags::DEFAULTPARAM));
        if all_implicit && !matches!(pt, Type::Method { .. } | Type::Function { .. }) {
            for (i, pid) in rest.iter().enumerate() {
                let pty = param_tys
                    .get(args.len() + i)
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
            return;
        }
        if all_default {
            for pid in rest {
                if let Some(mut rhs) = self.st.get(pid).default_rhs.clone() {
                    let pty = self.st.get(pid).ty.clone();
                    self.type_expr(&mut rhs, &pty);
                    self.adapt(&mut rhs, &pty);
                    args.push(rhs);
                }
            }
            return;
        }
        self.error(
            span,
            format!(
                "not enough arguments: expected {}, found {}",
                param_tys.len(),
                args.len()
            ),
        );
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
                        cands.push((fun_sym, paramss.first().cloned().unwrap_or_default(), (**ret).clone()));
                    }
                }
                // recover real symbols from owner
                if !fun_sym.is_none() {
                    let name = self.st.get(fun_sym).name.clone();
                    let owner = self.st.get(fun_sym).owner;
                    cands.clear();
                    for m in self.st.lookup_member(owner, &name) {
                        if let Type::Method { paramss, ret } = &self.st.get(m).ty {
                            cands.push((m, paramss.first().cloned().unwrap_or_default(), (**ret).clone()));
                        }
                    }
                    // also same-scope overloads
                    if cands.is_empty() {
                        for m in self.st.lookup(&name) {
                            if let Type::Method { paramss, ret } = &self.st.get(m).ty {
                                cands.push((m, paramss.first().cloned().unwrap_or_default(), (**ret).clone()));
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
                        cands.push((m, paramss.first().cloned().unwrap_or_default(), (**ret).clone()));
                    }
                }
            }
            Type::ModuleRef(id) => {
                for m in self.st.lookup_member(*id, "apply") {
                    if let Type::Method { paramss, ret } = &self.st.get(m).ty {
                        cands.push((m, paramss.first().cloned().unwrap_or_default(), (**ret).clone()));
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
                    let id = self.st.alloc(n.clone(), self.st.owner, SymKind::Term, flags, "");
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
        tree.ty = if pt.is_no_type() { res } else { pt.clone() };
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
                    let id = self.st.alloc(n.clone(), self.st.owner, SymKind::Term, Flags::PARAM, "");
                    self.st.get_mut(id).ty = sel_ty.clone();
                    self.st.enter_in_current(&n, id);
                    pat.sym = id;
                    pat.ty = sel_ty.clone();
                }
            }
            TreeKind::Bind { name, body } => {
                self.type_pattern(body, sel_ty);
                let n = name.clone();
                let id = self.st.alloc(n.clone(), self.st.owner, SymKind::Term, Flags::PARAM, "");
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
                let fields = class_id
                    .map(|c| self.st.get(c).ctor_fields.clone())
                    .unwrap_or_default();
                let class_ty = class_id
                    .map(|c| Type::Class {
                        sym: c,
                        args: vec![],
                    })
                    .unwrap_or_else(|| sel_ty.clone());
                for (i, a) in args.iter_mut().enumerate() {
                    let ft = fields
                        .get(i)
                        .map(|f| self.st.get(*f).ty.clone())
                        .unwrap_or(Type::Any);
                    self.type_pattern(a, &ft);
                }
                pat.ty = class_ty;
                pat.sym = class_id.unwrap_or(SymbolId::NONE);
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
                    Some("Array") => Type::Array(Box::new(as_.first().cloned().unwrap_or(Type::Any))),
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
                        SymKind::Class | SymKind::ModuleClass | SymKind::Module | SymKind::TypeParam
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

fn is_sub_type(a: &Type, b: &Type) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (Type::Error, _) | (_, Type::Error) => true,
        (Type::Nothing, _) => true,
        (_, Type::Any) => true,
        (Type::Null, Type::AnyRef | Type::String | Type::Array(_) | Type::Class { .. } | Type::ModuleRef(_)) => true,
        (Type::Int | Type::Long | Type::Double | Type::Boolean | Type::Unit | Type::Char | Type::Float, Type::AnyVal) => true,
        (Type::String | Type::Array(_) | Type::Class { .. } | Type::ModuleRef(_) | Type::Function { .. }, Type::AnyRef) => true,
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
