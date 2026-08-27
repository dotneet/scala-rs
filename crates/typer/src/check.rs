#![allow(dead_code)]
//! Namer + typer. Trees are mutated in place (`ty`, `sym`).

use crate::implicits::ImplicitSearch;
use crate::javaclass::BinaryIndex;
use crate::prelude::install_prelude;
use crate::symbol::{SymKind, SymbolTable};
use crate::uncurry::{eta_expand, is_eta_marker};
use scala_rs_parser::ast::*;
use scala_rs_span::{Diagnostic, Span};
use std::collections::HashSet;
use std::path::PathBuf;

pub struct TypecheckOptions {
    pub fatal_warnings: bool,
    /// Type Option/List `withFilter` as the scala-library 2.13 shape, StringOps
    /// via `augmentString`, and Iterator. The backend still needs `library_abi`.
    pub library_abi: bool,
    /// Classes loaded from `-cp` (previous compilation's classfiles).
    pub classpath: Vec<ClasspathClass>,
    /// Directories and jars/jmods searched for Java `.class` files (plus the JDK).
    pub binary_path: Vec<PathBuf>,
    /// `-language:postfixOps` / `-language:implicitConversions` / `-language:dynamics`.
    pub language_features: Vec<String>,
}

enum OverloadPick {
    Found(SymbolId, Vec<Type>, Type),
    Ambiguous,
    None,
}

enum CtorDelegation {
    This,
    Super,
    AfterStats,
    Missing,
}

/// A method recovered from a classfile (JVM descriptor).
#[derive(Clone, Debug)]
pub struct ClasspathMethod {
    pub name: String,
    pub desc: String,
}

/// A method recovered from our ScalaSignature pickle subset.
#[derive(Clone, Debug)]
pub struct ClasspathPickleMethod {
    pub name: String,
    pub param_names: Vec<String>,
    pub param_types: Vec<String>,
    pub ret: String,
    pub tparams: Vec<String>,
    pub is_val: bool,
    pub is_ctor: bool,
    pub is_implicit: bool,
}

/// Binary class/object visible to namer/typer via `-cp`.
#[derive(Clone, Debug)]
pub struct ClasspathClass {
    pub jvm_name: String,
    pub is_module: bool,
    pub methods: Vec<ClasspathMethod>,
    pub pickle: Option<Vec<ClasspathPickleMethod>>,
    /// Class type parameter names recovered from the pickle, in order.
    pub pickle_tparams: Vec<String>,
}

impl Default for TypecheckOptions {
    fn default() -> Self {
        TypecheckOptions {
            fatal_warnings: false,
            library_abi: false,
            classpath: Vec::new(),
            binary_path: Vec::new(),
            language_features: Vec::new(),
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
    /// Nearest enclosing named method; `None` in class/object constructors.
    return_meth: Option<SymbolId>,
    /// `import scala.language.dynamics` / `-language:dynamics`.
    language_dynamics: bool,
    /// `import scala.language.postfixOps` / `-language:postfixOps`.
    language_postfix_ops: bool,
    /// `import scala.language.implicitConversions` / `-language:implicitConversions`.
    language_implicit_conversions: bool,
    binary: BinaryIndex,
    completed_java: HashSet<String>,
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
    crate::classpath::install_classpath(&mut t.st, &opts.classpath);
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
            return_meth: None,
            language_dynamics: language_flag_enabled(&opts.language_features, "dynamics"),
            language_postfix_ops: language_flag_enabled(&opts.language_features, "postfixOps"),
            language_implicit_conversions: language_flag_enabled(
                &opts.language_features,
                "implicitConversions",
            ),
            binary: BinaryIndex::from_user_paths(opts.binary_path.clone()),
            completed_java: HashSet::new(),
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
                let annots = mods.annotations.clone();
                let jvm = self.jvm_for_current(name);
                let id = self
                    .st
                    .alloc(name, self.st.owner, SymKind::Class, flags, &jvm);
                self.st.get_mut(id).annotations = annots;
                self.st.enter_in_current(name, id);
                tree.sym = id;
                if mods.flags.contains(Flags::CASE) {
                    self.ensure_companion(name, id);
                }
            }
            TreeKind::ModuleDef { name, mods, .. } => {
                let annots = mods.annotations.clone();
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
                self.st.get_mut(m).annotations = annots.clone();
                self.st.get_mut(cls).annotations = annots;
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
        for tp in tparams.iter() {
            if let TreeKind::TypeDef {
                ctx_bounds, views, ..
            } = &tp.kind
            {
                if is_trait && (!ctx_bounds.is_empty() || !views.is_empty()) {
                    self.error(
                        tp.span,
                        "traits cannot have type parameters with context bounds ': ...' nor view bounds '<% ...'",
                    );
                }
            }
        }
        // constructor params as fields
        let mut fields = Vec::new();
        for clause in vparamss.iter_mut() {
            for p in clause.iter_mut() {
                if let TreeKind::ValDef { name, mods, .. } = &p.kind {
                    let mut flags = mods.flags.with(Flags::PARAM);
                    if is_case {
                        flags.set(Flags::PRIVATE, false);
                        flags.set(Flags::LOCAL, false);
                    } else if mods.flags.contains(Flags::ACCESSOR)
                        || mods.flags.contains(Flags::MUTABLE)
                    {
                        // `val` / `var` ctor params are public unless the user
                        // wrote `private` / `protected`.
                    } else {
                        // Bare ctor param: nsc `private[this]`.
                        flags = flags.with(Flags::PRIVATE).with(Flags::LOCAL);
                    }
                    let fid = self.st.alloc(name, id, SymKind::Term, flags, "");
                    self.st.get_mut(fid).private_within = mods.private_within.clone();
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
        let conversions = implicit_class_conversions(body);
        for mut conv in conversions {
            self.namer_member(&mut conv);
            body.push(conv);
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
            let flags = match &tp.kind {
                TreeKind::TypeDef { mods, .. } => mods.flags,
                _ => Flags::EMPTY,
            };
            let id = if tp.sym.is_none() {
                let id = self.st.alloc(&name, owner, SymKind::TypeParam, flags, "");
                tp.sym = id;
                id
            } else {
                tp.sym
            };
            self.st.get_mut(id).flags = self.st.get(id).flags.with(flags);
            self.st.get_mut(id).ty = Type::TypeParam(id);
            if name != "_" {
                self.st.enter_in_current(&name, id);
            }
            ids.push(id);
            if let TreeKind::TypeDef { tparams: inner, .. } = &mut tp.kind {
                if !inner.is_empty() {
                    let inner_ids = self.enter_tparams(inner, id);
                    self.st.get_mut(id).tparams = inner_ids;
                }
            }
        }
        ids
    }

    /// Apply `args` to a type constructor, diagnosing kind mismatches.
    fn apply_types(&mut self, ctor: Type, args: Vec<Type>, span: Span) -> Type {
        if args.is_empty() {
            return ctor;
        }
        if ctor.is_error() || args.iter().any(|a| a.is_error()) {
            return Type::Error;
        }
        let ctor_arity = self.st.kind_arity(&ctor);
        if ctor_arity < args.len() {
            if ctor_arity == 0 {
                self.error(
                    span,
                    format!(
                        "{} does not take type parameters",
                        self.st.display_type(&ctor)
                    ),
                );
            } else {
                self.error(
                    span,
                    format!(
                        "too many type arguments for {}: expected {}, found {}",
                        self.st.display_type(&ctor),
                        ctor_arity,
                        args.len()
                    ),
                );
            }
            return Type::Error;
        }
        let expected = self.st.tparam_arities(&ctor);
        for (i, a) in args.iter().enumerate() {
            let exp = expected.get(i).copied().unwrap_or(0);
            let got = self.st.kind_arity(a);
            if got != exp {
                let ctor_s = self.st.display_type(&ctor);
                let arg_s = self.st.display_type(a);
                if exp > 0 && got == 0 {
                    self.error(
                        span,
                        format!(
                            "kinds of the type arguments ({arg_s}) do not conform to the expected kinds of the type parameters of {ctor_s}. type constructor takes type parameters, but {arg_s} does not"
                        ),
                    );
                } else if exp == 0 && got > 0 {
                    self.error(
                        span,
                        format!(
                            "kinds of the type arguments ({arg_s}) do not conform to the expected kinds of the type parameters of {ctor_s}. {arg_s} takes type parameters"
                        ),
                    );
                } else {
                    self.error(
                        span,
                        format!(
                            "kinds of the type arguments ({arg_s}) do not conform to the expected kinds of the type parameters of {ctor_s}"
                        ),
                    );
                }
                return Type::Error;
            }
        }
        let applied = crate::symbol::apply_type_ctor(ctor, args);
        self.st.expand_applied_hk_alias(applied)
    }

    fn check_proper_type(&mut self, ty: &Type, span: Span) {
        if ty.is_error() || ty.is_no_type() {
            return;
        }
        if self.st.kind_arity(ty) > 0 {
            self.error(
                span,
                format!("{} takes type parameters", self.st.display_type(ty)),
            );
        }
    }

    /// nsc: `class C[A <% V](x: A)` → extra implicit ctor clause `(implicit evidence$n: A => V)`.
    /// nsc: `class C[T: Ordering](x: T)` → extra implicit ctor clause `(implicit evidence$n: Ordering[T])`.
    /// Higher-kinded `F[_] <% V` / `F[_]: C` is illegal in scalac 2.13 (`type F takes type parameters`).
    fn class_bound_evidence(&mut self, class_id: SymbolId, tparams: &[Tree]) -> Vec<Tree> {
        let mut evidence = Vec::new();
        for tp in tparams {
            let TreeKind::TypeDef {
                views,
                ctx_bounds,
                tparams: inner,
                name,
                ..
            } = &tp.kind
            else {
                continue;
            };
            if views.is_empty() && ctx_bounds.is_empty() {
                continue;
            }
            if !inner.is_empty() {
                self.error(tp.span, format!("type {name} takes type parameters"));
                continue;
            }
            let tp_id = tp.sym;
            if tp_id.is_none() {
                continue;
            }
            for view in views {
                if matches!(
                    view.kind,
                    TreeKind::ExistentialTypeTree { .. }
                        | TreeKind::CompoundTypeTree { .. }
                        | TreeKind::Unimplemented { .. }
                ) {
                    self.error(
                        view.span,
                        "unimplemented syntax: view bound shape (existential/refinement)",
                    );
                    continue;
                }
                let view_ty = self.tree_to_type(view);
                if view_ty.is_error() {
                    continue;
                }
                self.gensym += 1;
                let ev_name = format!("evidence${}", self.gensym);
                let ev_ty = Type::Function {
                    params: vec![Type::TypeParam(tp_id)],
                    ret: Box::new(view_ty),
                };
                let flags = Flags::IMPLICIT
                    .with(Flags::PARAM)
                    .with(Flags::PRIVATE)
                    .with(Flags::LOCAL);
                let ev_id = self.st.alloc(&ev_name, class_id, SymKind::Term, flags, "");
                self.st.get_mut(ev_id).ty = ev_ty.clone();
                self.st.enter_in_current(&ev_name, ev_id);
                let mut ev = Tree::dummy(TreeKind::ValDef {
                    mods: Modifiers::new(flags),
                    name: ev_name,
                    tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                    rhs: Box::new(Tree::dummy(TreeKind::Empty)),
                });
                ev.span = tp.span;
                ev.sym = ev_id;
                ev.ty = ev_ty;
                evidence.push(ev);
            }
            for bound in ctx_bounds {
                if matches!(
                    bound.kind,
                    TreeKind::ExistentialTypeTree { .. }
                        | TreeKind::CompoundTypeTree { .. }
                        | TreeKind::Unimplemented { .. }
                ) {
                    self.error(
                        bound.span,
                        "unimplemented syntax: context bound shape (existential/refinement)",
                    );
                    continue;
                }
                let bound_ty = self.tree_to_type(bound);
                if bound_ty.is_error() {
                    continue;
                }
                let ev_ty = apply_context_bound(bound_ty, tp_id);
                self.gensym += 1;
                let ev_name = format!("evidence${}", self.gensym);
                let flags = Flags::IMPLICIT
                    .with(Flags::PARAM)
                    .with(Flags::PRIVATE)
                    .with(Flags::LOCAL);
                let ev_id = self.st.alloc(&ev_name, class_id, SymKind::Term, flags, "");
                self.st.get_mut(ev_id).ty = ev_ty.clone();
                self.st.enter_in_current(&ev_name, ev_id);
                let mut ev = Tree::dummy(TreeKind::ValDef {
                    mods: Modifiers::new(flags),
                    name: ev_name,
                    tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                    rhs: Box::new(Tree::dummy(TreeKind::Empty)),
                });
                ev.span = tp.span;
                ev.sym = ev_id;
                ev.ty = ev_ty;
                evidence.push(ev);
            }
        }
        evidence
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
        let conversions = implicit_class_conversions(body);
        for mut conv in conversions {
            self.namer_member(&mut conv);
            body.push(conv);
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
                let annots = mods.annotations.clone();
                let id = self
                    .st
                    .alloc(name, self.st.owner, SymKind::Term, mods.flags, "");
                self.st.get_mut(id).private_within = mods.private_within.clone();
                self.st.get_mut(id).annotations = annots;
                self.st.enter_in_current(name, id);
                tree.sym = id;
            }
            TreeKind::DefDef {
                name, mods, rhs, ..
            } => {
                let annots = mods.annotations.clone();
                let mut flags = if rhs.is_empty() && !mods.flags.contains(Flags::NATIVE) {
                    mods.flags.with(Flags::ABSTRACT)
                } else {
                    mods.flags
                };
                if name == "<init>" {
                    flags = flags.with(Flags::CONSTRUCTOR);
                }
                let id = self
                    .st
                    .alloc(name, self.st.owner, SymKind::Method, flags, "");
                self.st.get_mut(id).private_within = mods.private_within.clone();
                self.st.get_mut(id).annotations = annots;
                self.st.enter_in_current(name, id);
                tree.sym = id;
            }
            TreeKind::TypeDef { .. } => {
                let (name, flags, annots, within) = match &tree.kind {
                    TreeKind::TypeDef { name, mods, .. } => (
                        name.clone(),
                        mods.flags,
                        mods.annotations.clone(),
                        mods.private_within.clone(),
                    ),
                    _ => return,
                };
                let id = self
                    .st
                    .alloc(&name, self.st.owner, SymKind::TypeMember, flags, "");
                self.st.get_mut(id).private_within = within;
                self.st.get_mut(id).annotations = annots;
                self.st.enter_in_current(&name, id);
                tree.sym = id;
                if let TreeKind::TypeDef { tparams, .. } = &mut tree.kind {
                    if !tparams.is_empty() {
                        self.st.push_scope();
                        let tp_ids = self.enter_tparams(tparams, id);
                        self.st.get_mut(id).tparams = tp_ids;
                        self.st.pop_scope();
                    }
                }
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

    fn type_anon_class(&mut self, tpt: &mut Tree) {
        if let TreeKind::ClassDef { name, impl_, .. } = &mut tpt.kind {
            if name == "$anon" {
                self.gensym += 1;
                *name = format!("$anon${}", self.gensym);
            }
            if impl_.parents.is_empty() {
                let mut p = Tree::dummy(TreeKind::Ident {
                    name: "AnyRef".into(),
                });
                p.span = tpt.span;
                impl_.parents.push(p);
            }
        }
        if tpt.sym.is_none() {
            self.namer_class(tpt);
        }
        self.type_class(tpt);
    }

    fn type_eta(&mut self, tree: &mut Tree) {
        let dummy_method = Type::Method {
            paramss: vec![],
            ret: Box::new(Type::NoType),
        };
        if let TreeKind::Typed { expr, .. } = &mut tree.kind {
            self.type_expr(expr, &dummy_method);
        }
        if let TreeKind::Typed { expr, .. } = &mut tree.kind {
            let inner = std::mem::replace(expr, Box::new(Tree::dummy(TreeKind::Empty)));
            *tree = *inner;
        }
        if let Type::Method { paramss, ret } = tree.ty.clone() {
            let params: Vec<Type> = paramss.into_iter().flatten().collect();
            eta_expand(&mut self.st, &mut self.gensym, tree, params, *ret);
        }
    }

    fn type_class(&mut self, tree: &mut Tree) {
        let id = tree.sym;
        if let TreeKind::ClassDef {
            mods,
            name,
            vparamss,
            ..
        } = &tree.kind
        {
            if mods.flags.contains(Flags::IMPLICIT) {
                let owner_kind = if !id.is_none() {
                    self.st.get(self.st.get(id).owner).kind
                } else {
                    self.st.get(self.st.owner).kind
                };
                if owner_kind == SymKind::Package {
                    // nsc: top-level `implicit class` is illegal. Package objects
                    // own the class (ModuleClass), so they do not take this path.
                    self.error(
                        tree.span,
                        "`implicit` modifier cannot be used for top-level objects",
                    );
                }
                let nparams = vparamss.first().map(|c| c.len()).unwrap_or(0);
                if nparams != 1 {
                    self.error(
                        tree.span,
                        "unimplemented: implicit class must have a single parameter",
                    );
                }
                let cname = name.clone();
                let span = tree.span;
                self.check_implicit_conversions_feature(span, &cname);
            }
        }
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        let saved_ret = self.return_meth;
        self.st.owner = id;
        self.st.this_class = id;
        self.return_meth = None;
        self.st.push_scope();
        // re-enter members into local scope
        for m in self.st.get(id).members.clone() {
            let n = self.st.get(m).name.clone();
            self.st.enter_in_current(&n, m);
        }
        let (self_name, self_tpt, is_trait) = match &tree.kind {
            TreeKind::ClassDef { impl_, mods, .. } => (
                impl_.self_name.clone(),
                impl_.self_tpt.clone(),
                mods.flags.contains(Flags::TRAIT),
            ),
            _ => (None, None, false),
        };
        let (vparamss, body, parents, tparams) = match &mut tree.kind {
            TreeKind::ClassDef {
                vparamss,
                impl_,
                tparams,
                ..
            } => (
                vparamss,
                &mut impl_.body,
                &mut impl_.parents,
                tparams.clone(),
            ),
            _ => return,
        };
        let evidence = if is_trait {
            Vec::new()
        } else {
            self.class_bound_evidence(id, &tparams)
        };
        if !evidence.is_empty() {
            vparamss.push(evidence);
        }
        // Ctor params must be typed before `extends C(z)` so the argument `z`
        // is a known term, not a NoType ident.
        let mut paramss_ty: Vec<Vec<Type>> = Vec::new();
        let mut paramss_ids: Vec<Vec<SymbolId>> = Vec::new();
        let mut all_ctor_params = Vec::new();
        for clause in vparamss.iter_mut() {
            let mut ct = Vec::new();
            let mut ids = Vec::new();
            for p in clause.iter_mut() {
                self.type_val_sig(p);
                ct.push(p.ty.clone());
                if !p.sym.is_none() {
                    self.st.get_mut(p.sym).ty = p.ty.clone();
                    ids.push(p.sym);
                    all_ctor_params.push(p.sym);
                }
            }
            paramss_ty.push(ct);
            paramss_ids.push(ids);
        }
        let ctor_param_tys = paramss_ty.first().cloned().unwrap_or_default();
        // Primary `<init>` type must be visible before auxiliary `this(...)` bodies.
        if !id.is_none() {
            for mem in self.st.get(id).members.clone() {
                if self.st.get(mem).name == "<init>"
                    && self.st.get(mem).params == self.st.get(id).ctor_fields.clone()
                {
                    self.st.get_mut(mem).params = all_ctor_params.clone();
                    self.st.get_mut(mem).paramss = paramss_ids.clone();
                    self.st.get_mut(mem).ty = Type::Method {
                        paramss: paramss_ty.clone(),
                        ret: Box::new(Type::Unit),
                    };
                }
            }
        }
        let mut pts = Vec::new();
        for p in parents.iter_mut() {
            self.type_parent(p);
            pts.push(p.ty.clone());
        }
        if !pts.is_empty() {
            self.st.get_mut(id).parents = pts;
        }
        self.register_sealed_child(id);
        self.enter_inherited_members(id);
        self.bind_self_type(id, self_name, self_tpt.as_deref());
        for stt in body.iter_mut() {
            if matches!(stt.kind, TreeKind::Import { .. }) {
                self.type_import(stt);
            }
        }
        // type aliases / abstract type members before other signatures
        for stt in body.iter_mut() {
            if matches!(stt.kind, TreeKind::TypeDef { .. }) {
                self.type_type_member(stt);
            }
        }
        self.finish_type_aliases(body);
        self.st.get_mut(id).ty = Type::Class {
            sym: id,
            args: vec![],
        };
        for stt in body.iter_mut() {
            if !matches!(stt.kind, TreeKind::TypeDef { .. }) {
                self.type_member_sig(stt);
            }
        }
        for stt in body.iter_mut() {
            self.type_member_body(stt);
        }
        self.finish_case_apply(id, &ctor_param_tys);
        self.check_type_member_kind_override(id, tree.span);
        self.check_self_conformance(id, tree.span);
        self.check_class_variance(id, tree.span);
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        self.return_meth = saved_ret;
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
            let class_ty = Type::Class {
                sym: class_id,
                args: vec![],
            };
            let unapply_ret = match ctor_param_tys.len() {
                0 => Type::Boolean,
                1 => Type::Class {
                    sym: self.st.option_sym,
                    args: vec![ctor_param_tys[0].clone()],
                },
                _ => Type::Class {
                    sym: self.st.option_sym,
                    args: vec![Type::Tuple(ctor_param_tys.to_vec())],
                },
            };
            for mem in self.st.get(cls).members.clone() {
                let n = self.st.get(mem).name.clone();
                if n == "apply" {
                    self.st.get_mut(mem).ty = Type::Method {
                        paramss: vec![ctor_param_tys.to_vec()],
                        ret: Box::new(class_ty.clone()),
                    };
                } else if n == "unapply" {
                    self.st.get_mut(mem).ty = Type::Method {
                        paramss: vec![vec![class_ty.clone()]],
                        ret: Box::new(unapply_ret.clone()),
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
        // Primary constructor only. Auxiliary `def this` already has a Method
        // type from `type_def_sig`; overwriting it with the primary arity would
        // make `new C(1)` and `extends C(1)` miss the aux overload.
        for mem in self.st.get(class_id).members.clone() {
            if self.st.get(mem).name == "<init>" && self.st.get(mem).ty.is_no_type() {
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
        let saved_ret = self.return_meth;
        self.st.owner = cls;
        self.st.this_class = cls;
        self.return_meth = None;
        self.st.push_scope();
        for mem in self.st.get(cls).members.clone() {
            let n = self.st.get(mem).name.clone();
            self.st.enter_in_current(&n, mem);
        }
        let (self_name, self_tpt) = match &tree.kind {
            TreeKind::ModuleDef { impl_, .. } => (impl_.self_name.clone(), impl_.self_tpt.clone()),
            _ => (None, None),
        };
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
        self.enter_inherited_members(cls);
        self.bind_self_type(cls, self_name, self_tpt.as_deref());
        for stt in body.iter_mut() {
            if matches!(stt.kind, TreeKind::Import { .. }) {
                self.type_import(stt);
            }
        }
        for stt in body.iter_mut() {
            if matches!(stt.kind, TreeKind::TypeDef { .. }) {
                self.type_type_member(stt);
            }
        }
        self.finish_type_aliases(body);
        for stt in body.iter_mut() {
            if !matches!(stt.kind, TreeKind::TypeDef { .. }) {
                self.type_member_sig(stt);
            }
        }
        for stt in body.iter_mut() {
            self.type_member_body(stt);
        }
        self.check_self_conformance(cls, tree.span);
        self.check_type_member_kind_override(cls, tree.span);
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        self.return_meth = saved_ret;
        tree.ty = Type::ModuleRef(cls);
    }

    fn type_type_member(&mut self, tree: &mut Tree) {
        let span = tree.span;
        let type_member_id = tree.sym;
        let (has_tparams, has_bounds, name) = match &tree.kind {
            TreeKind::TypeDef {
                tparams,
                lo,
                hi,
                name,
                ..
            } => (
                !tparams.is_empty(),
                lo.is_some() || hi.is_some(),
                name.clone(),
            ),
            _ => return,
        };
        if has_bounds && !has_tparams {
            let ty = if let TreeKind::TypeDef { rhs, lo, hi, .. } = &mut tree.kind {
                let lo_ty = lo.as_ref().map(|t| self.tree_to_type(t));
                let hi_ty = hi.as_ref().map(|t| self.tree_to_type(t));
                if let Some(t) = &lo_ty {
                    self.check_proper_type(t, span);
                }
                if let Some(t) = &hi_ty {
                    self.check_proper_type(t, span);
                }
                if !type_member_id.is_none() {
                    self.st.get_mut(type_member_id).bound_lo = lo_ty.clone();
                    self.st.get_mut(type_member_id).bound_hi = hi_ty.clone();
                }
                if rhs.is_empty() {
                    Type::TypeMember(type_member_id)
                } else {
                    let rhs_ty = self.tree_to_type(rhs);
                    self.check_proper_type(&rhs_ty, span);
                    self.check_alias_against_bounds(
                        span,
                        &name,
                        &rhs_ty,
                        lo_ty.as_ref(),
                        hi_ty.as_ref(),
                    );
                    rhs_ty
                }
            } else {
                return;
            };
            tree.ty = ty.clone();
            if !type_member_id.is_none() {
                self.st.get_mut(type_member_id).ty = ty;
            }
            return;
        }
        if has_tparams {
            let ty = if let TreeKind::TypeDef {
                tparams,
                rhs,
                lo,
                hi,
                ..
            } = &mut tree.kind
            {
                self.st.push_scope();
                let tp_ids = self.enter_tparams(tparams, type_member_id);
                if !type_member_id.is_none() {
                    self.st.get_mut(type_member_id).tparams = tp_ids;
                }
                let lo_ty = lo.as_ref().map(|t| self.tree_to_type(t));
                let hi_ty = hi.as_ref().map(|t| self.tree_to_type(t));
                if let Some(t) = &lo_ty {
                    self.check_proper_type(t, span);
                }
                if let Some(t) = &hi_ty {
                    self.check_proper_type(t, span);
                }
                if !type_member_id.is_none() {
                    self.st.get_mut(type_member_id).bound_lo = lo_ty;
                    self.st.get_mut(type_member_id).bound_hi = hi_ty.clone();
                }
                let ty = if rhs.is_empty() {
                    Type::TypeMember(type_member_id)
                } else {
                    let rhs_ty = self.tree_to_type(rhs);
                    self.check_proper_type(&rhs_ty, span);
                    if let Some(h) = &hi_ty {
                        if !rhs_ty.is_error() && !self.st.is_sub_type(&rhs_ty, h) {
                            self.error(
                                span,
                                format!(
                                    "incompatible type in overriding: {} does not conform to {}",
                                    self.st.display_type(&rhs_ty),
                                    self.st.display_type(h)
                                ),
                            );
                        }
                    }
                    rhs_ty
                };
                self.st.pop_scope();
                ty
            } else {
                return;
            };
            tree.ty = ty.clone();
            if !type_member_id.is_none() {
                self.st.get_mut(type_member_id).ty = ty;
            }
            let _ = name;
            return;
        }
        let rhs_empty = match &tree.kind {
            TreeKind::TypeDef { rhs, .. } => rhs.is_empty(),
            _ => return,
        };
        let ty = if rhs_empty {
            Type::TypeMember(type_member_id)
        } else if let TreeKind::TypeDef { rhs, .. } = &tree.kind {
            self.tree_to_type(rhs)
        } else {
            return;
        };
        tree.ty = ty.clone();
        if !type_member_id.is_none() {
            self.st.get_mut(type_member_id).ty = ty;
        }
        let _ = name;
    }

    /// nsc: overriding `type F[_]` with `type F` (or the reverse) is a kind mismatch.
    fn check_type_member_kind_override(&mut self, class_id: SymbolId, span: Span) {
        if class_id.is_none() {
            return;
        }
        let members = self.st.get(class_id).members.clone();
        let parents = self.st.get(class_id).parents.clone();
        for mid in members {
            if self.st.get(mid).kind != SymKind::TypeMember || self.st.get(mid).owner != class_id {
                continue;
            }
            let name = self.st.get(mid).name.clone();
            let child_arity = self.st.get(mid).tparams.len();
            for p in &parents {
                let Some(pcls) = self.st.class_sym_of(p) else {
                    continue;
                };
                for m in self.st.lookup_member(pcls, &name) {
                    if self.st.get(m).kind != SymKind::TypeMember {
                        continue;
                    }
                    if self.st.get(m).owner == class_id {
                        continue;
                    }
                    let parent_arity = self.st.get(m).tparams.len();
                    if child_arity != parent_arity {
                        self.error(
                            span,
                            format!(
                                "illegal inheritance: type member {name} has incompatible kinds (child takes {child_arity} type parameters, parent takes {parent_arity})"
                            ),
                        );
                    } else {
                        let parent_hi = self.st.get(m).bound_hi.clone();
                        let parent_lo = self.st.get(m).bound_lo.clone();
                        let child_ty = self.st.get(mid).ty.clone();
                        let child_hi = self.st.get(mid).bound_hi.clone();
                        let child_lo = self.st.get(mid).bound_lo.clone();
                        let child_abs = matches!(&child_ty, Type::TypeMember(id) if *id == mid);
                        if let Some(phi) = parent_hi {
                            let ok = if child_abs {
                                child_hi
                                    .as_ref()
                                    .map(|h| self.st.is_sub_type(h, &phi))
                                    .unwrap_or(false)
                            } else {
                                self.st.is_sub_type(&child_ty, &phi)
                            };
                            if !ok && !child_ty.is_error() && !phi.is_error() {
                                self.error(
                                    span,
                                    format!(
                                        "incompatible type in overriding type {name}: {} does not conform to <: {}",
                                        self.st.display_type(&child_ty),
                                        self.st.display_type(&phi)
                                    ),
                                );
                            }
                        }
                        if let Some(plo) = parent_lo {
                            let ok = if child_abs {
                                child_lo
                                    .as_ref()
                                    .map(|l| self.st.is_sub_type(&plo, l))
                                    .unwrap_or(false)
                            } else {
                                self.st.is_sub_type(&plo, &child_ty)
                            };
                            if !ok && !child_ty.is_error() && !plo.is_error() {
                                self.error(
                                    span,
                                    format!(
                                        "incompatible type in overriding type {name}: {} does not conform to >: {}",
                                        self.st.display_type(&child_ty),
                                        self.st.display_type(&plo)
                                    ),
                                );
                            }
                        }
                    }
                    break;
                }
            }
        }
    }

    fn check_alias_against_bounds(
        &mut self,
        span: Span,
        name: &str,
        rhs: &Type,
        lo: Option<&Type>,
        hi: Option<&Type>,
    ) {
        if rhs.is_error() {
            return;
        }
        if let Some(h) = hi {
            if !h.is_error() && !self.st.is_sub_type(rhs, h) {
                self.error(
                    span,
                    format!(
                        "incompatible type in overriding type {name}: {} does not conform to <: {}",
                        self.st.display_type(rhs),
                        self.st.display_type(h)
                    ),
                );
            }
        }
        if let Some(l) = lo {
            if !l.is_error() && !self.st.is_sub_type(l, rhs) {
                self.error(
                    span,
                    format!(
                        "incompatible type in overriding type {name}: {} does not conform to >: {}",
                        self.st.display_type(rhs),
                        self.st.display_type(l)
                    ),
                );
            }
        }
    }

    /// Expand `type T = U` chains and diagnose `illegal cyclic reference`.
    /// Abstract `type A` (empty rhs) is left as `TypeMember`.
    fn finish_type_aliases(&mut self, body: &mut [Tree]) {
        let mut alias_ids = HashSet::new();
        for stt in body.iter() {
            if let TreeKind::TypeDef { rhs, .. } = &stt.kind {
                if !rhs.is_empty() && !stt.sym.is_none() {
                    alias_ids.insert(stt.sym.0);
                }
            }
        }
        for stt in body.iter_mut() {
            let TreeKind::TypeDef { name, rhs, .. } = &stt.kind else {
                continue;
            };
            if rhs.is_empty() || stt.sym.is_none() || stt.ty.is_error() {
                continue;
            }
            let name = name.clone();
            let span = stt.span;
            let mut seen = Vec::new();
            match expand_alias_type(&self.st, &stt.ty, &alias_ids, &mut seen) {
                Ok(t) => {
                    stt.ty = t.clone();
                    self.st.get_mut(stt.sym).ty = t;
                }
                Err(_) => {
                    self.error(
                        span,
                        format!("illegal cyclic reference involving type {name}"),
                    );
                    stt.ty = Type::Error;
                    self.st.get_mut(stt.sym).ty = Type::Error;
                }
            }
        }
    }

    fn bind_self_type(
        &mut self,
        class_id: SymbolId,
        self_name: Option<String>,
        self_tpt: Option<&Tree>,
    ) {
        let Some(tpt) = self_tpt else {
            return;
        };
        let st = self.tree_to_type(tpt);
        if st.is_error() {
            return;
        }
        self.st.get_mut(class_id).self_type = Some(st.clone());
        if let Some(cls) = self.st.class_sym_of(&st) {
            for m in self.st.get(cls).members.clone() {
                let n = self.st.get(m).name.clone();
                if n.ends_with('$') || n == "<init>" {
                    continue;
                }
                self.st.enter_in_current(&n, m);
            }
            // members of Foo's parents too (lookup_member walks them; Ident needs scope)
            let mut work = self.st.get(cls).parents.clone();
            let mut seen = std::collections::HashSet::new();
            seen.insert(cls.0);
            while let Some(p) = work.pop() {
                let Some(pid) = self.st.class_sym_of(&p) else {
                    continue;
                };
                if !seen.insert(pid.0) {
                    continue;
                }
                for m in self.st.get(pid).members.clone() {
                    let n = self.st.get(m).name.clone();
                    if n.ends_with('$') || n == "<init>" {
                        continue;
                    }
                    self.st.enter_in_current(&n, m);
                }
                work.extend(self.st.get(pid).parents.clone());
            }
        }
        if let Some(name) = self_name {
            if name != "this" {
                let sid = self
                    .st
                    .alloc(&name, class_id, SymKind::Term, Flags::SYNTHETIC, "");
                self.st.get_mut(sid).ty = st;
                self.st.enter_in_current(&name, sid);
            }
        }
    }

    /// Put inherited members in the template scope so `val Red = Value` inside
    /// `object Color extends Enumeration` resolves `Value` like nsc.
    fn enter_inherited_members(&mut self, cls: SymbolId) {
        let mut work = self.st.get(cls).parents.clone();
        let mut seen = std::collections::HashSet::new();
        seen.insert(cls.0);
        while let Some(p) = work.pop() {
            let Some(pid) = self.st.class_sym_of(&p) else {
                continue;
            };
            if !seen.insert(pid.0) {
                continue;
            }
            for m in self.st.get(pid).members.clone() {
                let n = self.st.get(m).name.clone();
                if n.ends_with('$') || n == "<init>" {
                    continue;
                }
                self.st.enter_in_current(&n, m);
            }
            work.extend(self.st.get(pid).parents.clone());
        }
    }

    fn check_self_conformance(&mut self, class_id: SymbolId, span: Span) {
        if class_id.is_none() {
            return;
        }
        let is_trait = self.st.get(class_id).flags.contains(Flags::TRAIT);
        if is_trait {
            return;
        }
        let this_ty = Type::Class {
            sym: class_id,
            args: vec![],
        };
        let mut work = vec![class_id];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            if let Some(st) = self.st.get(id).self_type.clone() {
                if !self.st.is_sub_type(&this_ty, &st) {
                    self.error(
                        span,
                        format!(
                            "illegal inheritance: self-type {} does not conform to {}",
                            self.st.display_type(&this_ty),
                            self.st.display_type(&st)
                        ),
                    );
                }
            }
            for p in self.st.get(id).parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    work.push(ps);
                }
            }
        }
    }

    fn check_class_variance(&mut self, class_id: SymbolId, span: Span) {
        let tps = self.st.get(class_id).tparams.clone();
        if tps.is_empty() {
            return;
        }
        let mut vars: Vec<(SymbolId, i8, String)> = Vec::new();
        for tp in &tps {
            let f = self.st.get(*tp).flags;
            let v = if f.contains(Flags::COVARIANT) {
                1
            } else if f.contains(Flags::CONTRAVARIANT) {
                -1
            } else {
                0
            };
            vars.push((*tp, v, self.st.get(*tp).name.clone()));
        }
        if vars.iter().all(|(_, v, _)| *v == 0) {
            return;
        }
        let class_is_case = self.st.get(class_id).flags.contains(Flags::CASE);
        for f in self.st.get(class_id).ctor_fields.clone() {
            let flags = self.st.get(f).flags;
            let ty = self.st.get(f).ty.clone();
            let name = self.st.get(f).name.clone();
            if flags.contains(Flags::MUTABLE) {
                self.check_variance_ty(&vars, &ty, -1, span, &format!("value {name}"));
                self.check_variance_ty(&vars, &ty, 1, span, &format!("value {name}"));
            } else if flags.contains(Flags::ACCESSOR) || class_is_case {
                self.check_variance_ty(&vars, &ty, 1, span, &format!("value {name}"));
            }
        }
        for m in self.st.get(class_id).members.clone() {
            if self.st.get(m).kind != SymKind::Method {
                continue;
            }
            let name = self.st.get(m).name.clone();
            if name == "<init>" || name == "<clinit>" {
                continue;
            }
            if let Type::Method { paramss, ret } = self.st.get(m).ty.clone() {
                for (i, p) in paramss.iter().flatten().enumerate() {
                    self.check_variance_ty(&vars, p, -1, span, &format!("parameter {i} of {name}"));
                }
                self.check_variance_ty(&vars, &ret, 1, span, &format!("return type of {name}"));
            }
        }
    }

    fn check_variance_ty(
        &mut self,
        vars: &[(SymbolId, i8, String)],
        ty: &Type,
        pos: i8,
        span: Span,
        where_: &str,
    ) {
        match ty {
            Type::TypeParam(id) => {
                if let Some((_, vp, name)) = vars.iter().find(|(t, _, _)| t == id) {
                    if *vp != 0 && pos != 0 && (*vp < 0 && pos > 0 || *vp > 0 && pos < 0) {
                        let which = if *vp > 0 {
                            "covariant"
                        } else {
                            "contravariant"
                        };
                        let place = if pos > 0 {
                            "covariant"
                        } else {
                            "contravariant"
                        };
                        self.error(
                            span,
                            format!(
                                "{which} type {name} occurs in {place} position in type {} of {where_}",
                                self.st.display_type(ty)
                            ),
                        );
                    }
                    if *vp != 0 && pos == 0 {
                        let which = if *vp > 0 {
                            "covariant"
                        } else {
                            "contravariant"
                        };
                        self.error(
                            span,
                            format!(
                                "{which} type {name} occurs in invariant position in type {} of {where_}",
                                self.st.display_type(ty)
                            ),
                        );
                    }
                }
            }
            Type::Class { sym, args } => {
                let tps = self.st.get(*sym).tparams.clone();
                for (i, a) in args.iter().enumerate() {
                    let vp = tps
                        .get(i)
                        .map(|tp| {
                            let f = self.st.get(*tp).flags;
                            if f.contains(Flags::COVARIANT) {
                                1
                            } else if f.contains(Flags::CONTRAVARIANT) {
                                -1
                            } else {
                                0
                            }
                        })
                        .unwrap_or(0);
                    self.check_variance_ty(vars, a, pos * vp, span, where_);
                }
            }
            Type::Applied { ctor, args } => {
                self.check_variance_ty(vars, ctor, pos, span, where_);
                for a in args {
                    self.check_variance_ty(vars, a, 0, span, where_);
                }
            }
            Type::Function { params, ret } => {
                for p in params {
                    self.check_variance_ty(vars, p, -pos, span, where_);
                }
                self.check_variance_ty(vars, ret, pos, span, where_);
            }
            Type::Method { paramss, ret } => {
                for p in paramss.iter().flatten() {
                    self.check_variance_ty(vars, p, -pos, span, where_);
                }
                self.check_variance_ty(vars, ret, pos, span, where_);
            }
            Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => {
                self.check_variance_ty(vars, t, pos, span, where_);
            }
            Type::Tuple(ts) => {
                for t in ts {
                    self.check_variance_ty(vars, t, pos, span, where_);
                }
            }
            Type::Named { args, .. } => {
                for a in args {
                    self.check_variance_ty(vars, a, 0, span, where_);
                }
            }
            Type::Refined { parents, decls } => {
                for p in parents {
                    self.check_variance_ty(vars, p, pos, span, where_);
                }
                for d in decls {
                    match d {
                        scala_rs_parser::RefineDecl::Type { rhs: Some(t), .. }
                        | scala_rs_parser::RefineDecl::Val { ty: t, .. } => {
                            self.check_variance_ty(vars, t, pos, span, where_);
                        }
                        scala_rs_parser::RefineDecl::Def { paramss, ret, .. } => {
                            for p in paramss.iter().flatten() {
                                self.check_variance_ty(vars, p, -pos, span, where_);
                            }
                            self.check_variance_ty(vars, ret, pos, span, where_);
                        }
                        _ => {}
                    }
                }
            }
            Type::Annotated { tpe, annot } => {
                // nsc skips only `@uncheckedVariance`, not `@unchecked`.
                if annot.rsplit('.').next().unwrap_or(annot.as_str()) != "uncheckedVariance" {
                    self.check_variance_ty(vars, tpe, pos, span, where_);
                }
            }
            _ => {}
        }
    }

    fn type_member_sig(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ValDef { .. } => self.type_val_sig(tree),
            TreeKind::DefDef { .. } => self.type_def_sig(tree),
            TreeKind::ClassDef { .. } => self.type_class(tree),
            TreeKind::ModuleDef { .. } => self.type_module(tree),
            TreeKind::TypeDef { .. } => self.type_type_member(tree),
            _ => {}
        }
    }

    fn type_member_body(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ValDef { .. } => self.type_val_body(tree),
            TreeKind::DefDef { .. } => self.type_def_body(tree),
            TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } | TreeKind::TypeDef { .. } => {
                self.check_stored_annotations(tree);
            }
            TreeKind::Import { .. } => self.type_import(tree),
            _ => {
                self.type_stat(tree);
            }
        }
    }

    fn type_val_sig(&mut self, tree: &mut Tree) {
        let (tpt, name, flags, within) = match &tree.kind {
            TreeKind::ValDef {
                tpt, name, mods, ..
            } => (
                tpt.clone(),
                name.clone(),
                mods.flags,
                mods.private_within.clone(),
            ),
            _ => return,
        };
        let ty = if tpt.is_empty() {
            if !tree.ty.is_no_type() {
                tree.ty.clone()
            } else {
                Type::NoType
            }
        } else {
            let ty = self.tree_to_type(&tpt);
            self.check_proper_type(&ty, tree.span);
            ty
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
            self.st.get_mut(tree.sym).private_within = within;
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
            if flags.contains(Flags::LOCAL) {
                let f = self.st.get(tree.sym).flags.with(Flags::LOCAL);
                self.st.get_mut(tree.sym).flags = f;
            }
            if flags.contains(Flags::PRIVATE) {
                let f = self.st.get(tree.sym).flags.with(Flags::PRIVATE);
                self.st.get_mut(tree.sym).flags = f;
            }
            if flags.contains(Flags::PROTECTED) {
                let f = self.st.get(tree.sym).flags.with(Flags::PROTECTED);
                self.st.get_mut(tree.sym).flags = f;
            }
        }
        let _ = name;
    }

    fn type_val_body(&mut self, tree: &mut Tree) {
        let presuper = matches!(
            &tree.kind,
            TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::PRESUPER)
        );
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
        if presuper && tree_contains_this(rhs) {
            self.error(
                tree.span,
                "this can be used only in a class, object, or template",
            );
        }
        if declared.is_no_type() {
            tree.ty = rhs.ty.widen_constant();
            if !tree.sym.is_none() {
                self.st.get_mut(tree.sym).ty = tree.ty.clone();
            }
        } else {
            self.adapt(rhs, &declared);
            tree.ty = declared;
        }
        self.check_stored_annotations(tree);
    }

    fn type_def_sig(&mut self, tree: &mut Tree) {
        let span = tree.span;
        let (tparams, vparamss, tpt, name, mods_within, mods_flags, is_conv) = match &mut tree.kind
        {
            TreeKind::DefDef {
                tparams,
                vparamss,
                tpt,
                name,
                mods,
                ..
            } => {
                let is_conv = mods.flags.contains(Flags::IMPLICIT)
                    && !mods.flags.contains(Flags::SYNTHETIC)
                    && is_implicit_conversion_shape(vparamss);
                (
                    tparams,
                    vparamss,
                    tpt.clone(),
                    name.clone(),
                    mods.private_within.clone(),
                    mods.flags,
                    is_conv,
                )
            }
            _ => return,
        };
        if is_conv {
            self.check_implicit_conversions_feature(span, &name);
        }
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
        self.st.get_mut(tree.sym).tparams = tp_ids.clone();
        let mut view_work: Vec<(SymbolId, Vec<Tree>, Span, bool)> = Vec::new();
        let mut ctx_work: Vec<(SymbolId, Vec<Tree>, Span, bool)> = Vec::new();
        for (i, tp) in tparams.iter().enumerate() {
            if let TreeKind::TypeDef {
                views,
                ctx_bounds,
                tparams: inner,
                ..
            } = &tp.kind
            {
                let hk = !inner.is_empty();
                if !views.is_empty() {
                    view_work.push((tp_ids[i], views.clone(), tp.span, hk));
                }
                if !ctx_bounds.is_empty() {
                    ctx_work.push((tp_ids[i], ctx_bounds.clone(), tp.span, hk));
                }
            }
        }
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
        // nsc: `T <% V` becomes an extra implicit clause `(implicit evidence$n: T => V)`.
        let mut evidence = Vec::new();
        for (tp_id, views, span, hk) in view_work {
            if hk {
                let tp_name = self.st.get(tp_id).name.clone();
                self.error(span, format!("type {tp_name} takes type parameters"));
                continue;
            }
            for view in views {
                if matches!(
                    view.kind,
                    TreeKind::ExistentialTypeTree { .. }
                        | TreeKind::CompoundTypeTree { .. }
                        | TreeKind::Unimplemented { .. }
                ) {
                    self.error(
                        view.span,
                        "unimplemented syntax: view bound shape (existential/refinement)",
                    );
                    continue;
                }
                let view_ty = self.tree_to_type(&view);
                if view_ty.is_error() {
                    continue;
                }
                self.gensym += 1;
                let ev_name = format!("evidence${}", self.gensym);
                let ev_ty = Type::Function {
                    params: vec![Type::TypeParam(tp_id)],
                    ret: Box::new(view_ty),
                };
                let ev_id = self.st.alloc(
                    &ev_name,
                    tree.sym,
                    crate::symbol::SymKind::Term,
                    Flags::IMPLICIT.with(Flags::PARAM),
                    "",
                );
                self.st.get_mut(ev_id).ty = ev_ty.clone();
                self.st.enter_in_current(&ev_name, ev_id);
                let mut ev = Tree::dummy(TreeKind::ValDef {
                    mods: Modifiers::new(Flags::IMPLICIT.with(Flags::PARAM)),
                    name: ev_name,
                    tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                    rhs: Box::new(Tree::dummy(TreeKind::Empty)),
                });
                ev.span = span;
                ev.sym = ev_id;
                ev.ty = ev_ty.clone();
                evidence.push(ev);
                all_params.push(ev_id);
            }
        }
        for (tp_id, bounds, span, hk) in ctx_work {
            if hk {
                let tp_name = self.st.get(tp_id).name.clone();
                self.error(span, format!("type {tp_name} takes type parameters"));
                continue;
            }
            for bound in bounds {
                if matches!(
                    bound.kind,
                    TreeKind::ExistentialTypeTree { .. }
                        | TreeKind::CompoundTypeTree { .. }
                        | TreeKind::Unimplemented { .. }
                ) {
                    self.error(
                        bound.span,
                        "unimplemented syntax: context bound shape (existential/refinement)",
                    );
                    continue;
                }
                let bound_ty = self.tree_to_type(&bound);
                if bound_ty.is_error() {
                    continue;
                }
                let ev_ty = apply_context_bound(bound_ty, tp_id);
                self.gensym += 1;
                let ev_name = format!("evidence${}", self.gensym);
                let ev_id = self.st.alloc(
                    &ev_name,
                    tree.sym,
                    crate::symbol::SymKind::Term,
                    Flags::IMPLICIT.with(Flags::PARAM),
                    "",
                );
                self.st.get_mut(ev_id).ty = ev_ty.clone();
                self.st.enter_in_current(&ev_name, ev_id);
                let mut ev = Tree::dummy(TreeKind::ValDef {
                    mods: Modifiers::new(Flags::IMPLICIT.with(Flags::PARAM)),
                    name: ev_name,
                    tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                    rhs: Box::new(Tree::dummy(TreeKind::Empty)),
                });
                ev.span = span;
                ev.sym = ev_id;
                ev.ty = ev_ty.clone();
                evidence.push(ev);
                all_params.push(ev_id);
            }
        }
        if !evidence.is_empty() {
            let tys: Vec<Type> = evidence.iter().map(|e| e.ty.clone()).collect();
            let ids: Vec<SymbolId> = evidence.iter().map(|e| e.sym).collect();
            paramss_ty.push(tys);
            paramss_ids.push(ids);
            vparamss.push(evidence);
        }
        self.synthesize_default_getters(saved_owner, tree.sym, &name, &tp_ids, &paramss_ids);
        self.st.owner = saved_owner;
        let ret = if name == "<init>" {
            Type::Unit
        } else if tpt.is_empty() {
            Type::NoType
        } else {
            let ret = self.tree_to_type(&tpt);
            self.check_proper_type(&ret, span);
            ret
        };
        if name == "<init>" && !tree.sym.is_none() {
            let f = self.st.get(tree.sym).flags.with(Flags::CONSTRUCTOR);
            self.st.get_mut(tree.sym).flags = f;
        }
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
            self.st.get_mut(tree.sym).private_within = mods_within;
            if mods_flags.contains(Flags::LOCAL) {
                let f = self.st.get(tree.sym).flags.with(Flags::LOCAL);
                self.st.get_mut(tree.sym).flags = f;
            }
            if mods_flags.contains(Flags::PRIVATE) {
                let f = self.st.get(tree.sym).flags.with(Flags::PRIVATE);
                self.st.get_mut(tree.sym).flags = f;
            }
            if mods_flags.contains(Flags::PROTECTED) {
                let f = self.st.get(tree.sym).flags.with(Flags::PROTECTED);
                self.st.get_mut(tree.sym).flags = f;
            }
        }
        let _ = name;
    }

    fn synthesize_default_getters(
        &mut self,
        owner: SymbolId,
        _meth: SymbolId,
        name: &str,
        tp_ids: &[SymbolId],
        paramss_ids: &[Vec<SymbolId>],
    ) {
        if owner.is_none() || name.contains("$default$") {
            return;
        }
        let flat: Vec<SymbolId> = paramss_ids.iter().flatten().copied().collect();
        for (i, pid) in flat.iter().enumerate() {
            if !self.st.get(*pid).flags.contains(Flags::DEFAULTPARAM) {
                continue;
            }
            let n = i + 1;
            let gname = format!("{name}$default${n}");
            if self
                .st
                .lookup_member(owner, &gname)
                .iter()
                .any(|&id| self.st.get(id).name == gname)
            {
                continue;
            }
            let preceding: Vec<SymbolId> = flat[..i].to_vec();
            let preceding_tys: Vec<Type> = preceding
                .iter()
                .map(|id| self.st.get(*id).ty.clone())
                .collect();
            let ret = self.st.get(*pid).ty.clone();
            let gid = self.st.alloc(
                &gname,
                owner,
                crate::symbol::SymKind::Method,
                Flags::SYNTHETIC,
                "",
            );
            self.st.get_mut(gid).ty = Type::Method {
                paramss: vec![preceding_tys],
                ret: Box::new(ret.clone()),
            };
            self.st.get_mut(gid).params = preceding.clone();
            self.st.get_mut(gid).paramss = vec![preceding.clone()];
            self.st.get_mut(gid).tparams = tp_ids.to_vec();
            if let Some(mut rhs) = self.st.get(*pid).default_rhs.clone() {
                self.st.push_scope();
                for tp in tp_ids {
                    let n = self.st.get(*tp).name.clone();
                    self.st.enter_in_current(&n, *tp);
                }
                for p in &preceding {
                    let n = self.st.get(*p).name.clone();
                    self.st.enter_in_current(&n, *p);
                }
                self.type_expr(&mut rhs, &ret);
                if !ret.is_no_type() {
                    self.adapt(&mut rhs, &ret);
                }
                self.st.pop_scope();
                self.st.get_mut(*pid).default_rhs = Some(rhs.clone());
                self.st.get_mut(gid).default_rhs = Some(rhs);
            }
        }
    }

    fn type_def_body(&mut self, tree: &mut Tree) {
        let is_ctor = match &tree.kind {
            TreeKind::DefDef { name, .. } => name == "<init>",
            _ => return,
        };
        {
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
                self.check_stored_annotations(tree);
                return;
            }
            self.st.push_scope();
            let saved_owner = self.st.owner;
            let saved_ret = self.return_meth;
            if !tree.sym.is_none() {
                self.return_meth = Some(tree.sym);
                self.st.owner = tree.sym;
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
            } else if !is_ctor {
                // infer result type (SIP-23: do not infer singleton/constant types)
                let inferred = rhs.ty.widen_constant();
                if let Type::Method { ret, .. } = &mut tree.ty {
                    *ret = Box::new(inferred.clone());
                }
                if !tree.sym.is_none() {
                    self.st.get_mut(tree.sym).ty = tree.ty.clone();
                }
            }
            self.st.owner = saved_owner;
            self.return_meth = saved_ret;
            self.st.pop_scope();
        }
        if is_ctor {
            self.check_aux_ctor(tree);
        }
        self.check_stored_annotations(tree);
    }

    fn in_aux_ctor(&self) -> bool {
        self.return_meth
            .map(|id| self.st.get(id).name == "<init>")
            .unwrap_or(false)
    }

    fn type_parent(&mut self, tree: &mut Tree) {
        if matches!(&tree.kind, TreeKind::Apply { .. }) {
            self.type_parent_ctor_app(tree);
            return;
        }
        tree.ty = self.tree_to_type(tree);
        self.check_proper_type(&tree.ty, tree.span);
        if let Some(id) = self.st.class_sym_of(&tree.ty) {
            tree.sym = id;
            if !self.st.get(id).flags.contains(Flags::TRAIT)
                && !self.st.get(id).flags.contains(Flags::INTERFACE)
            {
                match self.pick_ctor(id, &[], None) {
                    OverloadPick::Found(sym, _, _) => {
                        tree.sym = id;
                        let _ = sym;
                    }
                    OverloadPick::None => {
                        let has_init = self
                            .st
                            .lookup_member(id, "<init>")
                            .iter()
                            .any(|&m| !self.st.get(m).params.is_empty());
                        if has_init {
                            self.error(
                                tree.span,
                                format!(
                                    "no matching overload for constructor {}",
                                    self.st.get(id).name
                                ),
                            );
                        }
                    }
                    OverloadPick::Ambiguous => {}
                }
            }
        }
    }

    fn type_parent_ctor_app(&mut self, tree: &mut Tree) {
        let (fun, args) = match &mut tree.kind {
            TreeKind::Apply { fun, args } => (fun, args),
            _ => return,
        };
        let class_ty = self.tree_to_type(fun);
        fun.ty = class_ty.clone();
        let class_id = self.st.class_sym_of(&class_ty).unwrap_or(SymbolId::NONE);
        if !class_id.is_none() {
            fun.sym = class_id;
        }
        for a in args.iter_mut() {
            self.type_expr(a, &Type::NoType);
        }
        let arg_tys: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
        tree.ty = class_ty.clone();
        if class_id.is_none() {
            return;
        }
        match self.pick_ctor(class_id, &arg_tys, None) {
            OverloadPick::Found(sym, param_tys, _) => {
                for (i, a) in args.iter_mut().enumerate() {
                    if let Some(p) = param_tys.get(i) {
                        if !p.is_no_type() {
                            self.adapt(a, p);
                        }
                    }
                }
                tree.sym = sym;
            }
            OverloadPick::Ambiguous => {
                self.error(tree.span, "ambiguous overload for constructor");
            }
            OverloadPick::None => {
                self.error(
                    tree.span,
                    format!(
                        "no matching overload for constructor {} with arguments ({})",
                        self.st.get(class_id).name,
                        arg_tys
                            .iter()
                            .map(|t| self.st.display_type(t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
            }
        }
    }

    fn pick_ctor(
        &self,
        class_id: SymbolId,
        arg_tys: &[Type],
        skip: Option<SymbolId>,
    ) -> OverloadPick {
        if class_id.is_none() {
            return OverloadPick::None;
        }
        let alts: Vec<SymbolId> = self
            .st
            .lookup_member(class_id, "<init>")
            .into_iter()
            .filter(|&id| Some(id) != skip)
            .filter(|&id| self.st.get(id).kind == crate::symbol::SymKind::Method)
            .collect();
        if alts.is_empty() {
            return OverloadPick::None;
        }
        let fun_sym = alts[0];
        let fun_ty = if alts.len() == 1 {
            let ty = self.st.get(fun_sym).ty.clone();
            if ty.is_no_type() {
                Type::Method {
                    paramss: vec![self
                        .st
                        .get(fun_sym)
                        .params
                        .iter()
                        .map(|p| self.st.get(*p).ty.clone())
                        .collect()],
                    ret: Box::new(Type::Unit),
                }
            } else {
                ty
            }
        } else {
            Type::Overload(
                alts.iter()
                    .map(|id| {
                        let ty = self.st.get(*id).ty.clone();
                        if ty.is_no_type() {
                            Type::Method {
                                paramss: vec![self
                                    .st
                                    .get(*id)
                                    .params
                                    .iter()
                                    .map(|p| self.st.get(*p).ty.clone())
                                    .collect()],
                                ret: Box::new(Type::Unit),
                            }
                        } else {
                            ty
                        }
                    })
                    .collect(),
            )
        };
        match self.resolve_overload(&fun_ty, fun_sym, arg_tys, &Type::NoType) {
            OverloadPick::Found(sym, _, _) if Some(sym) == skip => OverloadPick::None,
            other => other,
        }
    }

    fn type_ctor_delegation(&mut self, tree: &mut Tree) {
        let (fun, args) = match &mut tree.kind {
            TreeKind::Apply { fun, args } => (fun, args),
            _ => return,
        };
        let is_super = matches!(&fun.kind, TreeKind::Super { .. })
            || matches!(&fun.kind, TreeKind::Ident { name } if name == "super");
        self.type_expr(fun, &Type::NoType);
        if is_super {
            self.error(
                tree.span,
                "auxiliary constructor cannot call super(...); the first action must be this(...)",
            );
            for a in args.iter_mut() {
                self.type_expr(a, &Type::NoType);
            }
            tree.ty = Type::Unit;
            return;
        }
        let class_id = self.st.this_class;
        for a in args.iter_mut() {
            self.type_expr(a, &Type::NoType);
        }
        let arg_tys: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
        let skip = self.return_meth;
        match self.pick_ctor(class_id, &arg_tys, skip) {
            OverloadPick::Found(sym, param_tys, _) => {
                if let Some(cur) = skip {
                    if !self.ctor_precedes(sym, cur) {
                        self.error(
                            tree.span,
                            "called constructor's definition must precede calling constructor's definition",
                        );
                    }
                }
                for (i, a) in args.iter_mut().enumerate() {
                    if let Some(p) = param_tys.get(i) {
                        if !p.is_no_type() {
                            self.adapt(a, p);
                        }
                    }
                }
                fun.sym = sym;
                tree.sym = sym;
                tree.ty = Type::Unit;
            }
            OverloadPick::Ambiguous => {
                self.error(tree.span, "ambiguous overload for constructor");
                tree.ty = Type::Unit;
            }
            OverloadPick::None => {
                self.error(
                    tree.span,
                    format!(
                        "no matching overload for constructor {} with arguments ({})",
                        self.st.get(class_id).name,
                        arg_tys
                            .iter()
                            .map(|t| self.st.display_type(t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
                tree.ty = Type::Unit;
            }
        }
    }

    fn ctor_precedes(&self, called: SymbolId, caller: SymbolId) -> bool {
        if called == caller {
            return false;
        }
        let owner = self.st.get(caller).owner;
        let members = &self.st.get(owner).members;
        let pos = |id: SymbolId| members.iter().position(|&m| m == id);
        match (pos(called), pos(caller)) {
            (Some(a), Some(b)) => a < b,
            _ => true,
        }
    }

    fn check_aux_ctor(&mut self, tree: &Tree) {
        let rhs = match &tree.kind {
            TreeKind::DefDef { rhs, name, .. } if name == "<init>" => rhs,
            _ => return,
        };
        match first_ctor_delegation(rhs) {
            CtorDelegation::This => {}
            CtorDelegation::Super => {
                self.error(
                    rhs.span,
                    "auxiliary constructor cannot call super(...); the first action must be this(...)",
                );
            }
            CtorDelegation::AfterStats => {
                self.error(
                    rhs.span,
                    "constructor invocation must be the first statement in an auxiliary constructor",
                );
            }
            CtorDelegation::Missing => {
                self.error(
                    rhs.span,
                    "auxiliary constructor must start with a call to this(...)",
                );
            }
        }
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
        if import_enables_feature(expr, "dynamics") {
            self.language_dynamics = true;
        }
        if import_enables_feature(expr, "postfixOps") {
            self.language_postfix_ops = true;
        }
        if import_enables_feature(expr, "implicitConversions") {
            self.language_implicit_conversions = true;
        }
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
                self.complete_binary_member(owner, &n, tree.span);
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
        } else if matches!(&tree.kind, TreeKind::Typed { tpt, .. } if is_eta_marker(tpt)) {
            self.type_eta(tree);
        } else {
            self.type_expr_inner(tree, pt);
        }
        if !pt.is_no_type() && !tree.ty.is_no_type() && !tree.ty.is_error() {
            self.adapt(tree, pt);
        }
    }

    fn type_expr_inner(&mut self, tree: &mut Tree, pt: &Type) {
        if let TreeKind::InterpolatedString { prefix, .. } = &tree.kind {
            if !matches!(prefix.as_str(), "s" | "f" | "raw") {
                if self.library_abi && !self.st.lookup("StringContext").is_empty() {
                    self.desugar_custom_interpolator(tree);
                    self.type_expr_inner(tree, pt);
                    return;
                }
            }
        }
        if matches!(&tree.kind, TreeKind::Assign { .. })
            && self.try_rewrite_dynamic_update(tree, pt)
        {
            return;
        }
        match &mut tree.kind {
            TreeKind::Literal { lit } => {
                tree.ty = Type::Constant(lit.clone());
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
                let mut targs = Vec::new();
                for a in args.iter() {
                    targs.push(self.tree_to_type(a));
                }
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
                if matches!(lhs.kind, TreeKind::Apply { .. }) {
                    let lhs = std::mem::replace(lhs.as_mut(), Tree::dummy(TreeKind::Empty));
                    let rhs = std::mem::replace(rhs.as_mut(), Tree::dummy(TreeKind::Empty));
                    let (fun, mut args) = match lhs.kind {
                        TreeKind::Apply { fun, args } => (*fun, args),
                        _ => unreachable!(),
                    };
                    args.push(rhs);
                    let update = Tree {
                        id: lhs.id,
                        span: lhs.span,
                        kind: TreeKind::Select {
                            qual: Box::new(fun),
                            name: "update".into(),
                        },
                        ty: Type::NoType,
                        sym: SymbolId::NONE,
                        postfix: false,
                    };
                    tree.kind = TreeKind::Apply {
                        fun: Box::new(update),
                        args,
                    };
                    self.type_expr(tree, pt);
                    return;
                }
                self.type_expr(lhs, &Type::NoType);
                if structural_select_lhs(lhs) {
                    // nsc: `x.foo = v` on a refinement is `x.foo_=(v)` (reflective).
                    let lhs = std::mem::replace(lhs.as_mut(), Tree::dummy(TreeKind::Empty));
                    let rhs = std::mem::replace(rhs.as_mut(), Tree::dummy(TreeKind::Empty));
                    let (qual, name) = match lhs.kind {
                        TreeKind::Select { qual, name } => (*qual, name),
                        _ => unreachable!(),
                    };
                    let setter = Tree {
                        id: lhs.id,
                        span: lhs.span,
                        kind: TreeKind::Select {
                            qual: Box::new(qual),
                            name: format!("{name}_="),
                        },
                        ty: Type::NoType,
                        sym: SymbolId::NONE,
                        postfix: false,
                    };
                    tree.kind = TreeKind::Apply {
                        fun: Box::new(setter),
                        args: vec![rhs],
                    };
                    self.type_expr(tree, pt);
                    return;
                }
                self.type_expr(rhs, &lhs.ty);
                self.adapt(rhs, &lhs.ty);
                tree.ty = Type::Unit;
            }
            TreeKind::Match { .. } => self.type_match(tree, pt),
            TreeKind::New { tpt } => {
                if matches!(&tpt.kind, TreeKind::ClassDef { .. }) {
                    self.type_anon_class(tpt);
                    tree.ty = tpt.ty.clone();
                    tree.sym = tpt.sym;
                    return;
                }
                if matches!(
                    &tpt.kind,
                    TreeKind::AppliedTypeTree { .. }
                        | TreeKind::TypeApply { .. }
                        | TreeKind::AnnotatedTypeTree { .. }
                        | TreeKind::Select { .. }
                ) {
                    tpt.ty = self.tree_to_type(tpt);
                    if let Some(id) = self.st.class_sym_of(&tpt.ty) {
                        tpt.sym = id;
                    }
                } else if let TreeKind::Ident { name } = &tpt.kind {
                    let n = name.clone();
                    self.expose_unqualified(&n, tpt.span);
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
                if let Type::Overload(alts) = &tpt.ty {
                    if let Some(id) = alts.iter().find_map(|t| match t {
                        Type::Class { sym, .. } => Some(*sym),
                        _ => None,
                    }) {
                        tpt.sym = id;
                        tpt.ty = Type::Class {
                            sym: id,
                            args: vec![],
                        };
                    }
                }
                tree.ty = tpt.ty.clone();
                tree.sym = tpt.sym;
                if tree.sym.is_none() {
                    if let Some(id) = self.st.class_sym_of(&tpt.ty) {
                        tree.sym = id;
                        // Keep `Array[T]` and applied class types so `new Array[T](n)`
                        // can still see the element and rewrite through `ClassTag`.
                        match &tree.ty {
                            Type::Array(_) => {}
                            Type::Class { args, .. } if !args.is_empty() => {}
                            _ => {
                                tree.ty = Type::Class {
                                    sym: id,
                                    args: vec![],
                                };
                            }
                        }
                    }
                }
                // nsc infers `new Q` as `Q[Int]` when the expected type is `Q[Int]`.
                if let Type::Class { args, sym } = &tree.ty {
                    if args.is_empty() {
                        if let Type::Class {
                            args: pt_args,
                            sym: pt_sym,
                        } = pt
                        {
                            if *sym == *pt_sym {
                                let tps = self.st.get(*sym).tparams.clone();
                                if type_args_are_instantiated(pt_args, &tps) {
                                    tree.ty = pt.clone();
                                }
                            }
                        }
                    }
                }
            }
            TreeKind::Typed { expr, tpt } => {
                let ascr = self.tree_to_type(tpt);
                let pt_inner = peel_empty_annot(&ascr);
                self.type_expr(expr, &pt_inner);
                if !pt_inner.is_no_type() {
                    self.adapt(expr, &pt_inner);
                }
                tree.ty = fill_empty_annot(ascr, &expr.ty);
            }
            TreeKind::Return { expr } => {
                let Some(meth) = self.return_meth else {
                    self.error(tree.span, "return outside method definition");
                    tree.ty = Type::Nothing;
                    return;
                };
                let ret = match &self.st.get(meth).ty {
                    Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
                    t => t.clone(),
                };
                if ret.is_no_type() {
                    self.type_expr(expr, &Type::NoType);
                } else {
                    self.type_expr(expr, &ret);
                    if !expr.is_empty() {
                        self.adapt(expr, &ret);
                    }
                }
                tree.sym = meth;
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
                match prefix.as_str() {
                    "s" | "raw" => {}
                    "f" => match scala_rs_parser::finterp::assemble_f(parts, args.len()) {
                        Ok((_, specs)) => {
                            for (a, spec) in args.iter_mut().zip(specs.iter()) {
                                self.type_expr(a, &Type::NoType);
                                if !self.f_arg_ok(&a.ty, spec.kind()) {
                                    self.error(
                                        a.span,
                                        format!(
                                            "f interpolator: %{} requires {}, found: {}",
                                            spec.conv,
                                            f_kind_name(spec.kind()),
                                            self.st.display_type(&a.ty)
                                        ),
                                    );
                                }
                            }
                        }
                        Err(scala_rs_parser::finterp::FInterpError::Unsupported(msg))
                        | Err(scala_rs_parser::finterp::FInterpError::Message(msg)) => {
                            self.error(tree.span, msg);
                        }
                    },
                    other => {
                        self.error(
                            tree.span,
                            format!("unimplemented interpolator `{other}` (only s\"...\" / f\"...\" / raw\"...\")"),
                        );
                    }
                }
                if prefix != "f" {
                    for a in args.iter_mut() {
                        self.type_expr(a, &Type::Any);
                    }
                }
                let _ = parts;
                tree.ty = Type::String;
            }
            TreeKind::Wildcard => {
                self.error(tree.span, "unbound placeholder parameter");
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
            TreeKind::AppliedTypeTree { .. }
            | TreeKind::SingletonTypeTree { .. }
            | TreeKind::CompoundTypeTree { .. }
            | TreeKind::AnnotatedTypeTree { .. }
            | TreeKind::ExistentialTypeTree { .. } => {
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

    /// Load a same-package (or default-package) Java class for an unqualified name.
    fn expose_unqualified(&mut self, name: &str, span: Span) {
        if name.is_empty() || !self.st.lookup(name).is_empty() {
            return;
        }
        let from = if !self.st.this_class.is_none() {
            self.st.this_class
        } else {
            self.st.owner
        };
        let pkg = self.enclosing_package(from);
        self.complete_binary_member(pkg, name, span);
        for id in self.st.lookup_member(pkg, name) {
            self.st.enter_in_current(name, id);
        }
        if self.st.lookup(name).is_empty() && pkg != self.st.root {
            self.complete_binary_member(self.st.root, name, span);
            for id in self.st.lookup_member(self.st.root, name) {
                self.st.enter_in_current(name, id);
            }
        }
    }

    fn type_ident(&mut self, tree: &mut Tree, name: String, pt: &Type) {
        if name == "_" {
            self.error(tree.span, "unbound placeholder parameter");
            tree.kind = TreeKind::Wildcard;
            tree.ty = Type::Error;
            return;
        }
        self.expose_unqualified(&name, tree.span);
        let mut found = self.st.lookup(&name);
        if found.is_empty() {
            found = self.st.lookup_member(self.st.root, &name);
        }
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
            if ty.is_no_type()
                && self.st.get(s).flags.contains(Flags::PARAM)
                && is_inferable_param_pt(pt)
            {
                self.st.get_mut(s).ty = pt.clone();
                ty = pt.clone();
            }
            ty = self.maybe_auto_apply(ty, pt);
            if !self.st.this_class.is_none() {
                ty = self.st.expand_type_members(self.st.this_class, &ty);
            }
            tree.ty = ty;
            return;
        }
        // Keep overloads intact so `println(1)` can still pick a 1-arg alternative.
        // Nullary alternatives still auto-apply in value position (`"x".stripMargin`).
        let ov = Type::Overload(found.iter().map(|s| self.st.get(*s).ty.clone()).collect());
        tree.ty = self.maybe_auto_apply(ov, pt);
        tree.sym = if matches!(tree.ty, Type::Overload(_)) {
            found[0]
        } else {
            found
                .iter()
                .copied()
                .find(|&s| self.is_nullary_method_sym(s))
                .unwrap_or(found[0])
        };
    }

    fn maybe_auto_apply(&self, ty: Type, pt: &Type) -> Type {
        match &ty {
            Type::Method { paramss, ret }
                if paramss.is_empty() || paramss.iter().all(|c| c.is_empty()) =>
            {
                if matches!(pt, Type::Function { .. } | Type::Method { .. }) {
                    ty
                } else {
                    (**ret).clone()
                }
            }
            Type::Overload(alts) => {
                if matches!(pt, Type::Function { .. } | Type::Method { .. }) {
                    return ty;
                }
                let nullary: Vec<&Type> = alts
                    .iter()
                    .filter(|a| match a {
                        Type::Method { paramss, .. } => {
                            paramss.is_empty() || paramss.iter().all(|c| c.is_empty())
                        }
                        _ => false,
                    })
                    .collect();
                if let [Type::Method { ret, .. }] = nullary.as_slice() {
                    (**ret).clone()
                } else {
                    ty
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

    fn is_nullary_method_sym(&self, id: SymbolId) -> bool {
        match &self.st.get(id).ty {
            Type::Method { paramss, .. } => {
                paramss.is_empty() || paramss.iter().all(|c| c.is_empty())
            }
            _ => false,
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
            postfix: false,
        };
    }

    fn type_select(&mut self, tree: &mut Tree, pt: &Type) {
        if tree.postfix && !self.language_postfix_ops {
            let name = match &tree.kind {
                TreeKind::Select { name, .. } => name.clone(),
                _ => String::new(),
            };
            self.warning(
                tree.span,
                format!(
                    "postfix operator {name} should be enabled by making the implicit value scala.language.postfixOps visible"
                ),
            );
        }
        let (qual, name) = match &mut tree.kind {
            TreeKind::Select { qual, name } => (qual, name.clone()),
            _ => return,
        };
        if qual.ty.is_no_type() {
            self.type_expr(qual, &Type::NoType);
        }
        if name == "_" {
            self.error(
                tree.span,
                "unimplemented syntax: wildcard import/select in expression",
            );
            tree.ty = Type::Error;
            return;
        }
        let refined_term = match &qual.ty {
            Type::Refined { decls, .. } => {
                let from_term = decls.iter().any(|d| {
                    matches!(
                        d,
                        scala_rs_parser::RefineDecl::Def { name: n, .. }
                            | scala_rs_parser::RefineDecl::Val { name: n, .. }
                            if n == &name
                    )
                });
                if from_term {
                    SymbolTable::refine_member_type(decls, &name)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(mty) = refined_term {
            let mty = self.st.expand_in_type(&qual.ty, &mty);
            tree.ty = self.maybe_auto_apply(mty, pt);
            return;
        }
        // String concatenation via any2stringadd: handled at Apply of +
        let mut found = Vec::new();
        if let Type::Refined { parents, .. } = &qual.ty {
            for p in parents {
                if let Some(o) = self.st.class_sym_of(p) {
                    found.extend(self.st.lookup_member(o, &name));
                }
            }
        }
        if found.is_empty() {
            if let Some(o) = self.st.class_sym_of(&qual.ty) {
                found = self.st.lookup_member(o, &name);
                if found.is_empty() && matches!(&qual.ty, Type::Class { .. } | Type::ModuleRef(_)) {
                    // `asList(...).size()`: the receiver type is a Java stub until
                    // the classfile is completed. `qual.sym` is the method, not List.
                    // Skip `Type::String` / primitives so StringOps / RichChar views
                    // are not shadowed by `java.lang.String` / `Character` overloads.
                    self.ensure_java_loaded(o, tree.span);
                    found = self.st.lookup_member(o, &name);
                }
            }
        }
        // Module: members of module class
        if found.is_empty() {
            if let Type::ModuleRef(id) = &qual.ty {
                found = self.st.lookup_member(*id, &name);
            }
        }
        // Package / term prefix: `scala.reflect.ClassTag` and Java `java.lang.Math`.
        if found.is_empty() && !qual.sym.is_none() {
            self.complete_binary_member(qual.sym, &name, tree.span);
            found = self.st.lookup_member(qual.sym, &name);
        }
        if found.is_empty() && name == "toString" {
            found = self.st.lookup_member(self.st.any_sym, "toString");
        }
        if found.is_empty() {
            if let Some((conv, member, to)) = self.search_extension(&qual.ty, &name, tree.span) {
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
                    ty: to.clone(),
                    sym: conv,
                    postfix: false,
                };
                found = if let Some(cls) = self.st.class_sym_of(&to) {
                    self.st.lookup_member(cls, &name)
                } else {
                    vec![member]
                };
            }
        }
        if found.is_empty() && self.is_dynamic_receiver(&qual.ty) {
            if matches!(pt, Type::Method { .. }) {
                // `d.foo(args)`: type_apply rewrites to applyDynamic.
                tree.ty = Type::Error;
                return;
            }
            self.rewrite_select_dynamic(tree, pt);
            return;
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
        found = self.drop_overridden(found);
        found.retain(|s| self.accessible(*s, Some(qual.as_ref())));
        if found.is_empty() {
            self.error(
                tree.span,
                format!(
                    "value {name} cannot be accessed as a member of {} from {}",
                    self.st.display_type(&qual.ty),
                    self.access_from_name()
                ),
            );
            tree.ty = Type::Error;
            return;
        }
        // Term position prefers the companion module (and methods/vals) over
        // the class of the same name, matching `type_ident`.
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
        if !terms.is_empty() {
            found = terms;
        }
        let subst_args: Vec<Type> = match &qual.ty {
            Type::Class { args, .. } => args.clone(),
            Type::Tuple(ts) => ts.clone(),
            _ => Vec::new(),
        };
        let subst = |ty: Type| -> Type {
            let ty = self.st.subst_as_seen_from(&qual.ty, &ty);
            if !subst_args.is_empty() {
                if let Some(owner) = found.first().map(|s| self.st.get(*s).owner) {
                    return self.st.subst_tparams(owner, &subst_args, &ty);
                }
            }
            ty
        };
        let expand = |ty: Type| -> Type { self.st.expand_in_type(&qual.ty, &ty) };
        if found.len() == 1 {
            let s = found[0];
            tree.sym = s;
            let ty = expand(subst(self.st.get(s).ty.clone()));
            tree.ty = self.maybe_auto_apply(ty, pt);
            if let Type::Array(elem) = &qual.ty {
                if name == "apply" {
                    tree.ty = Type::Method {
                        paramss: vec![vec![Type::Int]],
                        ret: Box::new((**elem).clone()),
                    };
                } else if name == "update" {
                    tree.ty = Type::Method {
                        paramss: vec![vec![Type::Int, (**elem).clone()]],
                        ret: Box::new(Type::Unit),
                    };
                }
            }
        } else {
            tree.sym = found[0];
            let owner = self.st.get(found[0]).owner;
            let args = subst_args.clone();
            let ov = Type::Overload(
                found
                    .iter()
                    .map(|s| {
                        let t = self.st.get(*s).ty.clone();
                        let t = if args.is_empty() {
                            t
                        } else {
                            self.st.subst_tparams(owner, &args, &t)
                        };
                        expand(t)
                    })
                    .collect(),
            );
            tree.ty = self.maybe_auto_apply(ov, pt);
            if !matches!(tree.ty, Type::Overload(_)) {
                if let Some(id) = found
                    .iter()
                    .copied()
                    .find(|&s| self.is_nullary_method_sym(s))
                {
                    tree.sym = id;
                }
            }
        }
    }

    /// Prefer a definition on a subclass over the inherited member it overrides.
    fn drop_overridden(&self, found: Vec<SymbolId>) -> Vec<SymbolId> {
        if found.len() <= 1 {
            return found;
        }
        found
            .iter()
            .copied()
            .filter(|&s| {
                let owner = self.st.get(s).owner;
                !found.iter().any(|&other| {
                    if other == s {
                        return false;
                    }
                    let oo = self.st.get(other).owner;
                    if oo == owner {
                        return false;
                    }
                    let child = Type::Class {
                        sym: oo,
                        args: vec![],
                    };
                    let parent = Type::Class {
                        sym: owner,
                        args: vec![],
                    };
                    self.st.is_sub_type(&child, &parent)
                })
            })
            .collect()
    }

    fn access_from_name(&self) -> String {
        if self.st.this_class.is_none() {
            "<none>".into()
        } else {
            self.st.get(self.st.this_class).name.clone()
        }
    }

    /// nsc-style accessibility. `private[this]` requires a `this` prefix.
    /// `protected[C]` is protected plus everything nested in `C`.
    fn accessible(&self, sym: SymbolId, prefix: Option<&Tree>) -> bool {
        if sym.is_none() {
            return true;
        }
        let s = self.st.get(sym);
        let flags = s.flags;
        let restricted = flags.contains(Flags::PRIVATE)
            || flags.contains(Flags::PROTECTED)
            || s.private_within.is_some();
        if !restricted {
            return true;
        }
        let owner = s.owner;
        let current = self.st.this_class;
        if flags.contains(Flags::PRIVATE) && flags.contains(Flags::LOCAL) {
            return self.prefix_is_this(prefix) && self.nested_in(current, owner);
        }
        if flags.contains(Flags::PRIVATE) {
            if let Some(w) = &s.private_within {
                return self.access_within(current, w);
            }
            return self.nested_in(current, owner);
        }
        if flags.contains(Flags::PROTECTED) {
            // Java `protected` is also package-private (JLS / nsc Java interop).
            if flags.contains(Flags::JAVA) && self.java_same_package(current, owner) {
                return true;
            }
            let by_qual = s
                .private_within
                .as_ref()
                .map(|w| self.access_within(current, w))
                .unwrap_or(false);
            let by_sub = self.protected_subclass_ok(current, owner, prefix);
            return by_qual || by_sub;
        }
        if let Some(w) = &s.private_within {
            return self.access_within(current, w);
        }
        true
    }

    fn prefix_is_this(&self, prefix: Option<&Tree>) -> bool {
        match prefix {
            None => true,
            Some(t) => matches!(t.kind, TreeKind::This { .. } | TreeKind::Super { .. }),
        }
    }

    fn nested_in(&self, current: SymbolId, owner: SymbolId) -> bool {
        if owner.is_none() {
            return true;
        }
        let mut c = current;
        while !c.is_none() {
            if c == owner {
                return true;
            }
            c = self.st.get(c).owner;
        }
        false
    }

    fn access_within(&self, current: SymbolId, name: &str) -> bool {
        let boundary = self.resolve_access_boundary(name);
        if boundary.is_none() {
            return false;
        }
        self.nested_in(current, boundary)
            || self.st.get(current).name == name
            || self.st.get(current).name.trim_end_matches('$') == name
    }

    fn resolve_access_boundary(&self, name: &str) -> SymbolId {
        for id in self.st.lookup(name) {
            if matches!(
                self.st.get(id).kind,
                SymKind::Class | SymKind::ModuleClass | SymKind::Module | SymKind::Package
            ) {
                return match self.st.get(id).kind {
                    SymKind::Module => self.st.module_class_of(id),
                    _ => id,
                };
            }
        }
        let mut c = self.st.this_class;
        while !c.is_none() {
            if self.st.get(c).name == name || self.st.get(c).name.trim_end_matches('$') == name {
                return c;
            }
            c = self.st.get(c).owner;
        }
        SymbolId::NONE
    }

    fn protected_subclass_ok(
        &self,
        current: SymbolId,
        owner: SymbolId,
        prefix: Option<&Tree>,
    ) -> bool {
        if current.is_none() || owner.is_none() {
            return false;
        }
        let cur_ty = self.st.type_of_class(current);
        let own_ty = self.st.type_of_class(owner);
        if current != owner && !self.st.is_sub_type(&cur_ty, &own_ty) {
            return false;
        }
        match prefix {
            None => true,
            Some(t) if matches!(t.kind, TreeKind::This { .. } | TreeKind::Super { .. }) => true,
            Some(t) => self.st.is_sub_type(&t.ty, &cur_ty),
        }
    }

    fn java_same_package(&self, current: SymbolId, member_owner: SymbolId) -> bool {
        let a = self.enclosing_package(current);
        let b = self.enclosing_package(member_owner);
        !a.is_none() && a == b
    }

    fn enclosing_package(&self, mut id: SymbolId) -> SymbolId {
        while !id.is_none() {
            if self.st.get(id).kind == SymKind::Package {
                return id;
            }
            id = self.st.get(id).owner;
        }
        self.st.root
    }

    /// When a member exists on the receiver (e.g. `Int.+`) but the argument
    /// types do not match, try an implicit conversion that *does* have the
    /// method (`any2stringadd` for `1 + "x"`).
    fn rewrite_apply_extension(&mut self, fun: &mut Tree) -> bool {
        let TreeKind::Select { qual, name } = &mut fun.kind else {
            return false;
        };
        let Some((conv, member, to)) = self.search_extension(&qual.ty, name, fun.span) else {
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
            postfix: false,
        };
        fun.sym = member;
        fun.ty = self.st.get(member).ty.clone();
        true
    }

    fn is_dynamic_receiver(&self, ty: &Type) -> bool {
        if let Type::Named { name, .. } = ty {
            if name == "Dynamic" || name.ends_with(".Dynamic") {
                return true;
            }
        }
        let mut work = Vec::new();
        if let Some(c) = self.st.class_sym_of(ty) {
            work.push(c);
        }
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = work.pop() {
            if !seen.insert(id.0) {
                continue;
            }
            let s = self.st.get(id);
            if s.name == "Dynamic"
                || s.jvm_name == "scala/Dynamic"
                || s.jvm_name.ends_with("/Dynamic")
            {
                return true;
            }
            for p in s.parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    work.push(ps);
                } else if let Type::Named { name, .. } = &p {
                    if name == "Dynamic" || name.ends_with(".Dynamic") {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn receiver_has_term(&self, ty: &Type, name: &str) -> bool {
        match ty {
            Type::Refined { decls, parents } => {
                let in_decl = decls.iter().any(|d| {
                    matches!(
                        d,
                        scala_rs_parser::RefineDecl::Def { name: n, .. }
                            | scala_rs_parser::RefineDecl::Val { name: n, .. }
                            if n == name
                    )
                });
                if in_decl {
                    return true;
                }
                parents.iter().any(|p| self.receiver_has_term(p, name))
            }
            Type::ModuleRef(id) => !self.st.lookup_member(*id, name).is_empty(),
            _ => {
                if let Some(o) = self.st.class_sym_of(ty) {
                    if !self.st.lookup_member(o, name).is_empty() {
                        return true;
                    }
                }
                name == "toString"
            }
        }
    }

    fn dynamics_feature_error(&mut self, span: Span, method: &str) {
        self.error(
            span,
            format!(
                "Dynamic method {method} needs to be enabled by making the implicit value scala.language.dynamics visible"
            ),
        );
    }

    fn check_implicit_conversions_feature(&mut self, span: Span, name: &str) {
        if self.language_implicit_conversions {
            return;
        }
        self.warning(
            span,
            format!(
                "implicit conversion method {name} should be enabled by making the implicit value scala.language.implicitConversions visible"
            ),
        );
    }

    fn rewrite_select_dynamic(&mut self, tree: &mut Tree, pt: &Type) {
        if !self.language_dynamics {
            self.dynamics_feature_error(tree.span, "selectDynamic");
            tree.ty = Type::Error;
            return;
        }
        let span = tree.span;
        let id = tree.id;
        let (qual, dyn_name) = match &mut tree.kind {
            TreeKind::Select { qual, name } => (
                std::mem::replace(qual, Box::new(Tree::dummy(TreeKind::Empty))),
                name.clone(),
            ),
            _ => return,
        };
        let name_lit = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Literal {
                lit: Lit::String(dyn_name),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        let sel = Tree {
            id,
            span,
            kind: TreeKind::Select {
                qual,
                name: "selectDynamic".into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        tree.kind = TreeKind::Apply {
            fun: Box::new(sel),
            args: vec![name_lit],
        };
        self.type_apply(tree, pt);
    }

    /// nsc `convertToAssignment`: `x += 1` becomes `x = x.+(1)` when `+=` is
    /// not a member and the receiver is assignable. A real `def +=` wins.
    fn try_rewrite_assignment_op(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let name = match &tree.kind {
            TreeKind::Apply { fun, .. } => match &fun.kind {
                TreeKind::Select { name, .. } if is_assignment_op(name) => name.clone(),
                _ => return false,
            },
            _ => return false,
        };
        let span = tree.span;
        let id = tree.id;
        let TreeKind::Apply { fun, args } = &mut tree.kind else {
            return false;
        };
        let fun_id = fun.id;
        let fun_span = fun.span;
        let TreeKind::Select { qual, .. } = &mut fun.kind else {
            return false;
        };
        self.type_expr(qual, &Type::NoType);
        let qual_ty = qual.ty.clone();
        if self.receiver_has_term(&qual_ty, &name)
            || self.search_extension(&qual_ty, &name, span).is_some()
        {
            return false;
        }
        if self.is_assignable_lhs(qual) {
            let op = name[..name.len() - 1].to_string();
            let lhs = (**qual).clone();
            let rhs_args = args.clone();
            let plus = Tree {
                id: fun_id,
                span: fun_span,
                kind: TreeKind::Select {
                    qual: Box::new(lhs.clone()),
                    name: op,
                },
                ty: Type::NoType,
                sym: SymbolId::NONE,
                postfix: false,
            };
            let rhs = Tree {
                id,
                span,
                kind: TreeKind::Apply {
                    fun: Box::new(plus),
                    args: rhs_args,
                },
                ty: Type::NoType,
                sym: SymbolId::NONE,
                postfix: false,
            };
            tree.kind = TreeKind::Assign {
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
            self.type_expr(tree, pt);
            return true;
        }
        self.error(
            span,
            format!(
                "value {name} is not a member of {}",
                self.st.display_type(&qual_ty)
            ),
        );
        self.error(
            span,
            "Expression does not convert to assignment because receiver is not assignable.",
        );
        tree.ty = Type::Error;
        true
    }

    fn is_assignable_lhs(&self, tree: &Tree) -> bool {
        if tree.sym.is_none() {
            return false;
        }
        let s = self.st.get(tree.sym);
        s.kind == SymKind::Term && s.flags.contains(Flags::MUTABLE)
    }

    fn try_rewrite_dynamic_apply(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let dyn_name = match &tree.kind {
            TreeKind::Apply { fun, .. } => match &fun.kind {
                TreeKind::Select { name, .. }
                    if !matches!(
                        name.as_str(),
                        "applyDynamic" | "selectDynamic" | "updateDynamic" | "applyDynamicNamed"
                    ) =>
                {
                    name.clone()
                }
                _ => return false,
            },
            _ => return false,
        };
        {
            let TreeKind::Apply { fun, .. } = &mut tree.kind else {
                return false;
            };
            let TreeKind::Select { qual, .. } = &mut fun.kind else {
                return false;
            };
            self.type_expr(qual, &Type::NoType);
            if !self.is_dynamic_receiver(&qual.ty) {
                return false;
            }
            if self.receiver_has_term(&qual.ty, &dyn_name) {
                return false;
            }
        }
        if !self.language_dynamics {
            self.dynamics_feature_error(
                tree.span,
                if has_named_dynamic_args(tree) {
                    "applyDynamicNamed"
                } else {
                    "applyDynamic"
                },
            );
            tree.ty = Type::Error;
            return true;
        }
        let span = tree.span;
        let TreeKind::Apply { fun, args } = std::mem::replace(&mut tree.kind, TreeKind::Empty)
        else {
            return false;
        };
        let TreeKind::Select { qual, .. } = fun.kind else {
            tree.kind = TreeKind::Apply { fun, args };
            return false;
        };
        let named = args.iter().any(|a| Self::named_arg_parts(a).is_some());
        let method = if named {
            "applyDynamicNamed"
        } else {
            "applyDynamic"
        };
        let args = if named {
            args.into_iter()
                .map(|a| self.named_dynamic_tuple(a))
                .collect()
        } else {
            args
        };
        let name_lit = Tree::new(
            NodeId(0),
            span,
            TreeKind::Literal {
                lit: Lit::String(dyn_name),
            },
        );
        let sel = Tree {
            id: fun.id,
            span: fun.span,
            kind: TreeKind::Select {
                qual,
                name: method.into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        let inner = Tree {
            id: fun.id,
            span: fun.span,
            kind: TreeKind::Apply {
                fun: Box::new(sel),
                args: vec![name_lit],
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        tree.kind = TreeKind::Apply {
            fun: Box::new(inner),
            args,
        };
        self.type_apply(tree, pt);
        true
    }

    fn named_dynamic_tuple(&self, arg: Tree) -> Tree {
        let span = arg.span;
        let (name, value) = if let Some((n, rhs)) = Self::named_arg_parts(&arg) {
            (n, rhs)
        } else {
            (String::new(), arg)
        };
        let name_lit = Tree::new(
            NodeId(0),
            span,
            TreeKind::Literal {
                lit: Lit::String(name),
            },
        );
        let tpt = Tree::new(
            NodeId(0),
            span,
            TreeKind::Ident {
                name: "Tuple2".into(),
            },
        );
        let neu = Tree::new(NodeId(0), span, TreeKind::New { tpt: Box::new(tpt) });
        Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(neu),
                args: vec![name_lit, value],
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        }
    }

    fn try_rewrite_dynamic_update(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        enum DynUpd {
            Select(String),
            Indexed(String),
        }
        let kind = {
            let TreeKind::Assign { lhs, .. } = &mut tree.kind else {
                return false;
            };
            match &mut lhs.kind {
                TreeKind::Select { qual, name }
                    if !matches!(
                        name.as_str(),
                        "updateDynamic" | "selectDynamic" | "applyDynamic" | "applyDynamicNamed"
                    ) =>
                {
                    let dyn_name = name.clone();
                    self.type_expr(qual, &Type::NoType);
                    if !self.is_dynamic_receiver(&qual.ty)
                        || self.receiver_has_term(&qual.ty, &dyn_name)
                    {
                        return false;
                    }
                    DynUpd::Select(dyn_name)
                }
                TreeKind::Apply { fun, .. } => match &mut fun.kind {
                    TreeKind::Select { qual, name }
                        if !matches!(
                            name.as_str(),
                            "update" | "apply" | "updateDynamic" | "selectDynamic"
                        ) =>
                    {
                        let dyn_name = name.clone();
                        self.type_expr(qual, &Type::NoType);
                        if !self.is_dynamic_receiver(&qual.ty)
                            || self.receiver_has_term(&qual.ty, &dyn_name)
                        {
                            return false;
                        }
                        DynUpd::Indexed(dyn_name)
                    }
                    _ => return false,
                },
                _ => return false,
            }
        };
        if !self.language_dynamics {
            let method = match &kind {
                DynUpd::Select(_) => "updateDynamic",
                DynUpd::Indexed(_) => "selectDynamic",
            };
            self.dynamics_feature_error(tree.span, method);
            tree.ty = Type::Error;
            return true;
        }
        let span = tree.span;
        let TreeKind::Assign { lhs, rhs } = std::mem::replace(&mut tree.kind, TreeKind::Empty)
        else {
            return false;
        };
        match kind {
            DynUpd::Select(dyn_name) => {
                let TreeKind::Select { qual, .. } = lhs.kind else {
                    return false;
                };
                let name_lit = Tree::new(
                    NodeId(0),
                    span,
                    TreeKind::Literal {
                        lit: Lit::String(dyn_name),
                    },
                );
                let sel = Tree {
                    id: lhs.id,
                    span: lhs.span,
                    kind: TreeKind::Select {
                        qual,
                        name: "updateDynamic".into(),
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                };
                let inner = Tree {
                    id: lhs.id,
                    span: lhs.span,
                    kind: TreeKind::Apply {
                        fun: Box::new(sel),
                        args: vec![name_lit],
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                };
                tree.kind = TreeKind::Apply {
                    fun: Box::new(inner),
                    args: vec![*rhs],
                };
            }
            DynUpd::Indexed(dyn_name) => {
                let TreeKind::Apply { fun, mut args } = lhs.kind else {
                    return false;
                };
                let TreeKind::Select { qual, .. } = fun.kind else {
                    return false;
                };
                args.push(*rhs);
                let name_lit = Tree::new(
                    NodeId(0),
                    span,
                    TreeKind::Literal {
                        lit: Lit::String(dyn_name),
                    },
                );
                let sel = Tree {
                    id: fun.id,
                    span: fun.span,
                    kind: TreeKind::Select {
                        qual,
                        name: "selectDynamic".into(),
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                };
                let selected = Tree {
                    id: fun.id,
                    span: fun.span,
                    kind: TreeKind::Apply {
                        fun: Box::new(sel),
                        args: vec![name_lit],
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                };
                let update = Tree {
                    id: fun.id,
                    span: fun.span,
                    kind: TreeKind::Select {
                        qual: Box::new(selected),
                        name: "update".into(),
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                };
                tree.kind = TreeKind::Apply {
                    fun: Box::new(update),
                    args,
                };
            }
        }
        self.type_expr(tree, pt);
        true
    }

    fn type_apply(&mut self, tree: &mut Tree, pt: &Type) {
        if self.try_rewrite_assignment_op(tree, pt) {
            return;
        }
        if self.try_rewrite_dynamic_apply(tree, pt) {
            return;
        }
        let ctor_del = match &tree.kind {
            TreeKind::Apply { fun, .. } => self.in_aux_ctor() && is_this_or_super_callee(fun),
            _ => false,
        };
        if ctor_del {
            self.type_ctor_delegation(tree);
            return;
        }
        let (fun, args) = match &mut tree.kind {
            TreeKind::Apply { fun, args } => (fun, args),
            _ => return,
        };
        // new C(args)
        if matches!(&fun.kind, TreeKind::New { .. }) {
            self.type_expr(fun, pt);
            if let Some(elem) = array_elem_of(&fun.ty) {
                if needs_classtag_elem(&elem) {
                    self.rewrite_generic_array_new(tree, elem);
                    return;
                }
                for a in args.iter_mut() {
                    self.type_expr(a, &Type::Int);
                    self.adapt(a, &Type::Int);
                }
                tree.ty = Type::Array(Box::new(elem));
                tree.sym = fun.sym;
                return;
            }
            let class_id = fun
                .sym
                .is_none()
                .then(|| self.st.class_sym_of(&fun.ty))
                .flatten()
                .or(Some(fun.sym))
                .filter(|s| !s.is_none());
            let class_id = class_id.or_else(|| self.st.class_sym_of(&fun.ty));
            let tps = class_id
                .map(|c| self.st.get(c).tparams.clone())
                .unwrap_or_default();
            // Keep explicit `new C[T](…)` args; otherwise infer. Do not adapt
            // constructor arguments to raw type parameters (`A`) first.
            let explicit: Vec<Type> = match &fun.ty {
                Type::Class { args, .. } if type_args_are_instantiated(args, &tps) => args.clone(),
                _ => Vec::new(),
            };
            let infer = !tps.is_empty() && explicit.is_empty();
            for a in args.iter_mut() {
                self.type_expr(a, &Type::NoType);
            }
            let arg_tys: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
            let field_tys: Vec<Type> = class_id
                .map(|c| {
                    self.st
                        .get(c)
                        .ctor_fields
                        .iter()
                        .map(|f| self.st.get(*f).ty.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let (ctor_sym, ctor_params) = if let Some(c) = class_id {
                match self.pick_ctor(c, &arg_tys, None) {
                    OverloadPick::Found(sym, ps, _) => (Some(sym), ps),
                    OverloadPick::Ambiguous => {
                        self.error(tree.span, "ambiguous overload for constructor");
                        (None, Vec::new())
                    }
                    OverloadPick::None => {
                        if field_tys.len() != arg_tys.len() {
                            self.error(
                                tree.span,
                                format!(
                                    "no matching overload for constructor {} with arguments ({})",
                                    self.st.get(c).name,
                                    arg_tys
                                        .iter()
                                        .map(|t| self.st.display_type(t))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                            );
                        }
                        (None, field_tys.clone())
                    }
                }
            } else {
                (None, Vec::new())
            };
            // Generic inference must use ctor *fields* (`Tuple2._1: A`) even when
            // the picked `<init>` is erased to `(Any, Any)` in the prelude.
            let nargs = args.len();
            let unify_params = if infer && field_tys.len() == nargs && !field_tys.is_empty() {
                field_tys.clone()
            } else {
                ctor_params.clone()
            };
            let mut inferred_args: Vec<Type> = Vec::new();
            if let Some(c) = class_id {
                if !explicit.is_empty() {
                    inferred_args = explicit;
                    tree.ty = Type::Class {
                        sym: c,
                        args: inferred_args.clone(),
                    };
                    fun.ty = tree.ty.clone();
                } else if infer {
                    let arg_tys: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
                    let pt_args: Vec<Type> = match pt {
                        Type::Class { args: a, sym } if *sym == c => a.clone(),
                        Type::Tuple(ts)
                            if self.st.get(c).name.starts_with("Tuple")
                                && ts.len() == tps.len() =>
                        {
                            ts.clone()
                        }
                        _ => Vec::new(),
                    };
                    for (i, tp) in tps.iter().enumerate() {
                        inferred_args.push(
                            unify_tparam(*tp, &unify_params, &arg_tys)
                                .or_else(|| pt_args.get(i).cloned())
                                .unwrap_or(Type::Any),
                        );
                    }
                    tree.ty = Type::Class {
                        sym: c,
                        args: inferred_args.clone(),
                    };
                    fun.ty = tree.ty.clone();
                } else {
                    tree.ty = fun.ty.clone();
                }
            } else {
                tree.ty = fun.ty.clone();
            }
            for (i, a) in args.iter_mut().enumerate() {
                let mut p = if infer && field_tys.len() == nargs && !field_tys.is_empty() {
                    field_tys.get(i).cloned().unwrap_or(Type::NoType)
                } else {
                    ctor_params.get(i).cloned().unwrap_or(Type::NoType)
                };
                if let Some(c) = class_id {
                    if !inferred_args.is_empty() {
                        p = self.st.subst_tparams(c, &inferred_args, &p);
                    }
                }
                if !p.is_no_type() {
                    self.adapt(a, &p);
                }
            }
            tree.sym = ctor_sym.or(class_id).unwrap_or(SymbolId::NONE);
            if let Some(csym) = ctor_sym {
                let mut ctor_ty = self.st.get(csym).ty.clone();
                if let Some(c) = class_id {
                    if !inferred_args.is_empty() {
                        ctor_ty = self.st.subst_tparams(c, &inferred_args, &ctor_ty);
                    }
                }
                let ctor_fun = Tree {
                    id: fun.id,
                    span: fun.span,
                    kind: TreeKind::Ident {
                        name: "<init>".into(),
                    },
                    ty: ctor_ty,
                    sym: csym,
                    postfix: false,
                };
                let _ =
                    self.fill_defaults_and_implicits(tree.span, args, &ctor_params, &ctor_fun, pt);
            }
            return;
        }

        let dummy_method = Type::Method {
            paramss: vec![],
            ret: Box::new(Type::NoType),
        };
        // Expected type Method so nullary methods (`unary_-`, `def f: Int` called as `f()`)
        // are not auto-applied before this Apply is typed.
        self.type_expr(fun, &dummy_method);
        self.rewrite_receiver_apply(fun);
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
            if let TreeKind::Function { vparams, .. } = &a.kind {
                arg_tys.push(Type::Function {
                    params: vec![Type::NoType; vparams.len()],
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
            OverloadPick::Found(sym, mut param_tys, mut ret) => {
                if !sym.is_none() {
                    fun.sym = sym;
                    tree.sym = sym;
                    if let Some(Type::Class { args, .. }) = recv_ty.as_ref() {
                        if !args.is_empty() {
                            let owner = self.st.get(sym).owner;
                            param_tys = param_tys
                                .iter()
                                .map(|p| self.st.subst_tparams(owner, args, p))
                                .collect();
                            ret = self.st.subst_tparams(owner, args, &ret);
                        }
                    }
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
                            let fret = if matches!(fr.as_ref(), Type::TypeParam(_)) {
                                Box::new(Type::Any)
                            } else {
                                fr.clone()
                            };
                            param_tys[0] = Type::Function {
                                params: vec![elem.clone()],
                                ret: fret,
                            };
                        }
                    }
                    if fun_name == "collect" && !param_tys.is_empty() {
                        if let Type::Class { sym, args } = &param_tys[0] {
                            if is_partial_function_sym(&self.st, *sym) {
                                let to = args.get(1).cloned().unwrap_or(Type::Any);
                                param_tys[0] = Type::Class {
                                    sym: *sym,
                                    args: vec![elem, to],
                                };
                            }
                        }
                    }
                }
                for (i, a) in args.iter_mut().enumerate() {
                    let p = param_at(&param_tys, i).cloned().unwrap_or(Type::NoType);
                    if matches!(a.kind, TreeKind::Function { .. }) || a.ty.is_no_type() {
                        self.type_expr(a, &p);
                    }
                    if !p.is_no_type() {
                        self.adapt(a, &p);
                    }
                    if let TreeKind::Function { body, .. } = &a.kind {
                        let body_ty = body.ty.widen_constant();
                        if let Type::Function { params, ret } = &a.ty {
                            if matches!(ret.as_ref(), Type::Any | Type::NoType)
                                && !body_ty.is_no_type()
                                && !body_ty.is_error()
                            {
                                let params = params.clone();
                                a.ty = Type::Function {
                                    params,
                                    ret: Box::new(body_ty),
                                };
                            }
                        }
                    }
                }
                let nparams = param_tys.len();
                if args.len() > nparams && split_repeated(&param_tys).1.is_none() {
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
                    self.fill_defaults_and_implicits(tree.span, args, &param_tys, fun, pt);
                let method_name = if !sym.is_none() {
                    self.st.get(sym).name.clone()
                } else {
                    fun_name.clone()
                };
                if method_name == "::" {
                    if let Some(a0) = args.first() {
                        ret = Type::Class {
                            sym: self.st.list_sym,
                            args: vec![a0.ty.widen_constant()],
                        };
                    }
                } else if method_name == "apply"
                    && !sym.is_none()
                    && self.st.get(self.st.get(sym).owner).name.starts_with("Some")
                {
                    if let Some(a0) = args.first() {
                        ret = Type::Class {
                            sym: self.st.some_sym,
                            args: vec![a0.ty.widen_constant()],
                        };
                    }
                } else if method_name == "map" {
                    if self.is_array_ops_ty(recv_ty.as_ref()) {
                        if let Some(a0) = args.first() {
                            if let Type::Function { ret: fr, .. } = &a0.ty {
                                ret = Type::Array(Box::new(fr.as_ref().widen_constant()));
                            }
                        }
                    } else if !self.is_with_filter_ty(recv_ty.as_ref()) {
                        if let Some(a0) = args.first() {
                            if let Type::Function { ret: fr, .. } = &a0.ty {
                                if let Some(cls) = recv_ty
                                    .as_ref()
                                    .and_then(|t| self.st.class_sym_of(t))
                                    .map(|c| self.collection_root(c))
                                {
                                    ret = Type::Class {
                                        sym: cls,
                                        args: vec![fr.as_ref().widen_constant()],
                                    };
                                }
                            }
                        }
                    }
                } else if method_name == "collect" {
                    if let Some(a0) = args.first() {
                        let to = match &a0.ty {
                            Type::Class { args, .. } if args.len() >= 2 => Some(args[1].clone()),
                            Type::Function { ret, .. } => Some((**ret).clone()),
                            _ => None,
                        };
                        if let Some(to) = to {
                            if let Some(cls) = recv_ty
                                .as_ref()
                                .and_then(|t| self.st.class_sym_of(t))
                                .map(|c| self.collection_root(c))
                            {
                                ret = Type::Class {
                                    sym: cls,
                                    args: vec![to.widen_constant()],
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
                                args: vec![
                                    args[0].ty.widen_constant(),
                                    args[1].ty.widen_constant(),
                                ],
                            };
                        } else if n == "Vector" && args.len() >= 2 {
                            ret = Type::Class {
                                sym: cls,
                                args: vec![args[1].ty.widen_constant()],
                            };
                        }
                    }
                } else if method_name == ":+" {
                    if let Some(cls) = recv_ty.as_ref().and_then(|t| self.st.class_sym_of(t)) {
                        if self.st.get(cls).name == "Vector" {
                            if let Some(a0) = args.first() {
                                ret = Type::Class {
                                    sym: cls,
                                    args: vec![a0.ty.widen_constant()],
                                };
                            }
                        }
                    }
                } else if method_name == "apply" && !sym.is_none() {
                    let owner_n = self.st.get(self.st.get(sym).owner).name.clone();
                    if owner_n == "Map$" {
                        if let Some(a0) = args.first() {
                            if let Type::Class { args: targs, .. } = &a0.ty {
                                if targs.len() == 2 {
                                    if let Some(map) =
                                        self.st.lookup("Map").into_iter().find(|id| {
                                            self.st.get(*id).kind == crate::symbol::SymKind::Class
                                        })
                                    {
                                        ret = Type::Class {
                                            sym: map,
                                            args: targs.clone(),
                                        };
                                    }
                                }
                            }
                        }
                    } else if owner_n == "Vector$"
                        || owner_n == "List$"
                        || owner_n == "Set$"
                        || owner_n == "Seq$"
                        || owner_n == "LazyList$"
                    {
                        if let Some(a0) = args.first() {
                            if let Some(cls) = self
                                .st
                                .lookup(owner_n.trim_end_matches('$'))
                                .into_iter()
                                .find(|id| self.st.get(*id).kind == crate::symbol::SymKind::Class)
                            {
                                ret = Type::Class {
                                    sym: cls,
                                    args: vec![a0.ty.widen_constant()],
                                };
                            }
                        }
                    } else if owner_n == "Left$" || owner_n == "Right$" {
                        if let Some(inst) = self.instantiate_either_ctor_apply(&owner_n, args, pt) {
                            ret = inst;
                        }
                    } else if owner_n == "Try$" || owner_n == "Success$" {
                        if let Some(a0) = args.first() {
                            let elem = unwrap_fn0_or_byname(&a0.ty);
                            let elem = match &a0.kind {
                                TreeKind::Function { body, .. }
                                    if matches!(elem, Type::Any | Type::AnyRef) =>
                                {
                                    if body.ty.is_no_type() || body.ty.is_error() {
                                        elem
                                    } else {
                                        body.ty.clone()
                                    }
                                }
                                _ => elem,
                            };
                            let cname = owner_n.trim_end_matches('$');
                            if let Some(cls) =
                                self.st.lookup(cname).into_iter().find(|id| {
                                    self.st.get(*id).kind == crate::symbol::SymKind::Class
                                })
                            {
                                ret = Type::Class {
                                    sym: cls,
                                    args: vec![elem.widen_constant()],
                                };
                            }
                        }
                    }
                }
                tree.ty = leftover.unwrap_or(ret);
            }
            OverloadPick::Ambiguous => {
                self.error(
                    tree.span,
                    format!(
                        "ambiguous overload for {} with arguments ({})",
                        fun_name,
                        arg_tys
                            .iter()
                            .map(|t| self.st.display_type(t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
                tree.ty = Type::Error;
            }
            OverloadPick::None => {
                if self.rewrite_apply_extension(fun) {
                    let fun_ty = fun.ty.clone();
                    match self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt) {
                        OverloadPick::Found(sym, param_tys, ret) => {
                            fun.sym = sym;
                            tree.sym = sym;
                            for (i, a) in args.iter_mut().enumerate() {
                                let p = param_at(&param_tys, i).cloned().unwrap_or(Type::NoType);
                                if matches!(a.kind, TreeKind::Function { .. }) || a.ty.is_no_type()
                                {
                                    self.type_expr(a, &p);
                                }
                                if !p.is_no_type() {
                                    self.adapt(a, &p);
                                }
                            }
                            tree.ty = ret;
                            return;
                        }
                        OverloadPick::Ambiguous => {
                            self.error(
                                tree.span,
                                format!(
                                    "ambiguous overload for {} with arguments ({})",
                                    fun_name,
                                    arg_tys
                                        .iter()
                                        .map(|t| self.st.display_type(t))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                            );
                            tree.ty = Type::Error;
                            return;
                        }
                        OverloadPick::None => {}
                    }
                }
                // nsc: `c(1)` looks up `apply`, never `update`. Assignment
                // `c(i) = v` is the only path that rewrites to `update`.
                let has_apply = match &fun_ty {
                    Type::Method { .. } | Type::Overload(_) | Type::Function { .. } => true,
                    Type::Array(_) => true,
                    Type::Class { sym, .. } | Type::ModuleRef(sym) => {
                        !self.st.lookup_member(*sym, "apply").is_empty()
                    }
                    _ => false,
                };
                if !has_apply {
                    self.error(
                        tree.span,
                        format!(
                            "value apply is not a member of {}",
                            self.st.display_type(&fun_ty)
                        ),
                    );
                } else {
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
                }
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

    fn is_array_ops_ty(&self, ty: Option<&Type>) -> bool {
        ty.and_then(|t| self.st.class_sym_of(t))
            .is_some_and(|id| self.st.get(id).name == "ArrayOps")
    }

    fn elem_type(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Class { sym, args }
                if args.len() == 2 && is_tuple2_elem_map(&self.st.get(*sym).name) =>
            {
                Some(Type::Class {
                    sym: self.tuple2_sym(),
                    args: args.clone(),
                })
            }
            Type::Class { sym, .. } if self.st.get(*sym).name == "Range" => Some(Type::Int),
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

    /// `Left.apply` / `Right.apply` → `Left[A, B]` / `Right[A, B]`.
    /// The value argument fills `A` (Left) or `B` (Right); the other param
    /// comes from an expected `Either[A, B]` (or `Nothing` if none).
    fn instantiate_either_ctor_apply(
        &self,
        owner_n: &str,
        args: &[Tree],
        pt: &Type,
    ) -> Option<Type> {
        let (cname, value_idx) = match owner_n {
            "Left$" => ("Left", 0usize),
            "Right$" => ("Right", 1usize),
            _ => return None,
        };
        let cls = self
            .st
            .lookup(cname)
            .into_iter()
            .find(|id| self.st.get(*id).kind == crate::symbol::SymKind::Class)?;
        let val_ty = args.first()?.ty.widen_constant();
        let n = self.st.get(cls).tparams.len();
        let pt_args: &[Type] = match pt {
            Type::Class { args, .. } if args.len() == n => args,
            _ => &[],
        };
        let mut inferred = Vec::with_capacity(n);
        for i in 0..n {
            if i == value_idx {
                inferred.push(val_ty.clone());
            } else {
                inferred.push(pt_args.get(i).cloned().unwrap_or(Type::Nothing));
            }
        }
        Some(Type::Class {
            sym: cls,
            args: inferred,
        })
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
                    let flags = self.st.get(pid).flags;
                    let default_rhs = self.st.get(pid).default_rhs.clone();
                    if flags.contains(Flags::DEFAULTPARAM) {
                        if let Some(filled) = self.default_getter_apply(fun, pid, i + 1, &out) {
                            out.push(filled);
                        } else if let Some(rhs) = default_rhs {
                            out.push(rhs);
                        }
                    } else if flags.contains(Flags::IMPLICIT) {
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
        fun: &Tree,
        pt: &Type,
    ) -> Option<Type> {
        let sym = fun.sym;
        if sym.is_none() {
            return None;
        }
        let fun_ty = &fun.ty;
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
                let start = args.len();
                for (k, pid) in rest.iter().enumerate() {
                    let idx = start + k + 1;
                    if let Some(filled) = self.default_getter_apply(fun, *pid, idx, args) {
                        args.push(filled);
                    } else if let Some(mut rhs) = self.st.get(*pid).default_rhs.clone() {
                        let pty = self.st.get(*pid).ty.clone();
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
                // Prefer the (possibly TypeApply-substituted) method type so
                // `mk[Int](2)` searches `ClassTag[Int]`, not raw `ClassTag[T]`.
                let rest_tys: Vec<Type> = match fun_ty {
                    Type::Method { paramss, .. } if paramss.len() > 1 => {
                        paramss[1..].iter().flatten().cloned().collect()
                    }
                    _ => rest_ids
                        .iter()
                        .map(|id| self.st.get(*id).ty.clone())
                        .collect(),
                };
                let rest_tys = if rest_tys.len() == rest_ids.len() {
                    rest_tys
                } else {
                    rest_ids
                        .iter()
                        .map(|id| self.st.get(*id).ty.clone())
                        .collect()
                };
                let rest_tys = self.instantiate_from_call(sym, &first, args, rest_tys);
                self.fill_implicit_params(span, args, &rest_tys, &rest_ids);
                return None;
            }
            let rest_tys: Vec<Vec<Type>> = match fun_ty {
                Type::Method { paramss, .. } if paramss.len() > 1 => paramss[1..].to_vec(),
                _ => paramss_ids[1..]
                    .iter()
                    .map(|clause| {
                        clause
                            .iter()
                            .map(|id| self.st.get(*id).ty.clone())
                            .collect()
                    })
                    .collect(),
            };
            let rest_tys: Vec<Vec<Type>> = rest_tys
                .into_iter()
                .map(|tys| self.instantiate_from_call(sym, &first, args, tys))
                .collect();
            let ret = match fun_ty {
                Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
                _ => match &self.st.get(sym).ty {
                    Type::Method { ret, .. } => (**ret).clone(),
                    _ => Type::NoType,
                },
            };
            let ret = self
                .instantiate_from_call(sym, &first, args, vec![ret])
                .into_iter()
                .next()
                .unwrap_or(Type::NoType);
            return Some(Type::Method {
                paramss: rest_tys,
                ret: Box::new(ret),
            });
        }
        None
    }

    /// Instantiate remaining clause types with method type arguments inferred
    /// from the already-typed first clause (`T <% Ordered[T]` → `Box => Ordered[Box]`).
    fn instantiate_from_call(
        &self,
        sym: SymbolId,
        first: &[SymbolId],
        args: &[Tree],
        tys: Vec<Type>,
    ) -> Vec<Type> {
        if self.st.get(sym).tparams.is_empty() || tys.is_empty() {
            return tys;
        }
        let orig_first: Vec<Type> = first.iter().map(|id| self.st.get(*id).ty.clone()).collect();
        let arg_tys: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
        let inst = self.infer_method_tparams(sym, &orig_first, &arg_tys);
        if inst.is_empty() {
            return tys;
        }
        let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
        let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
        tys.iter()
            .map(|t| crate::symbol::subst_tparams_slice(&tps, &args_t, t))
            .collect()
    }

    fn default_getter_apply(
        &mut self,
        fun: &Tree,
        param: SymbolId,
        index_1based: usize,
        preceding: &[Tree],
    ) -> Option<Tree> {
        let meth = fun.sym;
        if meth.is_none() {
            return None;
        }
        let mname = self.st.get(meth).name.clone();
        let gname = format!("{mname}$default${index_1based}");
        let owner = self.st.get(meth).owner;
        let gid = self
            .st
            .lookup_member(owner, &gname)
            .into_iter()
            .find(|&id| self.st.get(id).kind == crate::symbol::SymKind::Method)?;
        let span = fun.span;
        let recv = self.method_receiver(fun);
        let mut gfun = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(recv),
                name: gname,
            },
            ty: self.st.get(gid).ty.clone(),
            sym: gid,
            postfix: false,
        };
        self.type_expr(&mut gfun, &Type::NoType);
        let mut call = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(gfun),
                args: preceding.to_vec(),
            },
            ty: self.st.get(param).ty.clone(),
            sym: gid,
            postfix: false,
        };
        self.type_expr(&mut call, &self.st.get(param).ty.clone());
        Some(call)
    }

    fn method_receiver(&self, fun: &Tree) -> Tree {
        match &fun.kind {
            TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
                self.method_receiver(fun)
            }
            TreeKind::Select { qual, .. } => (**qual).clone(),
            _ => {
                let this_ty = if self.st.this_class.is_none() {
                    Type::NoType
                } else {
                    Type::ModuleRef(self.st.this_class)
                };
                Tree {
                    id: NodeId(0),
                    span: fun.span,
                    kind: TreeKind::This { qual: None },
                    ty: this_ty,
                    sym: self.st.this_class,
                    postfix: false,
                }
            }
        }
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
                    if let Some(ct) = self.classtag_apply_fallback(&pty, span) {
                        args.push(ct);
                    } else if let Some(lam) = self.identity_view(&pty, span) {
                        args.push(lam);
                    } else {
                        self.error(span, self.missing_implicit_message(&pty));
                    }
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

    /// nsc fills `ClassTag[String]` via `ClassTag.apply(classOf[String])` when
    /// there is no primitive getter (`ClassTag.Int`, …).
    fn classtag_apply_fallback(&self, pt: &Type, span: Span) -> Option<Tree> {
        let Type::Class { sym, args } = pt else {
            return None;
        };
        if self.st.get(*sym).name != "ClassTag" || args.is_empty() {
            return None;
        }
        let elem = args[0].clone();
        let module = self.st.companion_module(*sym)?;
        let mcls = self.st.module_class_of(module);
        let apply = self
            .st
            .lookup_member(mcls, "apply")
            .into_iter()
            .find(|&id| self.st.get(id).kind == crate::symbol::SymKind::Method)?;
        let class_arg = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: "$classOf".into(),
            },
            ty: elem,
            sym: SymbolId::NONE,
            postfix: false,
        };
        let recv = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: "ClassTag".into(),
            },
            ty: Type::ModuleRef(module),
            sym: module,
            postfix: false,
        };
        let fun = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(recv),
                name: "apply".into(),
            },
            ty: self.st.get(apply).ty.clone(),
            sym: apply,
            postfix: false,
        };
        Some(Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(fun),
                args: vec![class_arg],
            },
            ty: pt.clone(),
            sym: apply,
            postfix: false,
        })
    }

    /// nsc: `A <: B` is a view `A => B` (identity / asInstanceOf).
    fn identity_view(&mut self, pt: &Type, span: Span) -> Option<Tree> {
        let Type::Function { params, ret } = pt else {
            return None;
        };
        if params.len() != 1 {
            return None;
        }
        if !self.st.is_sub_type(&params[0], ret) {
            return None;
        }
        let from = params[0].clone();
        let to = (**ret).clone();
        self.gensym += 1;
        let pname = format!("x${}", self.gensym);
        let pid = self.st.alloc(
            &pname,
            self.st.owner,
            crate::symbol::SymKind::Term,
            Flags::PARAM.with(Flags::SYNTHETIC),
            "",
        );
        self.st.get_mut(pid).ty = from.clone();
        let ident = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: pname.clone(),
            },
            ty: from.clone(),
            sym: pid,
            postfix: false,
        };
        let param = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::ValDef {
                mods: Modifiers::new(Flags::PARAM),
                name: pname,
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(Tree::dummy(TreeKind::Empty)),
            },
            ty: from.clone(),
            sym: pid,
            postfix: false,
        };
        let mut lam = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Function {
                vparams: vec![param],
                body: Box::new(ident),
            },
            ty: Type::Function {
                params: vec![from],
                ret: Box::new(to.clone()),
            },
            sym: SymbolId::NONE,
            postfix: false,
        };
        self.type_expr(&mut lam, pt);
        self.adapt(&mut lam, pt);
        Some(lam)
    }

    fn resolve_overload(
        &self,
        fun_ty: &Type,
        fun_sym: SymbolId,
        arg_tys: &[Type],
        _pt: &Type,
    ) -> OverloadPick {
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
                if !fun_sym.is_none() {
                    let name = self.st.get(fun_sym).name.clone();
                    let owner = self.st.get(fun_sym).owner;
                    cands.clear();
                    let methods = self.drop_overridden(self.st.lookup_member(owner, &name));
                    for m in methods {
                        if let Type::Method { paramss, ret } = &self.st.get(m).ty {
                            cands.push((
                                m,
                                paramss.first().cloned().unwrap_or_default(),
                                (**ret).clone(),
                            ));
                        }
                    }
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
            Type::Array(elem) => {
                for m in self.st.lookup_member(self.st.array_sym, "apply") {
                    cands.push((m, vec![Type::Int], (**elem).clone()));
                }
            }
            _ => return OverloadPick::None,
        }
        let applicable: Vec<(SymbolId, Vec<Type>, Type)> = {
            let no_view: Vec<_> = cands
                .iter()
                .filter(|(sym, ps, _)| self.is_applicable(*sym, ps, arg_tys, false))
                .cloned()
                .collect();
            if !no_view.is_empty() {
                no_view
            } else {
                cands
                    .into_iter()
                    .filter(|(sym, ps, _)| self.is_applicable(*sym, ps, arg_tys, true))
                    .collect()
            }
        };
        match applicable.len() {
            0 => OverloadPick::None,
            1 => {
                let (s, p, r) = applicable.into_iter().next().unwrap();
                OverloadPick::Found(s, p, r)
            }
            _ => {
                let winners: Vec<(SymbolId, Vec<Type>, Type)> = applicable
                    .iter()
                    .filter(|a| {
                        applicable
                            .iter()
                            .all(|b| a.0 == b.0 || self.is_as_specific_method(&a.1, &b.1))
                    })
                    .cloned()
                    .collect();
                match winners.len() {
                    1 => {
                        let (s, p, r) = winners.into_iter().next().unwrap();
                        OverloadPick::Found(s, p, r)
                    }
                    _ => OverloadPick::Ambiguous,
                }
            }
        }
    }

    /// nsc: `A` is as specific as `B` when `B` is applicable to `A`'s parameter types.
    fn is_as_specific_method(&self, a_ps: &[Type], b_ps: &[Type]) -> bool {
        self.is_applicable(SymbolId::NONE, b_ps, a_ps, true)
    }

    fn is_applicable(
        &self,
        sym: SymbolId,
        params: &[Type],
        args: &[Type],
        allow_widen: bool,
    ) -> bool {
        let instantiated;
        let params = if !sym.is_none() && !self.st.get(sym).tparams.is_empty() {
            let inst = self.infer_method_tparams(sym, params, args);
            if inst.is_empty() {
                params
            } else {
                let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
                instantiated = params
                    .iter()
                    .map(|p| crate::symbol::subst_tparams_slice(&tps, &args_t, p))
                    .collect::<Vec<_>>();
                instantiated.as_slice()
            }
        } else {
            params
        };
        let (fixed, repeated) = split_repeated(params);
        if let Some(elem) = repeated {
            if args.len() < fixed.len() {
                return false;
            }
            return args
                .iter()
                .zip(fixed)
                .all(|(a, p)| self.arg_conforms(a, p, allow_widen))
                && args[fixed.len()..]
                    .iter()
                    .all(|a| self.arg_conforms(a, elem, allow_widen));
        }
        if args.len() > params.len() {
            return false;
        }
        if args.len() < params.len() && !self.trailing_omissible(sym, args.len(), params.len()) {
            return false;
        }
        args.iter()
            .zip(params)
            .all(|(a, p)| self.arg_conforms(a, p, allow_widen))
    }

    fn arg_conforms(&self, arg: &Type, param: &Type, allow_widen: bool) -> bool {
        match self.arg_score(arg, param) {
            Some(3) if !allow_widen => false, // numeric widen
            Some(_) => true,
            None if allow_widen => {
                // nsc view: `wrapString` makes String applicable to Seq.
                !matches!(self.search_conversion(arg, param), ImplicitSearch::None)
            }
            None => false,
        }
    }

    fn trailing_omissible(&self, sym: SymbolId, given: usize, total: usize) -> bool {
        if sym.is_none() || given >= total {
            return false;
        }
        let s = self.st.get(sym);
        let ids = if !s.params.is_empty() {
            s.params.clone()
        } else {
            s.paramss.first().cloned().unwrap_or_default()
        };
        if ids.len() < total {
            return false;
        }
        ids[given..total].iter().all(|p| {
            let f = self.st.get(*p).flags;
            f.contains(Flags::DEFAULTPARAM) || f.contains(Flags::IMPLICIT)
        })
    }

    fn compat_score(&self, params: &[Type], args: &[Type]) -> Option<i32> {
        let (fixed, repeated) = split_repeated(params);
        if let Some(elem) = repeated {
            if args.len() < fixed.len() {
                return None;
            }
            let mut s = 0;
            for (a, p) in args.iter().zip(fixed) {
                s += self.arg_score(a, p)?;
            }
            for a in &args[fixed.len()..] {
                s += self.arg_score(a, elem)?;
            }
            return Some(s);
        }
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
        if let Type::Repeated(inner) = param {
            return self.arg_score(arg, inner);
        }
        if let Type::Method { paramss, ret } = arg {
            let f = Type::Function {
                params: paramss.iter().flatten().cloned().collect(),
                ret: ret.clone(),
            };
            return self.arg_score(&f, param);
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
        // Overload scoring only: `is_sub_type` compares class args so
        // `ClassTag[Int]` does not inhabit `ClassTag[T]`. Constructors whose
        // args are type parameters still match, the way nsc infers `Map.apply`.
        if class_ctor_matches_typeparam_args(arg, param) {
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
        let pf_result = partial_function_type(&self.st, pt);
        let sam = if pf_result.is_none() {
            self.st.sam_sig(pt)
        } else {
            None
        };
        let (pts, ret_pt) = if let Some((from, to)) = &pf_result {
            (vec![from.clone()], to.clone())
        } else if let Some(sam) = &sam {
            (sam.param_tys.clone(), sam.ret_ty.clone())
        } else {
            match pt {
                Type::Function { params, ret } => (params.clone(), (**ret).clone()),
                Type::Named { name, args }
                    if name.starts_with("Function") && name != "Function" =>
                {
                    if args.is_empty() {
                        (vec![Type::NoType; vparams.len()], Type::NoType)
                    } else {
                        let ret = args.last().cloned().unwrap_or(Type::NoType);
                        (args[..args.len() - 1].to_vec(), ret)
                    }
                }
                _ => (vec![Type::NoType; vparams.len()], Type::NoType),
            }
        };
        // nsc: expected FunctionN / SAM param types apply only when the
        // expanded section's arity matches. `_ + _` against `Int => Int`
        // (or `_ + 1` against `(Int, Int) => Int`) leaves every param unknown
        // (`missing parameter type for expanded function`) and then mismatches.
        let (pts, ret_pt) = if pts.len() == vparams.len() {
            (pts, ret_pt)
        } else {
            (vec![Type::NoType; vparams.len()], Type::NoType)
        };
        self.st.push_scope();
        let mut param_tys = Vec::new();
        for (i, p) in vparams.iter_mut().enumerate() {
            self.type_val_sig(p);
            if p.ty.is_no_type() {
                p.ty = pts.get(i).cloned().unwrap_or(Type::NoType);
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
        param_tys.clear();
        for p in vparams.iter_mut() {
            if p.ty.is_no_type() && !p.sym.is_none() {
                p.ty = self.st.get(p.sym).ty.clone();
            }
            if p.ty.is_no_type() {
                self.error(p.span, "missing parameter type for expanded function");
                p.ty = Type::Error;
                if !p.sym.is_none() {
                    self.st.get_mut(p.sym).ty = Type::Error;
                }
            }
            param_tys.push(p.ty.clone());
        }
        let ret = if ret_pt.is_no_type() {
            body.ty.clone()
        } else {
            self.adapt(body, &ret_pt);
            ret_pt
        };
        self.st.pop_scope();
        if pf_result.is_some() {
            pt.clone()
        } else if sam.is_some() && param_tys.len() == pts.len() {
            pt.clone()
        } else {
            Type::Function {
                params: param_tys,
                ret: Box::new(ret),
            }
        }
    }

    fn f_arg_ok(&self, ty: &Type, kind: scala_rs_parser::finterp::FConvKind) -> bool {
        use scala_rs_parser::finterp::FConvKind;
        let ty = ty.widen_constant();
        match kind {
            FConvKind::General => true,
            FConvKind::Integral => matches!(ty, Type::Int | Type::Long | Type::Byte | Type::Short),
            FConvKind::Floating => matches!(ty, Type::Float | Type::Double),
            FConvKind::Character => matches!(ty, Type::Char | Type::Int),
            FConvKind::Unsupported => false,
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
        if let TreeKind::Match { selector, cases } = &tree.kind {
            self.check_match_exhaustive(span, &sel_ty, cases);
            if tree_has_switch(selector) && !match_can_switch(&sel_ty, cases) {
                self.warning(
                    selector.span,
                    "could not emit switch for @switch annotated match",
                );
            }
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
            TreeKind::Select { .. } => {
                // Stable identifier pattern (`Color.RED`, `java.lang.Thread.State.NEW`).
                self.type_expr(pat, sel_ty);
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

    fn tree_to_type(&mut self, tpt: &Tree) -> Type {
        match &tpt.kind {
            TreeKind::Empty => Type::NoType,
            TreeKind::Ident { name } if name == "_" => Type::Wildcard,
            TreeKind::Ident { name } => {
                self.expose_unqualified(name, tpt.span);
                self.resolve_type_name(name, &[])
            }
            TreeKind::Select { name, qual } => {
                if let TreeKind::Ident { name: q } = &qual.kind {
                    let q = q.clone();
                    self.expose_unqualified(&q, tpt.span);
                }
                if name == "String" && !self.type_select_is_term_prefix(qual) {
                    Type::String
                } else if self.type_select_is_term_prefix(qual) {
                    self.path_dependent_type(tpt.span, qual, name)
                } else if let Some(id) = self.lookup_qualified_type(qual, name) {
                    match self.st.get(id).kind {
                        SymKind::Module | SymKind::ModuleClass => Type::ModuleRef(id),
                        SymKind::TypeParam => Type::TypeParam(id),
                        SymKind::TypeMember => Type::TypeMember(id),
                        _ => Type::Class {
                            sym: id,
                            args: vec![],
                        },
                    }
                } else {
                    self.resolve_type_name(name, &[])
                }
            }
            TreeKind::SelectFromTypeTree { qual, name, hash } => {
                if !*hash {
                    if self.type_select_is_term_prefix(qual)
                        || matches!(&qual.kind, TreeKind::This { .. } | TreeKind::Super { .. })
                    {
                        return self.path_dependent_type(tpt.span, qual, name);
                    }
                }
                let prefix = self.tree_to_type(qual);
                self.project_from_prefix(tpt.span, &prefix, name)
            }
            TreeKind::CompoundTypeTree {
                parents,
                refinements,
            } => self.compound_to_type(tpt.span, parents, refinements),
            TreeKind::SingletonTypeTree { ref_ } => self.singleton_to_type(tpt.span, ref_),
            TreeKind::AnnotatedTypeTree { tpt: inner, annot } => {
                let ty = self.tree_to_type(inner);
                let path = annot.annotation_path();
                let simple = path.rsplit('.').next().unwrap_or(path.as_str()).to_string();
                Type::Annotated {
                    tpe: Box::new(ty),
                    annot: simple,
                }
            }
            TreeKind::AppliedTypeTree { tpt, args } => {
                let span = tpt.span;
                let mut as_ = Vec::new();
                for a in args {
                    as_.push(self.tree_to_type(a));
                }
                match tpt.name() {
                    Some("Array") => {
                        Type::Array(Box::new(as_.first().cloned().unwrap_or(Type::Any)))
                    }
                    Some("<repeated>") => {
                        Type::Repeated(Box::new(as_.first().cloned().unwrap_or(Type::Any)))
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
                    Some(n) if n.starts_with("Function") && n != "Function" => {
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
                    Some("Function") => match self.tree_to_type(tpt) {
                        Type::Class { sym, .. } => {
                            self.apply_types(Type::Class { sym, args: vec![] }, as_, span)
                        }
                        ctor => self.apply_types(ctor, as_, span),
                    },
                    Some(n) if n.starts_with("Tuple") => Type::Tuple(as_),
                    Some(_) => {
                        let ctor = self.tree_to_type(tpt);
                        self.apply_types(ctor, as_, span)
                    }
                    None => Type::Error,
                }
            }
            TreeKind::TypeApply { fun, args } => {
                // `new C[T]` in term position may be TypeApply; treat it as a type.
                let mut as_ = Vec::new();
                for a in args {
                    as_.push(self.tree_to_type(a));
                }
                match fun.name() {
                    Some("Array") => {
                        Type::Array(Box::new(as_.first().cloned().unwrap_or(Type::Any)))
                    }
                    Some(n) => {
                        let ctor = self.resolve_type_name(n, &[]);
                        self.apply_types(ctor, as_, fun.span)
                    }
                    None => Type::Error,
                }
            }
            TreeKind::Literal { lit } => Type::Constant(lit.clone()),
            TreeKind::TypeDef {
                name,
                tparams,
                lo,
                hi,
                rhs,
                ..
            } => {
                if name == "_" {
                    if !tparams.is_empty() {
                        self.error(tpt.span, "unimplemented type: higher-kinded wildcard");
                        return Type::Error;
                    }
                    if lo.is_none() && hi.is_none() {
                        Type::Wildcard
                    } else {
                        Type::BoundedWildcard {
                            lo: lo.as_ref().map(|t| Box::new(self.tree_to_type(t))),
                            hi: hi.as_ref().map(|t| Box::new(self.tree_to_type(t))),
                        }
                    }
                } else if !rhs.is_empty() {
                    self.tree_to_type(rhs)
                } else {
                    Type::Named {
                        name: name.clone(),
                        args: vec![],
                    }
                }
            }
            TreeKind::ExistentialTypeTree {
                tpt: inner,
                clauses,
            } => {
                let mut quantified = Vec::new();
                let mut val_clauses: Vec<(String, Tree, Span)> = Vec::new();
                let mut ok = true;
                for c in clauses {
                    match &c.kind {
                        TreeKind::TypeDef {
                            name,
                            tparams,
                            lo,
                            hi,
                            rhs,
                            ..
                        } => {
                            if !tparams.is_empty() || !rhs.is_empty() {
                                self.error(
                                    c.span,
                                    "unimplemented type: bounded or higher-kinded existential",
                                );
                                ok = false;
                            } else {
                                let lo_ty = lo.as_ref().map(|t| self.tree_to_type(t));
                                let hi_ty = hi.as_ref().map(|t| self.tree_to_type(t));
                                quantified.push(ExistQuant {
                                    name: name.clone(),
                                    lo: lo_ty,
                                    hi: hi_ty,
                                });
                            }
                        }
                        TreeKind::ValDef { name, tpt, .. } => {
                            val_clauses.push((name.clone(), (**tpt).clone(), c.span));
                        }
                        TreeKind::Unimplemented { what } => {
                            self.error(c.span, format!("unimplemented type: {what}"));
                            ok = false;
                        }
                        _ => {
                            self.error(c.span, "unimplemented type: existential clause");
                            ok = false;
                        }
                    }
                }
                if !val_clauses.is_empty() {
                    if quantified.is_empty() {
                        if let Some(packed) = self.pack_value_existential(inner, &val_clauses) {
                            return packed;
                        }
                    }
                    for (_, _, sp) in &val_clauses {
                        self.error(
                            *sp,
                            "unimplemented type: value existential (`forSome { val … }`)",
                        );
                    }
                    return Type::Error;
                }
                let ty = self.tree_to_type(inner);
                if !ok {
                    return Type::Error;
                }
                subst_quantified(ty, &quantified)
            }
            TreeKind::Unimplemented { what } => {
                self.error(tpt.span, format!("unimplemented type: {what}"));
                Type::Error
            }
            _ => Type::Named {
                name: tpt.name().unwrap_or("?").to_string(),
                args: vec![],
            },
        }
    }

    /// `p.Inner forSome { val p: Outer }` packs to `Outer#Inner` (tiny legal case).
    fn pack_value_existential(
        &mut self,
        inner: &Tree,
        vals: &[(String, Tree, Span)],
    ) -> Option<Type> {
        if vals.len() != 1 {
            return None;
        }
        let (vname, tpt, _) = &vals[0];
        let (pname, tname) = match &inner.kind {
            TreeKind::Select { qual, name } => match &qual.kind {
                TreeKind::Ident { name: q } => (q.as_str(), name.as_str()),
                _ => return None,
            },
            TreeKind::SelectFromTypeTree { qual, name, hash } if !*hash => match &qual.kind {
                TreeKind::Ident { name: q } => (q.as_str(), name.as_str()),
                _ => return None,
            },
            _ => return None,
        };
        if pname != vname {
            return None;
        }
        let prefix = self.tree_to_type(tpt);
        if prefix.is_error() {
            return None;
        }
        Some(self.project_from_prefix(inner.span, &prefix, tname))
    }

    /// `p.T` where `p` is a term is path-dependent; `java.lang.String` is not.
    fn type_select_is_term_prefix(&self, t: &Tree) -> bool {
        match &t.kind {
            TreeKind::This { .. } | TreeKind::Super { .. } => true,
            TreeKind::Ident { name } => {
                let found = self.st.lookup(name);
                let type_like = found.iter().any(|s| {
                    matches!(
                        self.st.get(*s).kind,
                        SymKind::Class
                            | SymKind::Module
                            | SymKind::ModuleClass
                            | SymKind::Package
                            | SymKind::TypeParam
                            | SymKind::TypeMember
                    )
                });
                let term_like = found
                    .iter()
                    .any(|s| matches!(self.st.get(*s).kind, SymKind::Term | SymKind::Method));
                term_like && !type_like
            }
            TreeKind::Select { qual, .. } => self.type_select_is_term_prefix(qual),
            _ => false,
        }
    }

    fn project_type_member(&mut self, span: Span, prefix: Type, name: &str) -> Type {
        self.project_from_prefix(span, &prefix, name)
    }

    fn project_from_prefix(&mut self, span: Span, prefix: &Type, name: &str) -> Type {
        if let Type::Refined { .. } = prefix {
            if let Some(t) = self.st.lookup_type_member_on(prefix, name) {
                return t;
            }
            self.error(
                span,
                format!(
                    "type {name} is not a member of {}",
                    self.st.display_type(prefix)
                ),
            );
            return Type::Error;
        }
        let cls = match prefix {
            Type::TypeMember(id) => {
                if !self.st.get(*id).tparams.is_empty() {
                    let n = self.st.get(*id).name.clone();
                    self.error(span, format!("type {n} takes type parameters"));
                    return Type::Error;
                }
                let seen = self.st.type_member_as_seen(*id);
                if !matches!(seen, Type::TypeMember(_)) {
                    return self.project_from_prefix(span, &seen, name);
                }
                if let Some(hi) = self.st.get(*id).bound_hi.clone() {
                    return self.project_from_prefix(span, &hi, name);
                }
                self.error(
                    span,
                    format!(
                        "type {name} is not a member of {}",
                        self.st.display_type(prefix)
                    ),
                );
                return Type::Error;
            }
            other => match self.st.class_sym_of(other) {
                Some(sym) => sym,
                None => {
                    self.error(
                        span,
                        format!(
                            "type {name} is not a member of {}",
                            self.st.display_type(other)
                        ),
                    );
                    return Type::Error;
                }
            },
        };
        let mut found = self.st.lookup_member(cls, name);
        found.sort_by_key(|s| if self.st.get(*s).owner == cls { 0 } else { 1 });
        for m in found {
            let ty = match self.st.get(m).kind {
                SymKind::TypeMember => self.st.type_member_as_seen(m),
                SymKind::Class | SymKind::ModuleClass => Type::Class {
                    sym: m,
                    args: vec![],
                },
                _ => continue,
            };
            return self.st.expand_in_type(prefix, &ty);
        }
        self.error(
            span,
            format!(
                "type {name} is not a member of {}",
                self.st.display_type(prefix)
            ),
        );
        Type::Error
    }

    fn path_dependent_type(&mut self, span: Span, prefix: &Tree, name: &str) -> Type {
        if !self.is_stable_path(prefix) {
            self.error(
                span,
                format!(
                    "stable identifier required, but {} found",
                    path_display(prefix)
                ),
            );
            return Type::Error;
        }
        let Some(pty) = self.term_path_type(prefix) else {
            self.error(
                span,
                format!(
                    "stable identifier required, but {} found",
                    path_display(prefix)
                ),
            );
            return Type::Error;
        };
        self.project_from_prefix(span, &pty, name)
    }

    fn is_stable_path(&self, t: &Tree) -> bool {
        match &t.kind {
            TreeKind::This { .. } => true,
            TreeKind::Ident { name } => self.ident_is_stable(name),
            TreeKind::Select { qual, name } => {
                self.is_stable_path(qual) && self.member_is_stable(qual, name)
            }
            TreeKind::SelectFromTypeTree { qual, hash, name } if !hash => {
                self.is_stable_path(qual) && self.member_is_stable(qual, name)
            }
            TreeKind::Apply { .. } | TreeKind::New { .. } => false,
            _ => false,
        }
    }

    fn singleton_to_type(&mut self, span: Span, ref_: &Tree) -> Type {
        match &ref_.kind {
            TreeKind::This { qual } => {
                let id = if let Some(name) = qual {
                    self.st
                        .enclosing_class_named(self.st.this_class, name)
                        .unwrap_or(self.st.this_class)
                } else {
                    self.st.this_class
                };
                if id.is_none() {
                    self.error(span, "`this.type` is not allowed here");
                    Type::Error
                } else {
                    Type::ThisType(id)
                }
            }
            _ => {
                if !self.is_stable_path(ref_) {
                    self.error(
                        span,
                        format!(
                            "stable identifier required, but {} found",
                            path_display(ref_)
                        ),
                    );
                    return Type::Error;
                }
                let Some(sym) = self.term_path_sym(ref_) else {
                    self.error(
                        span,
                        format!(
                            "stable identifier required, but {} found",
                            path_display(ref_)
                        ),
                    );
                    return Type::Error;
                };
                let owner = self.st.get(sym).owner;
                let prefix =
                    if owner.is_none() || matches!(self.st.get(owner).kind, SymKind::Method) {
                        Type::NoType
                    } else {
                        Type::ThisType(owner)
                    };
                Type::SingleType {
                    prefix: Box::new(prefix),
                    sym,
                }
            }
        }
    }

    fn term_path_sym(&self, t: &Tree) -> Option<SymbolId> {
        match &t.kind {
            TreeKind::Ident { name } => self.st.lookup(name).into_iter().find(|s| {
                matches!(
                    self.st.get(*s).kind,
                    SymKind::Term | SymKind::Module | SymKind::ModuleClass
                )
            }),
            TreeKind::Select { qual, name } | TreeKind::SelectFromTypeTree { qual, name, .. } => {
                let qt = self.term_path_type(qual)?;
                if let Type::Refined { decls, .. } = &qt {
                    if decls.iter().any(|d| {
                        matches!(
                            d,
                            scala_rs_parser::RefineDecl::Val { name: n, .. } if n == name
                        )
                    }) {
                        return None;
                    }
                }
                let cls = self.st.class_sym_of(&qt)?;
                self.st.lookup_member(cls, name).into_iter().find(|s| {
                    matches!(
                        self.st.get(*s).kind,
                        SymKind::Term | SymKind::Module | SymKind::ModuleClass
                    )
                })
            }
            _ => None,
        }
    }

    fn ident_is_stable(&self, name: &str) -> bool {
        let found = self.st.lookup(name);
        found.iter().any(|s| {
            let sy = self.st.get(*s);
            match sy.kind {
                SymKind::Module | SymKind::ModuleClass | SymKind::Package => true,
                SymKind::Term => !sy.flags.contains(Flags::MUTABLE),
                SymKind::Method => false,
                _ => false,
            }
        })
    }

    fn member_is_stable(&self, qual: &Tree, name: &str) -> bool {
        let Some(pty) = self.term_path_type(qual) else {
            return false;
        };
        if let Type::Refined { decls, .. } = &pty {
            if decls.iter().any(|d| {
                matches!(
                    d,
                    scala_rs_parser::RefineDecl::Val { name: n, .. } if n == name
                )
            }) {
                return true;
            }
            if decls.iter().any(|d| {
                matches!(
                    d,
                    scala_rs_parser::RefineDecl::Def { name: n, .. } if n == name
                )
            }) {
                return false;
            }
        }
        let Some(cls) = self.st.class_sym_of(&pty) else {
            return false;
        };
        self.st.lookup_member(cls, name).iter().any(|s| {
            let sy = self.st.get(*s);
            match sy.kind {
                SymKind::Module | SymKind::ModuleClass | SymKind::Package => true,
                SymKind::Term => !sy.flags.contains(Flags::MUTABLE),
                SymKind::Method => false,
                _ => false,
            }
        })
    }

    fn term_path_type(&self, t: &Tree) -> Option<Type> {
        match &t.kind {
            TreeKind::This { .. } => {
                if self.st.this_class.is_none() {
                    None
                } else {
                    Some(self.st.type_of_class(self.st.this_class))
                }
            }
            TreeKind::Ident { name } => {
                let found = self.st.lookup(name);
                found.into_iter().find_map(|s| {
                    let sy = self.st.get(s);
                    match sy.kind {
                        SymKind::Term | SymKind::Method => Some(sy.ty.clone()),
                        SymKind::Module | SymKind::ModuleClass => Some(self.st.type_of_class(s)),
                        _ => None,
                    }
                })
            }
            TreeKind::Select { qual, name } | TreeKind::SelectFromTypeTree { qual, name, .. } => {
                let qt = self.term_path_type(qual)?;
                if let Type::Refined { decls, .. } = &qt {
                    if let Some(t) = SymbolTable::refine_member_type(decls, name) {
                        return Some(self.st.expand_in_type(&qt, &t));
                    }
                }
                let cls = self.st.class_sym_of(&qt)?;
                self.st.lookup_member(cls, name).into_iter().find_map(|s| {
                    let sy = self.st.get(s);
                    match sy.kind {
                        SymKind::Term | SymKind::Method => {
                            Some(self.st.expand_in_type(&qt, &sy.ty))
                        }
                        SymKind::Module | SymKind::ModuleClass => Some(self.st.type_of_class(s)),
                        _ => None,
                    }
                })
            }
            _ => None,
        }
    }

    fn compound_to_type(&mut self, span: Span, parents: &[Tree], refinements: &[Tree]) -> Type {
        let ps: Vec<Type> = parents.iter().map(|p| self.tree_to_type(p)).collect();
        if ps.iter().any(|p| p.is_error()) {
            return Type::Error;
        }
        if !self.compound_parents_ok(span, &ps) {
            return Type::Error;
        }
        let mut decls = Vec::new();
        let mut ok = true;
        self.st.push_scope();
        for r in refinements {
            if let TreeKind::TypeDef { .. } = &r.kind {
                match self.refinement_type_member(r) {
                    Some(d) => decls.push(d),
                    None => ok = false,
                }
            }
        }
        for r in refinements {
            match &r.kind {
                TreeKind::TypeDef { .. } => {}
                TreeKind::DefDef {
                    name,
                    tparams,
                    vparamss,
                    tpt,
                    rhs,
                    ..
                } => {
                    if !rhs.is_empty() {
                        self.error(r.span, "illegal implementation in refinement");
                        ok = false;
                        continue;
                    }
                    if !tparams.is_empty() {
                        self.error(r.span, "unimplemented type: type parameters in refinement");
                        ok = false;
                        continue;
                    }
                    let mut paramss = Vec::new();
                    for clause in vparamss {
                        let mut ct = Vec::new();
                        for p in clause {
                            if let TreeKind::ValDef { tpt, .. } = &p.kind {
                                ct.push(self.tree_to_type(tpt));
                            } else if !p.ty.is_no_type() {
                                ct.push(p.ty.clone());
                            } else {
                                ct.push(Type::Any);
                            }
                        }
                        paramss.push(ct);
                    }
                    let ret = self.tree_to_type(tpt);
                    decls.push(scala_rs_parser::RefineDecl::Def {
                        name: name.clone(),
                        paramss,
                        ret,
                    });
                }
                TreeKind::ValDef {
                    name,
                    tpt,
                    rhs,
                    mods,
                    ..
                } => {
                    if !rhs.is_empty() {
                        self.error(r.span, "illegal implementation in refinement");
                        ok = false;
                        continue;
                    }
                    let ty = self.tree_to_type(tpt);
                    if mods.flags.contains(Flags::MUTABLE) {
                        // nsc `{ var foo: T }` ≡ getter `foo` + setter `foo_=`.
                        decls.push(scala_rs_parser::RefineDecl::Val {
                            name: name.clone(),
                            ty: ty.clone(),
                        });
                        decls.push(scala_rs_parser::RefineDecl::Def {
                            name: format!("{name}_="),
                            paramss: vec![vec![ty]],
                            ret: Type::Unit,
                        });
                    } else {
                        decls.push(scala_rs_parser::RefineDecl::Val {
                            name: name.clone(),
                            ty,
                        });
                    }
                }
                TreeKind::Unimplemented { what } => {
                    self.error(r.span, format!("unimplemented type: {what}"));
                    ok = false;
                }
                _ => {
                    self.error(r.span, "unimplemented: structural update");
                    ok = false;
                }
            }
        }
        self.st.pop_scope();
        if !ok {
            return Type::Error;
        }
        let _ = span;
        Type::Refined { parents: ps, decls }
    }

    /// Type a refinement `type` member, including HK `type F[_]` / `type F[X] = Id[X]`
    /// and bounded `type A <: T`. Nullary class/trait `type A <: T` stays unimplemented.
    fn refinement_type_member(&mut self, r: &Tree) -> Option<scala_rs_parser::RefineDecl> {
        let TreeKind::TypeDef {
            name,
            tparams,
            rhs,
            lo,
            hi,
            ..
        } = &r.kind
        else {
            return None;
        };
        let hk = !tparams.is_empty();
        let bounded = lo.is_some() || hi.is_some();
        if !hk && !bounded {
            let alias = if rhs.is_empty() {
                None
            } else {
                Some(self.tree_to_type(rhs))
            };
            let id = self
                .st
                .alloc(name, SymbolId::NONE, SymKind::TypeMember, Flags::EMPTY, "");
            if let Some(t) = &alias {
                self.st.get_mut(id).ty = t.clone();
            } else {
                self.st.get_mut(id).ty = Type::TypeMember(id);
            }
            self.st.enter_in_current(name, id);
            return Some(scala_rs_parser::RefineDecl::Type {
                name: name.clone(),
                rhs: alias,
                tparams: 0,
                lo: None,
                hi: None,
            });
        }
        let id = self
            .st
            .alloc(name, SymbolId::NONE, SymKind::TypeMember, Flags::EMPTY, "");
        self.st.enter_in_current(name, id);
        self.st.push_scope();
        let mut tps = tparams.clone();
        let tp_ids = self.enter_tparams(&mut tps, id);
        self.st.get_mut(id).tparams = tp_ids;
        let lo_ty = lo.as_ref().map(|t| self.tree_to_type(t));
        let hi_ty = hi.as_ref().map(|t| self.tree_to_type(t));
        if let Some(t) = &lo_ty {
            self.check_proper_type(t, r.span);
        }
        if let Some(t) = &hi_ty {
            self.check_proper_type(t, r.span);
        }
        self.st.get_mut(id).bound_lo = lo_ty.clone();
        self.st.get_mut(id).bound_hi = hi_ty.clone();
        let rhs_ty = if rhs.is_empty() {
            Type::TypeMember(id)
        } else {
            let t = self.tree_to_type(rhs);
            self.check_proper_type(&t, r.span);
            if let Some(h) = &hi_ty {
                if !t.is_error() && !self.st.is_sub_type(&t, h) {
                    self.error(
                        r.span,
                        format!(
                            "incompatible type: {} does not conform to {}",
                            self.st.display_type(&t),
                            self.st.display_type(h)
                        ),
                    );
                }
            }
            t
        };
        self.st.get_mut(id).ty = rhs_ty;
        self.st.pop_scope();
        Some(scala_rs_parser::RefineDecl::Type {
            name: name.clone(),
            rhs: Some(Type::TypeMember(id)),
            tparams: tparams.len(),
            lo: lo_ty,
            hi: hi_ty,
        })
    }

    fn is_non_trait_class_type(&self, ty: &Type) -> bool {
        match ty {
            Type::Int
            | Type::Long
            | Type::Float
            | Type::Double
            | Type::Boolean
            | Type::Byte
            | Type::Short
            | Type::Unit
            | Type::Char
            | Type::String
            | Type::AnyVal
            | Type::AnyRef
            | Type::Array(_)
            | Type::ModuleRef(_) => true,
            Type::Class { sym, .. } => {
                let s = self.st.get(*sym);
                !s.flags.contains(Flags::TRAIT) && !s.flags.contains(Flags::INTERFACE)
            }
            Type::Annotated { tpe, .. } => self.is_non_trait_class_type(tpe),
            _ => false,
        }
    }

    fn compound_parents_ok(&mut self, span: Span, ps: &[Type]) -> bool {
        let classes: Vec<&Type> = ps
            .iter()
            .filter(|p| self.is_non_trait_class_type(p))
            .collect();
        if classes.len() <= 1 {
            return true;
        }
        let has_most_specific = classes
            .iter()
            .any(|c| classes.iter().all(|d| self.st.is_sub_type(c, d)));
        if has_most_specific {
            return true;
        }
        self.error(
            span,
            "illegal inheritance: compound type has more than one class parent",
        );
        false
    }

    fn lookup_qualified_type(&mut self, prefix: &Tree, name: &str) -> Option<SymbolId> {
        let owner = self.qualified_type_owner(prefix)?;
        self.complete_binary_member(owner, name, prefix.span);
        self.prefer_class_member(owner, name)
    }

    /// Nested classes live on the module class (`object Outer { class Inner }`).
    fn as_type_owner(&self, id: SymbolId) -> SymbolId {
        match self.st.get(id).kind {
            SymKind::Module => self.st.module_class_of(id),
            _ => id,
        }
    }

    /// `new Outer.Inner()` must bind the class, not `object Inner`.
    fn prefer_class_member(&self, owner: SymbolId, name: &str) -> Option<SymbolId> {
        let found = self.st.lookup_member(owner, name);
        found
            .iter()
            .copied()
            .find(|&s| self.st.get(s).kind == SymKind::Class)
            .or_else(|| {
                found.into_iter().find(|&s| {
                    matches!(
                        self.st.get(s).kind,
                        SymKind::Package
                            | SymKind::Module
                            | SymKind::ModuleClass
                            | SymKind::TypeMember
                            | SymKind::TypeParam
                    )
                })
            })
    }

    fn qualified_type_owner(&mut self, t: &Tree) -> Option<SymbolId> {
        match &t.kind {
            TreeKind::Ident { name } => {
                self.expose_unqualified(name, t.span);
                let id = self.st.lookup(name).into_iter().find(|id| {
                    matches!(
                        self.st.get(*id).kind,
                        SymKind::Package | SymKind::Class | SymKind::Module | SymKind::ModuleClass
                    )
                })?;
                Some(self.as_type_owner(id))
            }
            TreeKind::Select { qual, name } => {
                let owner = self.qualified_type_owner(qual)?;
                self.complete_binary_member(owner, name, t.span);
                self.prefer_class_member(owner, name)
                    .map(|id| self.as_type_owner(id))
            }
            _ => None,
        }
    }

    fn complete_binary_member(&mut self, owner: SymbolId, name: &str, span: Span) {
        if owner.is_none() || name.is_empty() {
            return;
        }
        let owner = self.as_type_owner(owner);
        if self.st.get(owner).kind == SymKind::Class {
            self.ensure_java_loaded(owner, span);
            if !self.st.lookup_member(owner, name).is_empty() {
                return;
            }
        } else if let Some(id) = self.st.lookup_member(owner, name).into_iter().find(|&id| {
            matches!(
                self.st.get(id).kind,
                SymKind::Class | SymKind::Package | SymKind::Module | SymKind::ModuleClass
            )
        }) {
            if self.st.get(id).kind == SymKind::Class {
                self.ensure_java_loaded(id, span);
            }
            return;
        }
        for internal in self.binary_member_candidates(owner, name) {
            if self.load_binary_into(&internal, owner, span, true) {
                return;
            }
        }
        if self.st.get(owner).kind == SymKind::Package {
            // `<root>` is allocated with jvm_name `scala/runtime` (prelude). A
            // top-level Java package like `jprot` lives at `jprot/`, not
            // `scala/runtime/jprot/`.
            let pkg_jvm = if owner == self.st.root {
                String::new()
            } else {
                self.st.get(owner).jvm_name.clone()
            };
            let internal = if pkg_jvm.is_empty() {
                name.to_string()
            } else {
                format!("{pkg_jvm}/{name}")
            };
            let prefix = format!("{internal}/");
            if self.binary.has_package_prefix(&prefix) {
                let _ = crate::classpath::ensure_package(&mut self.st, &internal);
            }
        }
    }

    fn binary_member_candidates(&self, owner: SymbolId, name: &str) -> Vec<String> {
        let owner_bin = if owner == self.st.root {
            String::new()
        } else {
            self.st.get(owner).jvm_name.clone()
        };
        let kind = self.st.get(owner).kind;
        let mut out = Vec::new();
        let push = |out: &mut Vec<String>, s: String| {
            if !s.is_empty() && !out.contains(&s) {
                out.push(s);
            }
        };
        match kind {
            SymKind::Package | SymKind::NoSymbol => {
                let base = if owner_bin.is_empty() {
                    name.to_string()
                } else {
                    format!("{owner_bin}/{name}")
                };
                push(&mut out, base.clone());
                push(&mut out, format!("{base}$"));
            }
            _ => {
                let base = owner_bin.trim_end_matches('$').to_string();
                push(&mut out, format!("{base}${name}"));
                push(&mut out, format!("{base}${name}$"));
                if !owner_bin.is_empty() && owner_bin != base {
                    push(&mut out, format!("{owner_bin}${name}"));
                    push(&mut out, format!("{owner_bin}${name}$"));
                }
            }
        }
        out
    }

    fn load_binary_into(
        &mut self,
        internal: &str,
        owner: SymbolId,
        span: Span,
        with_nested: bool,
    ) -> bool {
        if internal.is_empty() {
            return false;
        }
        if !self.completed_java.insert(internal.to_string()) {
            return crate::classpath::find_by_jvm(&self.st, internal).is_some();
        }
        match self.binary.find_class(internal) {
            Ok(Some(bytes)) => match crate::javaclass::parse_java_classfile(&bytes) {
                Ok(jc) => {
                    let id = crate::classpath::install_java_class_in(&mut self.st, &jc, owner);
                    self.complete_java_parents(id, span);
                    if with_nested {
                        self.complete_scala_nested(id, &jc, span);
                    }
                    true
                }
                Err(e) => {
                    self.error(
                        span,
                        format!("unsupported classfile {}: {e}", internal.replace('/', ".")),
                    );
                    false
                }
            },
            Ok(None) => false,
            Err(e) => {
                self.error(
                    span,
                    format!("unsupported classfile {}: {e}", internal.replace('/', ".")),
                );
                false
            }
        }
    }

    fn complete_scala_nested(
        &mut self,
        class_id: SymbolId,
        jc: &crate::javaclass::JavaClass,
        span: Span,
    ) {
        if !jc.is_scala {
            return;
        }
        let jvm = jc.internal_name.clone();
        if jvm.trim_end_matches('$').contains('$') {
            return;
        }
        let pkg_owner = self.st.get(class_id).owner;
        let companion = self.ensure_scala_companion(class_id, pkg_owner, span);
        let nest_owner = if companion.is_none() {
            class_id
        } else {
            self.st.module_class_of(companion)
        };
        let outer_trait = if matches!(
            self.st.get(class_id).kind,
            SymKind::Module | SymKind::ModuleClass
        ) {
            let stripped = jvm.trim_end_matches('$').to_string();
            crate::classpath::find_by_jvm(&self.st, &stripped).unwrap_or(class_id)
        } else {
            class_id
        };
        for inner in &jc.inner_classes {
            if !inner.inner_jvm.ends_with('$') || inner.inner_jvm.contains("$anon") {
                continue;
            }
            let simple = crate::classpath::java_simple_name(&inner.inner_jvm);
            if simple.is_empty() {
                continue;
            }
            if !self.st.lookup_member(nest_owner, &simple).is_empty() {
                continue;
            }
            if !self.load_binary_into(&inner.inner_jvm, nest_owner, span, false) {
                continue;
            }
            if let Some(ev) = scala_module_evidence_type(outer_trait, &simple) {
                mark_nested_module_implicit(&mut self.st, nest_owner, &simple, ev);
            }
        }
    }

    fn ensure_scala_companion(
        &mut self,
        class_id: SymbolId,
        pkg_owner: SymbolId,
        span: Span,
    ) -> SymbolId {
        match self.st.get(class_id).kind {
            SymKind::Module => return class_id,
            SymKind::ModuleClass => {
                let want = self.st.get(class_id).name.trim_end_matches('$').to_string();
                let members = self.st.get(pkg_owner).members.clone();
                return members
                    .into_iter()
                    .find(|&m| {
                        self.st.get(m).kind == SymKind::Module && self.st.get(m).name == want
                    })
                    .unwrap_or(class_id);
            }
            _ => {}
        }
        if let Some(m) = self.st.companion_module(class_id) {
            return m;
        }
        let jvm = self.st.get(class_id).jvm_name.clone();
        if jvm.is_empty() || jvm.ends_with('$') {
            return SymbolId::NONE;
        }
        let comp = format!("{jvm}$");
        self.load_binary_into(&comp, pkg_owner, span, false);
        self.st.companion_module(class_id).unwrap_or(SymbolId::NONE)
    }

    fn complete_java_type(&mut self, ty: &Type, span: Span) {
        match ty {
            Type::Class { sym, args } => {
                self.ensure_java_loaded(*sym, span);
                for a in args {
                    self.complete_java_type(a, span);
                }
            }
            Type::BoundedWildcard { lo, hi } => {
                if let Some(t) = lo {
                    self.complete_java_type(t, span);
                }
                if let Some(t) = hi {
                    self.complete_java_type(t, span);
                }
            }
            Type::Array(t)
            | Type::Repeated(t)
            | Type::ByName(t)
            | Type::Annotated { tpe: t, .. } => {
                self.complete_java_type(t, span);
            }
            _ => {}
        }
    }

    fn complete_java_parents(&mut self, class_id: SymbolId, span: Span) {
        let parents = self.st.get(class_id).parents.clone();
        for p in &parents {
            if let Some(s) = self.st.class_sym_of(p) {
                self.ensure_java_loaded(s, span);
            }
        }
    }

    pub(crate) fn ensure_java_loaded(&mut self, class_id: SymbolId, span: Span) {
        if class_id.is_none() {
            return;
        }
        let jvm = self.st.get(class_id).jvm_name.clone();
        if jvm.is_empty() || jvm.starts_with('[') {
            return;
        }
        let javaish = self.st.get(class_id).flags.contains(Flags::JAVA)
            || jvm.starts_with("java/")
            || jvm.starts_with("javax/");
        if !javaish {
            return;
        }
        if !self.completed_java.insert(jvm.clone()) {
            return;
        }
        match self.binary.find_class(&jvm) {
            Ok(Some(bytes)) => match crate::javaclass::parse_java_classfile(&bytes) {
                Ok(jc) => {
                    crate::classpath::install_java_class(&mut self.st, &jc);
                    self.complete_java_parents(class_id, span);
                }
                Err(e) => {
                    self.error(
                        span,
                        format!("unsupported classfile {}: {e}", jvm.replace('/', ".")),
                    );
                }
            },
            Ok(None) => {}
            Err(e) => {
                self.error(
                    span,
                    format!("unsupported classfile {}: {e}", jvm.replace('/', ".")),
                );
            }
        }
    }

    fn resolve_type_name(&self, name: &str, args: &[Type]) -> Type {
        match name {
            "Int" => Type::Int,
            "Long" => Type::Long,
            "Double" => Type::Double,
            "Float" => Type::Float,
            "Boolean" => Type::Boolean,
            "Byte" => Type::Byte,
            "Short" => Type::Short,
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
                // Prefer the class of a case-class/companion pair (`Point` vs `Point$`).
                let id = found
                    .iter()
                    .copied()
                    .find(|s| matches!(self.st.get(*s).kind, SymKind::Class))
                    .or_else(|| {
                        found.into_iter().find(|s| {
                            matches!(
                                self.st.get(*s).kind,
                                SymKind::ModuleClass
                                    | SymKind::Module
                                    | SymKind::TypeParam
                                    | SymKind::TypeMember
                            )
                        })
                    });
                if let Some(id) = id {
                    match self.st.get(id).kind {
                        SymKind::Module | SymKind::ModuleClass => Type::ModuleRef(id),
                        SymKind::TypeParam => Type::TypeParam(id),
                        SymKind::TypeMember => self.st.type_member_as_seen(id),
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
        self.complete_java_type(&tree.ty, tree.span);
        self.complete_java_type(pt, tree.span);
        if self.st.is_sub_type(&tree.ty, pt) {
            // `b.x` with `{ type A <: Int }` stays a TypeMember; pin it to the
            // expected primitive so erasure inserts the same unbox as `Bar#A`.
            if matches!(&tree.ty, Type::TypeMember(_))
                && matches!(
                    pt,
                    Type::Int
                        | Type::Long
                        | Type::Float
                        | Type::Double
                        | Type::Boolean
                        | Type::Byte
                        | Type::Short
                        | Type::Char
                        | Type::Unit
                )
            {
                tree.ty = pt.clone();
            }
            return;
        }
        if self.adapt_singleton(tree, pt) {
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
                let ret = if inner_tree.ty.is_no_type() || inner_tree.ty.is_error() {
                    (**inner).clone()
                } else {
                    inner_tree.ty.clone()
                };
                *tree = Tree {
                    id: inner_tree.id,
                    span,
                    kind: TreeKind::Function {
                        vparams: vec![],
                        body: Box::new(inner_tree),
                    },
                    ty: Type::Function {
                        params: vec![],
                        ret: Box::new(ret),
                    },
                    sym: SymbolId::NONE,
                    postfix: false,
                };
            }
            return;
        }
        if let Type::Method { paramss, ret } = &tree.ty {
            if is_function_pt(pt) || self.st.sam_sig(pt).is_some() {
                let params: Vec<Type> = paramss.iter().flatten().cloned().collect();
                let ret = (**ret).clone();
                eta_expand(&mut self.st, &mut self.gensym, tree, params, ret);
                if self.st.is_sub_type(&tree.ty, pt) {
                    return;
                }
            }
        }
        if self.adapt_to_sam(tree, pt) {
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
                    postfix: false,
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

    fn adapt_to_sam(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let Some(sam) = self.st.sam_sig(pt) else {
            return false;
        };
        let Type::Function { params, ret } = &tree.ty else {
            return false;
        };
        if params.len() != sam.param_tys.len() {
            return false;
        }
        if !ret.is_no_type() && !self.st.is_sub_type(ret, &sam.ret_ty) {
            return false;
        }
        for (have, want) in params.iter().zip(sam.param_tys.iter()) {
            if have.is_no_type() {
                continue;
            }
            if !self.st.is_sub_type(want, have) && !self.st.is_sub_type(have, want) {
                return false;
            }
        }
        tree.ty = pt.clone();
        true
    }

    fn adapt_singleton(&self, tree: &mut Tree, pt: &Type) -> bool {
        match pt {
            Type::ThisType(cls) => {
                if !matches!(&tree.kind, TreeKind::This { .. }) {
                    return false;
                }
                let ok = tree.sym == *cls
                    || matches!(
                        &tree.ty,
                        Type::Class { sym, .. } | Type::ModuleRef(sym) if *sym == *cls
                    );
                if ok {
                    tree.ty = pt.clone();
                }
                ok
            }
            Type::SingleType { sym, .. } => {
                if tree.sym == *sym {
                    tree.ty = pt.clone();
                    true
                } else {
                    false
                }
            }
            Type::Annotated { tpe, .. } => {
                if self.st.is_sub_type(&tree.ty, tpe) || self.adapt_singleton(tree, tpe) {
                    tree.ty = pt.clone();
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn desugar_custom_interpolator(&mut self, tree: &mut Tree) {
        let TreeKind::InterpolatedString {
            prefix,
            parts,
            args,
        } = &tree.kind
        else {
            return;
        };
        let span = tree.span;
        let prefix = prefix.clone();
        let parts = parts.clone();
        let args = args.clone();
        let sc = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: "StringContext".into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        let apply = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(sc),
                name: "apply".into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        let part_lits: Vec<Tree> = parts
            .into_iter()
            .map(|p| Tree {
                id: NodeId(0),
                span,
                kind: TreeKind::Literal {
                    lit: Lit::String(p),
                },
                ty: Type::String,
                sym: SymbolId::NONE,
                postfix: false,
            })
            .collect();
        let sc_apply = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(apply),
                args: part_lits,
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        let sel = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(sc_apply),
                name: prefix,
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        *tree = Tree {
            id: tree.id,
            span,
            kind: TreeKind::Apply {
                fun: Box::new(sel),
                args,
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
    }

    fn rewrite_generic_array_new(&mut self, tree: &mut Tree, elem: Type) {
        let span = tree.span;
        let args = match &mut tree.kind {
            TreeKind::Apply { args, .. } => std::mem::take(args),
            _ => Vec::new(),
        };
        let Some(ct_cls) = self
            .st
            .lookup("ClassTag")
            .into_iter()
            .find(|s| self.st.get(*s).kind == SymKind::Class)
        else {
            self.error(
                span,
                "unimplemented: generic Array construction without ClassTag",
            );
            tree.ty = Type::Error;
            return;
        };
        let ct_ty = Type::Class {
            sym: ct_cls,
            args: vec![elem.clone()],
        };
        match self.search_implicit(&ct_ty) {
            ImplicitSearch::Found(id) => {
                let mut recv = self.ref_implicit(id, span);
                self.adapt(&mut recv, &ct_ty);
                let sel = Tree {
                    id: NodeId(0),
                    span,
                    kind: TreeKind::Select {
                        qual: Box::new(recv),
                        name: "newArray".into(),
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                };
                *tree = Tree {
                    id: tree.id,
                    span,
                    kind: TreeKind::Apply {
                        fun: Box::new(sel),
                        args,
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                };
                self.type_expr_inner(tree, &Type::NoType);
            }
            ImplicitSearch::None => {
                self.error(span, self.missing_implicit_message(&ct_ty));
                tree.ty = Type::Error;
            }
            ImplicitSearch::Ambiguous(ids) => {
                self.error(
                    span,
                    format!("ambiguous implicit: {}", self.describe_implicits(&ids)),
                );
                tree.ty = Type::Error;
            }
        }
    }

    fn rewrite_receiver_apply(&mut self, fun: &mut Tree) {
        if matches!(&fun.kind, TreeKind::Select { .. } | TreeKind::New { .. }) {
            return;
        }
        // nsc inserts `.apply` for `c(1)` / `xs(i)` when `c` is a value, not a method.
        // Leave method/overload idents alone (`f(1)`).
        let insert = matches!(
            &fun.ty,
            Type::Array(_) | Type::Class { .. } | Type::ModuleRef(_)
        );
        if !insert {
            return;
        }
        let span = fun.span;
        let id = fun.id;
        let qual = std::mem::replace(fun, Tree::dummy(TreeKind::Empty));
        *fun = Tree {
            id,
            span,
            kind: TreeKind::Select {
                qual: Box::new(qual),
                name: "apply".into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        self.type_select(
            fun,
            &Type::Method {
                paramss: vec![],
                ret: Box::new(Type::NoType),
            },
        );
    }

    fn check_stored_annotations(&mut self, tree: &Tree) {
        let mods = match &tree.kind {
            TreeKind::DefDef { mods, .. }
            | TreeKind::ValDef { mods, .. }
            | TreeKind::ClassDef { mods, .. }
            | TreeKind::ModuleDef { mods, .. }
            | TreeKind::TypeDef { mods, .. } => mods,
            _ => return,
        };
        for a in &mods.annotations {
            let path = a.annotation_path();
            if is_tailrec_annot(&path) {
                if !matches!(&tree.kind, TreeKind::DefDef { .. }) {
                    self.error(
                        tree.span,
                        "could not optimize @tailrec annotated method: not a method",
                    );
                } else {
                    self.check_tailrec(tree);
                }
            }
            if is_override_annot(&path) {
                if !matches!(&tree.kind, TreeKind::DefDef { .. }) {
                    self.error(
                        tree.span,
                        format!(
                            "method {} overrides nothing",
                            tree.name().unwrap_or("<annot>")
                        ),
                    );
                } else {
                    self.check_java_override(tree);
                }
            }
            if is_inline_annot(&path) || is_noinline_annot(&path) {
                if !matches!(&tree.kind, TreeKind::DefDef { .. }) {
                    let simple = path.rsplit('.').next().unwrap_or(path.as_str());
                    self.error(tree.span, format!("@{simple} is only supported on methods"));
                }
            }
            if is_native_annot(&path) {
                if !matches!(&tree.kind, TreeKind::DefDef { .. }) {
                    self.error(tree.span, "@native is only supported on methods");
                } else if let TreeKind::DefDef { rhs, .. } = &tree.kind {
                    if !rhs.is_empty() {
                        self.error(tree.span, "native method cannot have a body");
                    }
                }
            }
        }
        let has_inline = mods
            .annotations
            .iter()
            .any(|a| is_inline_annot(&a.annotation_path()));
        let has_noinline = mods
            .annotations
            .iter()
            .any(|a| is_noinline_annot(&a.annotation_path()));
        if has_inline && has_noinline {
            self.error(tree.span, "@inline and @noinline cannot be used together");
        }
    }

    fn check_java_override(&mut self, tree: &Tree) {
        let name = tree.name().unwrap_or("").to_string();
        if name.is_empty() || name == "<init>" {
            self.error(tree.span, "method <init> overrides nothing");
            return;
        }
        if tree.sym.is_none() || !self.method_overrides_parent(tree.sym) {
            self.error(tree.span, format!("method {name} overrides nothing"));
        }
    }

    fn method_overrides_parent(&self, meth: SymbolId) -> bool {
        if meth.is_none() {
            return false;
        }
        let s = self.st.get(meth);
        let name = s.name.clone();
        let owner = s.owner;
        if owner.is_none() {
            return false;
        }
        let my_ps = method_value_params(&s.ty);
        let mut seen = std::collections::HashSet::new();
        let mut work: Vec<SymbolId> = Vec::new();
        for p in &self.st.get(owner).parents {
            if let Some(c) = self.st.class_sym_of(p) {
                work.push(c);
            }
        }
        work.push(self.st.anyref_sym);
        work.push(self.st.any_sym);
        while let Some(id) = work.pop() {
            if id.is_none() || id == owner || !seen.insert(id.0) {
                continue;
            }
            for m in self.st.get(id).members.clone() {
                if m == meth {
                    continue;
                }
                let cand = self.st.get(m);
                if cand.name != name {
                    continue;
                }
                if !matches!(cand.kind, SymKind::Method | SymKind::Term) {
                    continue;
                }
                let ps = method_value_params(&cand.ty);
                if ps.len() != my_ps.len() {
                    continue;
                }
                let ok = my_ps
                    .iter()
                    .zip(ps.iter())
                    .all(|(a, b)| a == b || self.st.is_sub_type(a, b) || self.st.is_sub_type(b, a));
                if ok {
                    return true;
                }
            }
            for p in self.st.get(id).parents.clone() {
                if let Some(c) = self.st.class_sym_of(&p) {
                    work.push(c);
                }
            }
        }
        false
    }

    fn check_tailrec(&mut self, tree: &Tree) {
        let rhs = match &tree.kind {
            TreeKind::DefDef { rhs, .. } => rhs.as_ref(),
            _ => return,
        };
        if !self.tailrec_effectively_final(tree.sym) {
            self.error(
                tree.span,
                "could not optimize @tailrec annotated method: it is neither private nor final so can be overridden",
            );
            return;
        }
        if rhs.is_empty() {
            self.error(
                tree.span,
                "could not optimize @tailrec annotated method: it contains no recursive calls",
            );
            return;
        }
        let mut tail = 0;
        let mut nontail = 0;
        count_tailrec_calls(rhs, tree.sym, true, &mut tail, &mut nontail);
        if nontail > 0 {
            self.error(
                tree.span,
                "could not optimize @tailrec annotated method: it contains a recursive call not in tail position",
            );
        } else if tail == 0 {
            self.error(
                tree.span,
                "could not optimize @tailrec annotated method: it contains no recursive calls",
            );
        }
    }

    fn tailrec_effectively_final(&self, meth: SymbolId) -> bool {
        if meth.is_none() {
            return true;
        }
        let s = self.st.get(meth);
        if s.flags.contains(Flags::FINAL)
            || s.flags.contains(Flags::PRIVATE)
            || s.flags.contains(Flags::LOCAL)
        {
            return true;
        }
        let owner = s.owner;
        if owner.is_none() {
            return true;
        }
        let o = self.st.get(owner);
        matches!(o.kind, SymKind::Module | SymKind::ModuleClass)
            || o.flags.contains(Flags::MODULE)
            || o.flags.contains(Flags::FINAL)
    }

    fn missing_implicit_message(&self, ty: &Type) -> String {
        if let Some(msg) = self.implicit_not_found_msg(ty) {
            return msg;
        }
        format!(
            "no implicit: could not find implicit value of type {}",
            self.st.display_type(ty)
        )
    }

    fn implicit_not_found_msg(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::Annotated { tpe, .. } => self.implicit_not_found_msg(tpe),
            Type::Class { sym, args } => self.implicit_not_found_on(*sym, args),
            Type::Named { name, args } => {
                let id = self.st.lookup(name).into_iter().find(|s| {
                    matches!(
                        self.st.get(*s).kind,
                        crate::symbol::SymKind::Class | crate::symbol::SymKind::TypeMember
                    )
                })?;
                self.implicit_not_found_on(id, args)
            }
            _ => None,
        }
    }

    fn implicit_not_found_on(&self, sym: SymbolId, args: &[Type]) -> Option<String> {
        let annots = self.st.get(sym).annotations.clone();
        let tps = self.st.get(sym).tparams.clone();
        for a in &annots {
            let path = a.annotation_path();
            let simple = path.rsplit('.').next().unwrap_or(path.as_str());
            if simple != "implicitNotFound" {
                continue;
            }
            let mut msg = annot_first_string(a)?;
            for (i, tp) in tps.iter().enumerate() {
                let n = self.st.get(*tp).name.clone();
                let shown = args
                    .get(i)
                    .map(|t| self.st.display_type(t))
                    .unwrap_or_else(|| n.clone());
                msg = msg.replace(&format!("${{{n}}}"), &shown);
            }
            return Some(msg);
        }
        None
    }
}

fn is_tuple2_elem_map(name: &str) -> bool {
    matches!(name, "Map" | "HashMap" | "LinkedHashMap")
}

fn is_tailrec_annot(path: &str) -> bool {
    matches!(
        path,
        "tailrec" | "annotation.tailrec" | "scala.annotation.tailrec"
    )
}

fn is_inline_annot(path: &str) -> bool {
    matches!(path, "inline" | "scala.inline")
}

fn is_noinline_annot(path: &str) -> bool {
    matches!(path, "noinline" | "scala.noinline")
}

fn is_native_annot(path: &str) -> bool {
    matches!(path, "native" | "scala.native")
}

fn is_override_annot(path: &str) -> bool {
    matches!(path, "Override" | "java.lang.Override")
}

fn peel_empty_annot(ty: &Type) -> Type {
    match ty {
        Type::Annotated { tpe, .. } if tpe.is_no_type() => Type::NoType,
        Type::Annotated { tpe, .. } => peel_empty_annot(tpe),
        other => other.clone(),
    }
}

fn fill_empty_annot(ascr: Type, found: &Type) -> Type {
    match ascr {
        Type::Annotated { tpe, annot } if tpe.is_no_type() => Type::Annotated {
            tpe: Box::new(found.clone()),
            annot,
        },
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(fill_empty_annot(*tpe, found)),
            annot,
        },
        other => other,
    }
}

fn tree_has_switch(t: &Tree) -> bool {
    fn ty_has_switch(ty: &Type) -> bool {
        match ty {
            Type::Annotated { annot, tpe } => {
                annot.rsplit('.').next() == Some("switch") || ty_has_switch(tpe)
            }
            _ => false,
        }
    }
    match &t.kind {
        TreeKind::Typed { tpt, expr } => annot_tree_is_switch(tpt) || tree_has_switch(expr),
        _ => ty_has_switch(&t.ty),
    }
}

fn annot_tree_is_switch(tpt: &Tree) -> bool {
    match &tpt.kind {
        TreeKind::AnnotatedTypeTree { annot, tpt } => {
            let path = annot.annotation_path();
            let simple = path.rsplit('.').next().unwrap_or(path.as_str());
            simple == "switch" || annot_tree_is_switch(tpt)
        }
        _ => false,
    }
}

fn match_can_switch(sel_ty: &Type, cases: &[scala_rs_parser::CaseDef]) -> bool {
    switch_case_keys(sel_ty, cases).is_some()
}

fn switch_case_keys(
    sel_ty: &Type,
    cases: &[scala_rs_parser::CaseDef],
) -> Option<Vec<(i32, usize)>> {
    let core = peel_type_annot(sel_ty);
    if !matches!(core, Type::Int | Type::Char) {
        return None;
    }
    let mut keys = Vec::new();
    let mut default = false;
    for (i, c) in cases.iter().enumerate() {
        if !c.guard.is_empty() {
            return None;
        }
        match switch_pat_key(&c.pat) {
            Some(SwitchPat::Key(k)) => keys.push((k, i)),
            Some(SwitchPat::Default) => {
                if default {
                    return None;
                }
                default = true;
            }
            None => return None,
        }
    }
    if keys.is_empty() {
        return None;
    }
    Some(keys)
}

fn peel_type_annot(ty: &Type) -> &Type {
    match ty {
        Type::Annotated { tpe, .. } => peel_type_annot(tpe),
        t => t,
    }
}

enum SwitchPat {
    Key(i32),
    Default,
}

fn switch_pat_key(pat: &Tree) -> Option<SwitchPat> {
    match &pat.kind {
        TreeKind::Literal { lit: Lit::Int(n) } => Some(SwitchPat::Key(*n)),
        TreeKind::Literal { lit: Lit::Char(c) } => Some(SwitchPat::Key(*c as i32)),
        TreeKind::Wildcard | TreeKind::Empty => Some(SwitchPat::Default),
        TreeKind::Ident { name } => {
            let is_varid = name
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_');
            if is_varid {
                Some(SwitchPat::Default)
            } else {
                None
            }
        }
        TreeKind::Bind { body, .. } => switch_pat_key(body),
        TreeKind::Typed { expr, .. } => switch_pat_key(expr),
        _ => None,
    }
}

fn annot_first_string(tree: &Tree) -> Option<String> {
    match &tree.kind {
        TreeKind::Apply { args, .. } => args.iter().find_map(annot_first_string),
        TreeKind::Assign { rhs, .. } => annot_first_string(rhs),
        TreeKind::Literal {
            lit: Lit::String(s),
        } => Some(s.clone()),
        _ => None,
    }
}

fn method_value_params(ty: &Type) -> Vec<Type> {
    match ty {
        Type::Method { paramss, .. } => paramss.iter().flatten().cloned().collect(),
        Type::Function { params, .. } => params.clone(),
        _ => Vec::new(),
    }
}

fn is_rec_apply(tree: &Tree, meth: SymbolId) -> bool {
    match &tree.kind {
        TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => {
            rec_fun_is_method(fun, meth)
        }
        _ => false,
    }
}

fn rec_fun_is_method(tree: &Tree, meth: SymbolId) -> bool {
    if meth.is_none() {
        return false;
    }
    match &tree.kind {
        TreeKind::TypeApply { fun, .. } => rec_fun_is_method(fun, meth),
        TreeKind::Ident { .. } => tree.sym == meth,
        TreeKind::Select { .. } => tree.sym == meth,
        _ => tree.sym == meth,
    }
}

fn count_tailrec_calls(
    tree: &Tree,
    meth: SymbolId,
    tail: bool,
    n_tail: &mut u32,
    n_nontail: &mut u32,
) {
    if is_rec_apply(tree, meth) {
        if tail {
            *n_tail += 1;
        } else {
            *n_nontail += 1;
        }
        match &tree.kind {
            TreeKind::Apply { args, .. } => {
                for a in args {
                    count_tailrec_calls(a, meth, false, n_tail, n_nontail);
                }
            }
            TreeKind::TypeApply { fun, .. } => {
                if let TreeKind::Apply { args, .. } = &fun.kind {
                    for a in args {
                        count_tailrec_calls(a, meth, false, n_tail, n_nontail);
                    }
                }
            }
            _ => {}
        }
        return;
    }
    match &tree.kind {
        TreeKind::If { cond, thenp, elsep } => {
            count_tailrec_calls(cond, meth, false, n_tail, n_nontail);
            count_tailrec_calls(thenp, meth, tail, n_tail, n_nontail);
            count_tailrec_calls(elsep, meth, tail, n_tail, n_nontail);
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                count_tailrec_calls(s, meth, false, n_tail, n_nontail);
            }
            count_tailrec_calls(expr, meth, tail, n_tail, n_nontail);
        }
        TreeKind::Match { selector, cases } => {
            count_tailrec_calls(selector, meth, false, n_tail, n_nontail);
            for c in cases {
                if !c.guard.is_empty() {
                    count_tailrec_calls(&c.guard, meth, false, n_tail, n_nontail);
                }
                count_tailrec_calls(&c.body, meth, tail, n_tail, n_nontail);
            }
        }
        TreeKind::Apply { fun, args } => {
            count_tailrec_calls(fun, meth, false, n_tail, n_nontail);
            for a in args {
                count_tailrec_calls(a, meth, false, n_tail, n_nontail);
            }
        }
        TreeKind::TypeApply { fun, args } => {
            count_tailrec_calls(fun, meth, tail, n_tail, n_nontail);
            let _ = args;
        }
        TreeKind::Select { qual, .. } => count_tailrec_calls(qual, meth, false, n_tail, n_nontail),
        TreeKind::Typed { expr, .. } => count_tailrec_calls(expr, meth, tail, n_tail, n_nontail),
        TreeKind::Assign { lhs, rhs } => {
            count_tailrec_calls(lhs, meth, false, n_tail, n_nontail);
            count_tailrec_calls(rhs, meth, false, n_tail, n_nontail);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            count_tailrec_calls(cond, meth, false, n_tail, n_nontail);
            count_tailrec_calls(body, meth, false, n_tail, n_nontail);
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            count_tailrec_calls(block, meth, false, n_tail, n_nontail);
            for c in catches {
                count_tailrec_calls(&c.body, meth, false, n_tail, n_nontail);
            }
            if !finalizer.is_empty() {
                count_tailrec_calls(finalizer, meth, false, n_tail, n_nontail);
            }
        }
        TreeKind::Function { body, .. } => {
            count_tailrec_calls(body, meth, false, n_tail, n_nontail);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => {
            count_tailrec_calls(expr, meth, false, n_tail, n_nontail);
        }
        _ => {}
    }
}

fn f_kind_name(kind: scala_rs_parser::finterp::FConvKind) -> &'static str {
    use scala_rs_parser::finterp::FConvKind;
    match kind {
        FConvKind::Integral => "integral type",
        FConvKind::Floating => "floating type",
        FConvKind::Character => "character/integral type",
        FConvKind::General => "Any",
        FConvKind::Unsupported => "a supported conversion",
    }
}

fn is_inferable_param_pt(pt: &Type) -> bool {
    !matches!(
        pt,
        Type::NoType | Type::Error | Type::Any | Type::AnyRef | Type::AnyVal | Type::Overload(_)
    )
}

fn is_function_pt(pt: &Type) -> bool {
    match pt {
        Type::Function { .. } => true,
        Type::Named { name, .. }
            if (name.starts_with("Function") && name != "Function")
                || name == "PartialFunction" =>
        {
            true
        }
        _ => false,
    }
}

fn is_partial_function_sym(st: &SymbolTable, id: SymbolId) -> bool {
    if id.is_none() {
        return false;
    }
    let s = st.get(id);
    s.name == "PartialFunction"
        && (s.jvm_name == "scala/PartialFunction" || s.jvm_name.ends_with("PartialFunction"))
}

fn partial_function_type(st: &SymbolTable, pt: &Type) -> Option<(Type, Type)> {
    match pt {
        Type::Named { name, args } if name == "PartialFunction" && args.len() == 2 => {
            Some((args[0].clone(), args[1].clone()))
        }
        Type::Class { sym, args } if is_partial_function_sym(st, *sym) => {
            if args.len() >= 2 {
                Some((args[0].clone(), args[1].clone()))
            } else {
                Some((Type::Any, Type::Any))
            }
        }
        _ => None,
    }
}

fn unwrap_fn0_or_byname(ty: &Type) -> Type {
    match ty {
        Type::ByName(t) => (**t).clone(),
        Type::Function { params, ret } if params.is_empty() => (**ret).clone(),
        other => other.clone(),
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

fn tree_contains_this(tree: &Tree) -> bool {
    match &tree.kind {
        TreeKind::This { .. } => true,
        TreeKind::Select { qual, .. } | TreeKind::Typed { expr: qual, .. } => {
            tree_contains_this(qual)
        }
        TreeKind::Apply { fun, args } | TreeKind::TypeApply { fun, args } => {
            tree_contains_this(fun) || args.iter().any(tree_contains_this)
        }
        TreeKind::Block { stats, expr } => {
            stats.iter().any(tree_contains_this) || tree_contains_this(expr)
        }
        TreeKind::If { cond, thenp, elsep } => {
            tree_contains_this(cond) || tree_contains_this(thenp) || tree_contains_this(elsep)
        }
        TreeKind::Assign { lhs, rhs } => tree_contains_this(lhs) || tree_contains_this(rhs),
        TreeKind::New { tpt } => tree_contains_this(tpt),
        _ => false,
    }
}

/// Overload applicability: `Tuple2[Any, Any]` matches `Tuple2[K, V]` when `K`/`V`
/// are type parameters. Not used for implicit search (`is_sub_type`).
fn class_ctor_matches_typeparam_args(arg: &Type, param: &Type) -> bool {
    match (arg, param) {
        (Type::Class { sym: sa, args: aa }, Type::Class { sym: sp, args: pa })
            if sa == sp && aa.len() == pa.len() =>
        {
            aa.iter().zip(pa.iter()).all(|(a, p)| {
                matches!(p, Type::TypeParam(_)) || a == p || class_ctor_matches_typeparam_args(a, p)
            })
        }
        (Type::Tuple(aa), Type::Class { args: pa, .. }) if aa.len() == pa.len() => {
            aa.iter().zip(pa.iter()).all(|(a, p)| {
                matches!(p, Type::TypeParam(_)) || a == p || class_ctor_matches_typeparam_args(a, p)
            })
        }
        (Type::Class { args: aa, .. }, Type::Tuple(pa)) if aa.len() == pa.len() => {
            aa.iter().zip(pa.iter()).all(|(a, p)| {
                matches!(p, Type::TypeParam(_)) || a == p || class_ctor_matches_typeparam_args(a, p)
            })
        }
        (_, Type::TypeParam(_)) => true,
        (a, Type::BoundedWildcard { hi: Some(h), .. }) => class_ctor_matches_typeparam_args(a, h),
        (
            a,
            Type::BoundedWildcard {
                lo: Some(l),
                hi: None,
            },
        ) => class_ctor_matches_typeparam_args(a, l),
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
            | Type::Byte
            | Type::Short
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
        (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, args: a2 }) if s1 == s2 => {
            if a1.is_empty() || a2.is_empty() {
                true
            } else if a1.len() == a2.len() {
                a1.iter().zip(a2.iter()).all(|(x, y)| is_sub_type(x, y))
            } else {
                false
            }
        }
        (Type::Wildcard, Type::AnyRef | Type::AnyVal | Type::Wildcard) => true,
        (_, Type::Wildcard) => true,
        (Type::Array(x), Type::Array(y)) => is_sub_type(x, y),
        (Type::ModuleRef(s), Type::Class { sym, .. }) if s == sym => true,
        _ => false,
    }
}

fn numeric_widen(a: &Type, b: &Type) -> Option<Type> {
    let a = a.widen_constant();
    let b = b.widen_constant();
    match (&a, &b) {
        (Type::Int, Type::Long) => Some(Type::Long),
        (Type::Int, Type::Double) => Some(Type::Double),
        (Type::Long, Type::Double) => Some(Type::Double),
        (Type::Float, Type::Double) => Some(Type::Double),
        (Type::Int, Type::Float) => Some(Type::Float),
        _ => None,
    }
}

fn lub(a: &Type, b: &Type) -> Type {
    if a == b {
        return a.clone();
    }
    let a = a.widen_constant();
    let b = b.widen_constant();
    if a == b {
        return a;
    }
    if is_sub_type(&a, &b) {
        return b;
    }
    if is_sub_type(&b, &a) {
        return a;
    }
    if matches!(a, Type::Nothing) {
        return b;
    }
    if matches!(b, Type::Nothing) {
        return a;
    }
    Type::Any
}

fn import_path(t: &Tree) -> String {
    match &t.kind {
        TreeKind::Ident { name } => name.clone(),
        TreeKind::Select { qual, name } => {
            let p = import_path(qual);
            if p.is_empty() {
                name.clone()
            } else {
                format!("{p}.{name}")
            }
        }
        _ => String::new(),
    }
}

fn import_enables_feature(expr: &Tree, feature: &str) -> bool {
    let p = import_path(expr);
    if p == format!("scala.language.{feature}")
        || p == format!("language.{feature}")
        || p.ends_with(&format!(".language.{feature}"))
        || p == "scala.language._"
        || p == "language._"
        || p.ends_with(".language._")
    {
        return true;
    }
    if let TreeKind::Select { qual, name } = &expr.kind {
        if name.starts_with('{') {
            let qp = import_path(qual);
            let is_lang = qp == "scala.language" || qp == "language" || qp.ends_with(".language");
            if is_lang && (name.contains(feature) || name.contains('_')) {
                return true;
            }
        }
    }
    false
}

fn language_flag_enabled(features: &[String], name: &str) -> bool {
    features.iter().any(|f| f == name || f == "_")
}

fn has_named_dynamic_args(tree: &Tree) -> bool {
    match &tree.kind {
        TreeKind::Apply { args, .. } => args.iter().any(|a| Typer::named_arg_parts(a).is_some()),
        _ => false,
    }
}

/// nsc `isAssignmentOp`: ends with `=`, length > 1, not `==` / `!=` / `<=` / `>=`.
fn is_assignment_op(name: &str) -> bool {
    name.len() > 1 && name.ends_with('=') && !matches!(name, "==" | "!=" | "<=" | ">=")
}

fn is_implicit_conversion_shape(vparamss: &[Vec<Tree>]) -> bool {
    let mut n_non_impl = 0usize;
    for clause in vparamss {
        let all_impl = !clause.is_empty()
            && clause.iter().all(|p| match &p.kind {
                TreeKind::ValDef { mods, .. } => mods.flags.contains(Flags::IMPLICIT),
                _ => false,
            });
        if all_impl {
            continue;
        }
        n_non_impl += clause.len();
    }
    n_non_impl == 1
}

/// nsc: `T: C` means implicit evidence of type `C[T]`.
fn is_this_or_super_callee(fun: &Tree) -> bool {
    match &fun.kind {
        TreeKind::This { .. } | TreeKind::Super { .. } => true,
        TreeKind::Ident { name } if name == "this" || name == "super" => true,
        _ => false,
    }
}

fn is_ctor_delegation_apply(t: &Tree) -> Option<bool> {
    match &t.kind {
        TreeKind::Apply { fun, .. } => {
            if matches!(&fun.kind, TreeKind::Super { .. })
                || matches!(&fun.kind, TreeKind::Ident { name } if name == "super")
            {
                Some(true)
            } else if matches!(&fun.kind, TreeKind::This { .. })
                || matches!(&fun.kind, TreeKind::Ident { name } if name == "this")
            {
                Some(false)
            } else {
                None
            }
        }
        TreeKind::Typed { expr, .. } => is_ctor_delegation_apply(expr),
        _ => None,
    }
}

fn tree_has_ctor_delegation(t: &Tree) -> bool {
    if is_ctor_delegation_apply(t).is_some() {
        return true;
    }
    match &t.kind {
        TreeKind::Block { stats, expr } => {
            stats.iter().any(tree_has_ctor_delegation) || tree_has_ctor_delegation(expr)
        }
        TreeKind::Typed { expr, .. } => tree_has_ctor_delegation(expr),
        _ => false,
    }
}

fn first_ctor_delegation(rhs: &Tree) -> CtorDelegation {
    match &rhs.kind {
        TreeKind::Typed { expr, .. } => first_ctor_delegation(expr),
        TreeKind::Block { stats, expr } => {
            let first = stats.first().unwrap_or(expr);
            match is_ctor_delegation_apply(first) {
                Some(true) => CtorDelegation::Super,
                Some(false) => CtorDelegation::This,
                None => {
                    if tree_has_ctor_delegation(rhs) {
                        CtorDelegation::AfterStats
                    } else {
                        CtorDelegation::Missing
                    }
                }
            }
        }
        _ => match is_ctor_delegation_apply(rhs) {
            Some(true) => CtorDelegation::Super,
            Some(false) => CtorDelegation::This,
            None => {
                if tree_has_ctor_delegation(rhs) {
                    CtorDelegation::AfterStats
                } else {
                    CtorDelegation::Missing
                }
            }
        },
    }
}

fn scala_module_evidence_type(outer: SymbolId, simple: &str) -> Option<Type> {
    if outer.is_none() {
        return None;
    }
    let arg = match simple {
        "Int" => Type::Int,
        "Long" => Type::Long,
        "Double" => Type::Double,
        "Float" => Type::Float,
        "Boolean" => Type::Boolean,
        "Byte" => Type::Byte,
        "Short" => Type::Short,
        "Char" => Type::Char,
        "Unit" => Type::Unit,
        "String" => Type::String,
        _ => return None,
    };
    Some(Type::Class {
        sym: outer,
        args: vec![arg],
    })
}

fn mark_nested_module_implicit(st: &mut SymbolTable, owner: SymbolId, simple: &str, ev: Type) {
    let Some(m) = st
        .lookup_member(owner, simple)
        .into_iter()
        .find(|&s| st.get(s).kind == SymKind::Module)
    else {
        return;
    };
    let f = st.get(m).flags.with(Flags::IMPLICIT);
    st.get_mut(m).flags = f;
    let cls = st.module_class_of(m);
    st.get_mut(cls).parents = vec![ev];
}

fn apply_context_bound(bound: Type, tp: SymbolId) -> Type {
    match bound {
        Type::Class { sym, args } if args.is_empty() => Type::Class {
            sym,
            args: vec![Type::TypeParam(tp)],
        },
        Type::Named { name, args } if args.is_empty() => Type::Named {
            name,
            args: vec![Type::TypeParam(tp)],
        },
        other => other,
    }
}

fn array_elem_of(ty: &Type) -> Option<Type> {
    match ty {
        Type::Array(t) => Some((**t).clone()),
        Type::Named { name, args } if name == "Array" && !args.is_empty() => Some(args[0].clone()),
        Type::Class { args, .. } if args.len() == 1 => {
            // only when the class is Array; callers pass New's type which is Array(_)
            None
        }
        _ => None,
    }
}

/// Expand type aliases. `alias_ids` are `type T = …` (non-empty rhs).
/// Hitting an alias already on `seen` is `illegal cyclic reference`.
fn expand_alias_type(
    st: &SymbolTable,
    ty: &Type,
    alias_ids: &HashSet<u32>,
    seen: &mut Vec<u32>,
) -> Result<Type, SymbolId> {
    match ty {
        Type::TypeMember(id) => {
            if !st.get(*id).tparams.is_empty() {
                return Ok(Type::TypeMember(*id));
            }
            let rhs = st.get(*id).ty.clone();
            match &rhs {
                Type::NoType | Type::Error => Ok(Type::TypeMember(*id)),
                Type::TypeMember(x) if *x == *id => {
                    if alias_ids.contains(&id.0) {
                        Err(*id)
                    } else {
                        Ok(Type::TypeMember(*id))
                    }
                }
                other => {
                    if seen.contains(&id.0) {
                        return Err(*id);
                    }
                    seen.push(id.0);
                    let r = expand_alias_type(st, other, alias_ids, seen);
                    seen.pop();
                    r
                }
            }
        }
        Type::Class { sym, args } => {
            let args: Result<Vec<_>, _> = args
                .iter()
                .map(|a| expand_alias_type(st, a, alias_ids, seen))
                .collect();
            Ok(Type::Class {
                sym: *sym,
                args: args?,
            })
        }
        Type::Applied { ctor, args } => {
            let ctor = expand_alias_type(st, ctor, alias_ids, seen)?;
            let args: Result<Vec<_>, _> = args
                .iter()
                .map(|a| expand_alias_type(st, a, alias_ids, seen))
                .collect();
            Ok(st.expand_applied_hk_alias(crate::symbol::apply_type_ctor(ctor, args?)))
        }
        Type::Array(t) => Ok(Type::Array(Box::new(expand_alias_type(
            st, t, alias_ids, seen,
        )?))),
        Type::Function { params, ret } => {
            let params: Result<Vec<_>, _> = params
                .iter()
                .map(|p| expand_alias_type(st, p, alias_ids, seen))
                .collect();
            Ok(Type::Function {
                params: params?,
                ret: Box::new(expand_alias_type(st, ret, alias_ids, seen)?),
            })
        }
        Type::Tuple(ts) => {
            let ts: Result<Vec<_>, _> = ts
                .iter()
                .map(|t| expand_alias_type(st, t, alias_ids, seen))
                .collect();
            Ok(Type::Tuple(ts?))
        }
        Type::Named { name, args } => {
            let args: Result<Vec<_>, _> = args
                .iter()
                .map(|a| expand_alias_type(st, a, alias_ids, seen))
                .collect();
            Ok(Type::Named {
                name: name.clone(),
                args: args?,
            })
        }
        Type::Refined { parents, decls } => {
            let parents: Result<Vec<_>, _> = parents
                .iter()
                .map(|p| expand_alias_type(st, p, alias_ids, seen))
                .collect();
            Ok(Type::Refined {
                parents: parents?,
                decls: decls.clone(),
            })
        }
        Type::Annotated { tpe, annot } => Ok(Type::Annotated {
            tpe: Box::new(expand_alias_type(st, tpe, alias_ids, seen)?),
            annot: annot.clone(),
        }),
        other => Ok(other.clone()),
    }
}

fn needs_classtag_elem(elem: &Type) -> bool {
    matches!(
        elem,
        Type::TypeParam(_) | Type::TypeMember(_) | Type::Wildcard
    )
}

fn implicit_class_conversions(body: &[Tree]) -> Vec<Tree> {
    let mut out = Vec::new();
    for stt in body {
        let TreeKind::ClassDef {
            mods,
            name,
            vparamss,
            ..
        } = &stt.kind
        else {
            continue;
        };
        if !mods.flags.contains(Flags::IMPLICIT) {
            continue;
        }
        let Some(params) = vparamss.first() else {
            continue;
        };
        if params.len() != 1 {
            continue;
        }
        let p = &params[0];
        let pname = p.name().unwrap_or("x$0").to_string();
        let tpt = match &p.kind {
            TreeKind::ValDef { tpt, .. } => (**tpt).clone(),
            _ => continue,
        };
        let mut param = Tree::dummy(TreeKind::ValDef {
            mods: Modifiers::new(Flags::PARAM.with(Flags::SYNTHETIC)),
            name: pname.clone(),
            tpt: Box::new(tpt),
            rhs: Box::new(Tree::dummy(TreeKind::Empty)),
        });
        param.span = p.span;
        let cls_tpt = Tree {
            id: NodeId(0),
            span: stt.span,
            kind: TreeKind::Ident { name: name.clone() },
            ty: Type::NoType,
            sym: stt.sym,
            postfix: false,
        };
        let arg = Tree {
            id: NodeId(0),
            span: p.span,
            kind: TreeKind::Ident { name: pname },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        let rhs = Tree {
            id: NodeId(0),
            span: stt.span,
            kind: TreeKind::Apply {
                fun: Box::new(Tree {
                    id: NodeId(0),
                    span: stt.span,
                    kind: TreeKind::New {
                        tpt: Box::new(cls_tpt),
                    },
                    ty: Type::NoType,
                    sym: stt.sym,
                    postfix: false,
                }),
                args: vec![arg],
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
        };
        let mut conv = Tree::dummy(TreeKind::DefDef {
            mods: Modifiers::new(Flags::IMPLICIT.with(Flags::SYNTHETIC)),
            name: name.clone(),
            tparams: vec![],
            vparamss: vec![vec![param]],
            tpt: Box::new(Tree {
                id: NodeId(0),
                span: stt.span,
                kind: TreeKind::Ident { name: name.clone() },
                ty: Type::NoType,
                sym: stt.sym,
                postfix: false,
            }),
            rhs: Box::new(rhs),
        });
        conv.span = stt.span;
        out.push(conv);
    }
    out
}

fn split_repeated(params: &[Type]) -> (&[Type], Option<&Type>) {
    match params.last() {
        Some(Type::Repeated(t)) => (&params[..params.len() - 1], Some(t.as_ref())),
        _ => (params, None),
    }
}

fn param_at(params: &[Type], i: usize) -> Option<&Type> {
    let (fixed, repeated) = split_repeated(params);
    if i < fixed.len() {
        Some(&fixed[i])
    } else {
        repeated
    }
}

fn unify_tparam(tp: SymbolId, params: &[Type], args: &[Type]) -> Option<Type> {
    for (p, a) in params.iter().zip(args) {
        if let Some(t) = unify_one(tp, p, a) {
            return Some(t);
        }
    }
    None
}

/// True when `args` instantiate `tps` rather than still mentioning them
/// (`Inv[A @uncheckedVariance]` is not an instantiation of `Inv`).
fn type_args_are_instantiated(args: &[Type], tps: &[SymbolId]) -> bool {
    !args.is_empty()
        && (tps.is_empty() || args.len() == tps.len())
        && args.iter().all(|a| !still_raw_tparam(a, tps))
}

fn still_raw_tparam(ty: &Type, tps: &[SymbolId]) -> bool {
    match ty {
        Type::TypeParam(id) => tps.contains(id),
        Type::Applied { ctor, args } => {
            still_raw_tparam(ctor, tps) || args.iter().any(|a| still_raw_tparam(a, tps))
        }
        Type::Annotated { tpe, .. } => still_raw_tparam(tpe, tps),
        _ => false,
    }
}

fn unify_one(tp: SymbolId, pattern: &Type, actual: &Type) -> Option<Type> {
    if let Type::Annotated { tpe, .. } = actual {
        return unify_one(tp, pattern, tpe);
    }
    match pattern {
        Type::Annotated { tpe, .. } => unify_one(tp, tpe, actual),
        Type::TypeParam(id) if *id == tp => {
            if actual.is_no_type() || actual.is_error() {
                None
            } else {
                Some(actual.widen_constant())
            }
        }
        Type::BoundedWildcard { hi: Some(h), .. } | Type::BoundedWildcard { lo: Some(h), .. } => {
            unify_one(tp, h, actual)
        }
        Type::Wildcard => None,
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
        Type::Applied { ctor, args: pas } => match actual {
            Type::Applied {
                ctor: ac,
                args: aas,
            } => {
                if let Some(t) = unify_one(tp, ctor, ac) {
                    return Some(t);
                }
                for (p, a) in pas.iter().zip(aas) {
                    if let Some(t) = unify_one(tp, p, a) {
                        return Some(t);
                    }
                }
                None
            }
            Type::Class { sym, args: aas } => {
                let unapplied = Type::Class {
                    sym: *sym,
                    args: vec![],
                };
                if let Some(t) = unify_one(tp, ctor, &unapplied) {
                    return Some(t);
                }
                for (p, a) in pas.iter().zip(aas) {
                    if let Some(t) = unify_one(tp, p, a) {
                        return Some(t);
                    }
                }
                None
            }
            _ => None,
        },
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
            _ => unify_one(tp, p, actual),
        },
        Type::Repeated(p) => unify_one(tp, p, actual),
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
                let mut has_main = false;
                for b in &impl_.body {
                    if let TreeKind::DefDef { name: mn, .. } = &b.kind {
                        if mn == "main" {
                            has_main = true;
                        }
                    }
                }
                if !has_main {
                    has_main = impl_.parents.iter().any(|p| parent_is_app(st, p));
                }
                if has_main {
                    out.push(name.clone());
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

fn parent_is_app(st: &SymbolTable, p: &Tree) -> bool {
    let id = st
        .class_sym_of(&p.ty)
        .or_else(|| if p.sym.is_none() { None } else { Some(p.sym) });
    let Some(id) = id else {
        return p.name() == Some("App");
    };
    class_extends_named(st, id, "App")
}

fn class_extends_named(st: &SymbolTable, id: SymbolId, name: &str) -> bool {
    if st.get(id).name == name {
        return true;
    }
    let mut work = st.get(id).parents.clone();
    let mut seen = std::collections::HashSet::new();
    seen.insert(id.0);
    while let Some(p) = work.pop() {
        let Some(pid) = st.class_sym_of(&p) else {
            continue;
        };
        if !seen.insert(pid.0) {
            continue;
        }
        if st.get(pid).name == name {
            return true;
        }
        work.extend(st.get(pid).parents.clone());
    }
    false
}

/// Replace quantified existential names (`type X` / `type X <: Bound`) with
/// wildcards. Bounded forms become `BoundedWildcard` so pickle/erasure reuse
/// the `List[_ <: AnyRef]` path.
struct ExistQuant {
    name: String,
    lo: Option<Type>,
    hi: Option<Type>,
}

fn subst_quantified(ty: Type, qs: &[ExistQuant]) -> Type {
    if qs.is_empty() {
        return ty;
    }
    let replace = |name: &str, args: &[Type]| -> Option<Type> {
        if !args.is_empty() {
            return None;
        }
        qs.iter().find(|q| q.name == name).map(|q| {
            if q.lo.is_none() && q.hi.is_none() {
                Type::Wildcard
            } else {
                Type::BoundedWildcard {
                    lo: q.lo.clone().map(Box::new),
                    hi: q.hi.clone().map(Box::new),
                }
            }
        })
    };
    match ty {
        Type::Named { name, args } => {
            if let Some(w) = replace(&name, &args) {
                w
            } else {
                Type::Named {
                    name,
                    args: args.into_iter().map(|a| subst_quantified(a, qs)).collect(),
                }
            }
        }
        Type::Class { sym, args } => Type::Class {
            sym,
            args: args.into_iter().map(|a| subst_quantified(a, qs)).collect(),
        },
        Type::Applied { ctor, args } => Type::Applied {
            ctor: Box::new(subst_quantified(*ctor, qs)),
            args: args.into_iter().map(|a| subst_quantified(a, qs)).collect(),
        },
        Type::Array(t) => Type::Array(Box::new(subst_quantified(*t, qs))),
        Type::Function { params, ret } => Type::Function {
            params: params
                .into_iter()
                .map(|p| subst_quantified(p, qs))
                .collect(),
            ret: Box::new(subst_quantified(*ret, qs)),
        },
        Type::Method { paramss, ret } => Type::Method {
            paramss: paramss
                .into_iter()
                .map(|ps| ps.into_iter().map(|p| subst_quantified(p, qs)).collect())
                .collect(),
            ret: Box::new(subst_quantified(*ret, qs)),
        },
        Type::ByName(t) => Type::ByName(Box::new(subst_quantified(*t, qs))),
        Type::Repeated(t) => Type::Repeated(Box::new(subst_quantified(*t, qs))),
        Type::Tuple(ts) => Type::Tuple(ts.into_iter().map(|t| subst_quantified(t, qs)).collect()),
        Type::Overload(alts) => {
            Type::Overload(alts.into_iter().map(|t| subst_quantified(t, qs)).collect())
        }
        Type::Annotated { tpe, annot } => Type::Annotated {
            tpe: Box::new(subst_quantified(*tpe, qs)),
            annot,
        },
        Type::BoundedWildcard { lo, hi } => Type::BoundedWildcard {
            lo: lo.map(|t| Box::new(subst_quantified(*t, qs))),
            hi: hi.map(|t| Box::new(subst_quantified(*t, qs))),
        },
        other => other,
    }
}

fn path_display(t: &Tree) -> String {
    match &t.kind {
        TreeKind::Ident { name } => name.clone(),
        TreeKind::Select { qual, name } => format!("{}.{}", path_display(qual), name),
        TreeKind::SelectFromTypeTree { qual, name, hash } => {
            let op = if *hash { "#" } else { "." };
            format!("{}{op}{name}", path_display(qual))
        }
        TreeKind::This { qual: None } => "this".into(),
        TreeKind::This { qual: Some(q) } => format!("{q}.this"),
        TreeKind::Super { .. } => "super".into(),
        TreeKind::Apply { fun, .. } => format!("{}()", path_display(fun)),
        TreeKind::New { tpt } => format!("new {}", tpt.name().unwrap_or("?")),
        _ => t.name().unwrap_or("<expr>").to_string(),
    }
}

fn structural_select_lhs(lhs: &Tree) -> bool {
    match &lhs.kind {
        TreeKind::Select { qual, .. } => match &qual.ty {
            Type::Refined { decls, .. } => SymbolTable::refined_has_term_members(decls),
            _ => false,
        },
        _ => false,
    }
}

pub fn has_errors(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.level == scala_rs_span::Level::Error)
}
