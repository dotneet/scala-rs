#![allow(dead_code)]
//! Namer + typer. Trees are mutated in place (`ty`, `sym`).

use crate::implicits::ImplicitSearch;
use crate::javaclass::BinaryIndex;
use crate::lazysig::PendingSig;
use crate::prelude::install_prelude;
use crate::symbol::{SymKind, SymbolTable};
use crate::uncurry::{eta_expand, is_eta_marker};
use scala_rs_parser::ast::*;
use scala_rs_span::{Diagnostic, Span};
use std::collections::{HashMap, HashSet};
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
    /// Enclosing package clauses; a nested one is relative to the last.
    pkg_nest: Vec<SymbolId>,
    /// Signature pass: fill member types across the whole run before any body
    /// is typed, so a unit can call into one that comes later.
    sigs_only: bool,
    /// Members whose signature the signature pass already built. Signature
    /// work is not idempotent -- it synthesizes evidence parameters and
    /// default getters -- so the body pass must not redo it.
    sig_done: std::collections::HashSet<(usize, scala_rs_parser::NodeId)>,
    fatal_warnings: bool,
    library_abi: bool,
    /// Nearest enclosing named method; `None` in class/object constructors.
    pub(crate) return_meth: Option<SymbolId>,
    /// `import scala.language.dynamics` / `-language:dynamics`.
    language_dynamics: bool,
    /// `import scala.language.postfixOps` / `-language:postfixOps`.
    language_postfix_ops: bool,
    /// `import scala.language.implicitConversions` / `-language:implicitConversions`.
    language_implicit_conversions: bool,
    binary: BinaryIndex,
    completed_java: HashSet<String>,
    /// While a template's parent list is being typed: `(the class, the class
    /// that encloses it)`. nsc types parents in the *outer* context, so
    /// `class B extends super.B` inside `trait Mid` means `Mid`'s `super`.
    parent_ctx: Option<(SymbolId, SymbolId)>,
    /// Fills library members the hand-written prelude does not declare, from
    /// their `ScalaSignature` pickles. Only consulted when resolution failed.
    pickle: crate::pickle_supply::PickleSupply,
    /// Packages whose jar package object's pickled `type` aliases have been
    /// installed (see `install_pickled_package_aliases`). One read per package.
    pkg_aliases_done: HashSet<u32>,
    /// Pickled package-object aliases whose right-hand side could not be
    /// rebuilt, by simple name: the name then reports *why* it is missing
    /// instead of the bare "not found".
    pkg_alias_gaps: HashMap<String, String>,
    /// Members without a type annotation, keyed by symbol: nsc's lazy
    /// completers (see `crate::lazysig`).
    pub(crate) pending_sigs: HashMap<SymbolId, PendingSig>,
    /// Signatures being completed right now (nsc's `LOCKED` flag).
    pub(crate) lazy_completing: Vec<SymbolId>,
    /// Symbols a `recursive ... needs type` was already reported for.
    pub(crate) lazy_cyclic: HashSet<SymbolId>,
    /// Definitions completed on demand, waiting to be spliced back.
    pub(crate) lazy_done: HashMap<SymbolId, Tree>,
    /// Definitions already spliced back; both template passes skip them.
    pub(crate) lazy_body_done: HashSet<SymbolId>,
    /// Number of scopes the prelude occupies; they stay in place while a
    /// signature is completed in the scope of its own definition.
    pub(crate) lazy_base_scopes: usize,
    /// nsc's `openImplicits`: the (implicit symbol, target type) pairs whose
    /// own implicit parameters are being resolved right now. Used to cut off
    /// diverging expansions (`crate::implicits`).
    pub(crate) open_implicits: std::cell::RefCell<Vec<(SymbolId, Type)>>,
    /// The first expansion cut off as diverging during the current top-level
    /// implicit search, for the diagnostic.
    pub(crate) diverged_implicit: std::cell::RefCell<Option<(SymbolId, Type)>>,
}

pub fn typecheck(tree: &mut Tree, file_index: usize) -> (SymbolTable, Vec<Diagnostic>) {
    typecheck_opts(tree, file_index, &TypecheckOptions::default())
}

pub fn typecheck_opts(
    tree: &mut Tree,
    file_index: usize,
    opts: &TypecheckOptions,
) -> (SymbolTable, Vec<Diagnostic>) {
    let mut units = [(tree, file_index)];
    typecheck_units(&mut units, opts)
}

/// How many times the header pass may sweep the run. Each round can only
/// turn rough (by-name) parents into resolved ones, so it converges; the cap
/// just bounds the work for deeply nested templates.
const MAX_HEADER_ROUNDS: usize = 3;

/// Typecheck a whole run in one symbol table: every unit is named before any
/// is typed, so a class can reference one defined in another file.
pub fn typecheck_units(
    units: &mut [(&mut Tree, usize)],
    opts: &TypecheckOptions,
) -> (SymbolTable, Vec<Diagnostic>) {
    let first = units.first().map(|(_, i)| *i).unwrap_or(0);
    let mut t = Typer::new(first, opts);
    t.fatal_warnings = opts.fatal_warnings;
    crate::classpath::install_classpath(&mut t.st, &opts.classpath);
    for (tree, file_index) in units.iter_mut() {
        t.file_index = *file_index;
        t.namer(tree);
        t.register_sealed_from_namer(tree);
    }
    {
        // Class headers before member types, across every unit: a class can
        // inherit from one whose own superclass chain is declared in a file
        // that comes later on the command line, and inherited names have to
        // be visible while that class's members are typed.
        let diag_mark = t.diags.len();
        let saved_lang = (
            t.language_dynamics,
            t.language_postfix_ops,
            t.language_implicit_conversions,
        );
        for _ in 0..MAX_HEADER_ROUNDS {
            let mut changed = false;
            for (tree, file_index) in units.iter_mut() {
                t.file_index = *file_index;
                changed |= t.parents_pass(tree, false);
            }
            if !changed {
                break;
            }
        }
        // Once the parents are known, one more sweep types the constructor
        // parameters, so `extends Parent(x)` in any file meets a complete
        // primary constructor.
        for (tree, file_index) in units.iter_mut() {
            t.file_index = *file_index;
            t.parents_pass(tree, true);
        }
        // The header pass exists only to resolve parents; it types imports
        // and parent trees before signatures are known, so anything it
        // complains about is reported for real by the passes below.
        t.diags.truncate(diag_mark);
        t.language_dynamics = saved_lang.0;
        t.language_postfix_ops = saved_lang.1;
        t.language_implicit_conversions = saved_lang.2;
    }
    {
        // Member types first, across every unit: typing a body may call a
        // member declared further down the file, or in a file that comes
        // later on the command line.
        t.sigs_only = true;
        for (tree, file_index) in units.iter_mut() {
            t.file_index = *file_index;
            t.typer(tree);
        }
        t.sigs_only = false;
    }
    for (tree, file_index) in units.iter_mut() {
        t.file_index = *file_index;
        t.typer(tree);
        t.report_macro_calls(tree);
        t.strip_macro_defs(tree);
    }
    // Class headers are typed by both passes, so the same complaint about a
    // parent or self type is raised twice. Member signatures are built once
    // (see `sig_done`), so their diagnostics survive here.
    dedup_diags(&mut t.diags);
    (t.st, t.diags)
}

impl Typer {
    pub fn new(file_index: usize, opts: &TypecheckOptions) -> Self {
        let mut st = SymbolTable::new();
        install_prelude(&mut st, opts.library_abi);
        let lazy_base_scopes = st.scopes.len();
        Typer {
            st,
            diags: Vec::new(),
            file_index,
            gensym: 0,
            pkg_nest: Vec::new(),
            sigs_only: false,
            sig_done: std::collections::HashSet::new(),
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
            parent_ctx: None,
            pickle: crate::pickle_supply::PickleSupply::new(),
            pkg_aliases_done: HashSet::new(),
            pkg_alias_gaps: HashMap::new(),
            pending_sigs: HashMap::new(),
            lazy_completing: Vec::new(),
            lazy_cyclic: HashSet::new(),
            lazy_done: HashMap::new(),
            lazy_body_done: HashSet::new(),
            lazy_base_scopes,
            open_implicits: std::cell::RefCell::new(Vec::new()),
            diverged_implicit: std::cell::RefCell::new(None),
        }
    }

    pub(crate) fn error(&mut self, span: Span, msg: impl Into<String>) {
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
                self.pkg_nest.push(pkg);
                self.st.push_scope();
                // First pass: enter classes/modules so they can forward-ref.
                for stt in stats.iter_mut() {
                    self.namer_enter_tmpl(stt);
                }
                for stt in stats.iter_mut() {
                    self.namer(stt);
                }
                self.st.pop_scope();
                self.pkg_nest.pop();
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
        // A nested package clause is relative: `package p` then `package q`
        // is `p.q`.
        let mut cur = self.pkg_nest.last().copied().unwrap_or(self.st.root);
        let mut jvm = if cur == self.st.root {
            String::new()
        } else {
            self.st.get(cur).jvm_name.clone()
        };
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
                // A `case class` already synthesized its companion; reuse that
                // symbol so `object C` does not become a second module.
                let owner = self.st.owner;
                if let Some(m) = self.st.lookup(name).into_iter().find(|&s| {
                    self.st.get(s).kind == SymKind::Module
                        && self.st.get(s).owner == owner
                        && self.st.get(s).flags.contains(Flags::SYNTHETIC)
                }) {
                    let mut f = self.st.get(m).flags.with(mods.flags).with(Flags::MODULE);
                    f.set(Flags::SYNTHETIC, false);
                    self.st.get_mut(m).flags = f;
                    self.st.get_mut(m).annotations = annots;
                    let cls = self.st.module_class_of(m);
                    let mut cf = self.st.get(cls).flags;
                    cf.set(Flags::SYNTHETIC, false);
                    self.st.get_mut(cls).flags = cf;
                    tree.sym = m;
                    return;
                }
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
        // A class defined inside a method (`new S { … }`) has the method as
        // its owner; nsc still names it after the enclosing class.
        let mut owner = self.st.owner;
        while !owner.is_none() {
            let ow = self.st.get(owner);
            if ow.kind == SymKind::Package {
                return if ow.name != "<_root_>"
                    && !ow.jvm_name.is_empty()
                    && ow.jvm_name != "scala/runtime"
                {
                    format!("{}/{}", ow.jvm_name, name)
                } else {
                    name.to_string()
                };
            }
            let base = ow.jvm_name.trim_end_matches('$');
            if !base.is_empty() {
                return format!("{base}${name}");
            }
            owner = ow.owner;
        }
        name.to_string()
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
        for tp in tparams.iter_mut() {
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
        // Bounds resolve after every parameter is in scope, so F-bounded
        // `A <: Comparable[A]` sees `A`.
        for tp in tparams.iter() {
            let TreeKind::TypeDef { lo, hi, .. } = &tp.kind else {
                continue;
            };
            let id = tp.sym;
            if id.is_none() {
                continue;
            }
            if let Some(t) = lo {
                let ty = self.tree_to_type(t);
                if !ty.is_error() {
                    self.st.get_mut(id).bound_lo = Some(ty);
                }
            }
            if let Some(t) = hi {
                let ty = self.tree_to_type(t);
                if !ty.is_error() {
                    self.st.get_mut(id).bound_hi = Some(ty);
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
        // An unresolved name applied to type arguments is a missing type, not a
        // kind error: nsc reports `not found: type X`.
        if let Type::Named { name, args: pre } = &ctor {
            if pre.is_empty() && self.st.class_sym_of(&ctor).is_none() {
                let name = name.clone();
                self.not_found_error(span, "type", &name);
                return Type::Error;
            }
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
        let expanded = self.st.expand_applied_hk_alias(applied);
        // An alias body may still name an abstract member (`type C[T] = self.C[T]`).
        // The class being typed knows how it implements those, so re-read the
        // result from there, as `bind_found` does for term types.
        if self.st.this_class.is_none() {
            expanded
        } else {
            self.st.expand_type_members(self.st.this_class, &expanded)
        }
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
            // A view bound on a higher-kinded parameter is illegal; a context
            // bound on one is not (`class C[F[_]: Async]` is accepted by nsc).
            if !inner.is_empty() && !views.is_empty() {
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

    /// `productPrefix: String` and `productArity: Int`, the two `scala.Product`
    /// members nsc synthesizes on every `case class` and `case object` without
    /// needing the rest of `Product`. The backend folds both to constants.
    fn synthesize_product_members(&mut self, class_id: SymbolId) {
        for (name, ret) in [("productPrefix", Type::String), ("productArity", Type::Int)] {
            if self
                .st
                .get(class_id)
                .members
                .iter()
                .any(|&m| self.st.get(m).name == name)
            {
                continue;
            }
            let id = self
                .st
                .alloc(name, class_id, SymKind::Method, Flags::SYNTHETIC, "");
            self.st.get_mut(id).ty = Type::Method {
                paramss: vec![],
                ret: Box::new(ret),
            };
        }
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
        // `copy`'s own parameter symbols are distinct from `ctor_fields`: reusing
        // the constructor's field symbols directly (as the companion `apply`
        // does below) would mean giving them `DEFAULTPARAM` + a `this.field`
        // default, which would then also apply to `apply`/`<init>` calls where
        // there is no `this` to default from. Each param defaults to the
        // matching field of the receiver, exactly like nsc's synthesized
        // `copy$default$N` getters (built by `synthesize_default_getters` below,
        // the same machinery a user-written `def f(x: Int = 5)` uses).
        let copy_params: Vec<SymbolId> = fields
            .iter()
            .map(|f| {
                let fname = self.st.get(*f).name.clone();
                let fty = self.st.get(*f).ty.clone();
                let pid = self.st.alloc(
                    &fname,
                    copy,
                    SymKind::Term,
                    Flags::PARAM.with(Flags::DEFAULTPARAM),
                    "",
                );
                self.st.get_mut(pid).ty = fty;
                let this_tree = Tree::dummy(TreeKind::This { qual: None });
                let default_rhs = Tree::dummy(TreeKind::Select {
                    qual: Box::new(this_tree),
                    name: fname,
                });
                self.st.get_mut(pid).default_rhs = Some(default_rhs);
                pid
            })
            .collect();
        let ptys: Vec<Type> = copy_params
            .iter()
            .map(|p| self.st.get(*p).ty.clone())
            .collect();
        self.st.get_mut(copy).params = copy_params.clone();
        self.st.get_mut(copy).paramss = vec![copy_params.clone()];
        self.st.get_mut(copy).ty = Type::Method {
            paramss: vec![ptys],
            ret: Box::new(class_ty.clone()),
        };
        // Field types are not resolved yet at this point in the namer pass;
        // `type_class` re-syncs `copy`'s param types from the real ctor
        // signature and synthesizes `copy$default$N` there instead.
        self.synthesize_product_members(class_id);
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
        // A `case object` is a `Product` too: nsc gives it `productPrefix` and
        // `productArity` (0), both folded to constants by the backend.
        if matches!(&tree.kind, TreeKind::ModuleDef { mods, .. } if mods.flags.contains(Flags::CASE))
        {
            self.synthesize_product_members(cls);
        }
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
        if matches!(
            &tree.kind,
            TreeKind::ValDef { .. } | TreeKind::DefDef { .. } | TreeKind::TypeDef { .. }
        ) {
            self.register_namer_sig(tree);
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
                for (from, to) in decode_import_selectors(name) {
                    if from == "_" || to == "_" {
                        continue;
                    }
                    let found = self.st.lookup(&from);
                    for f in found {
                        self.st.enter_in_current(&to, f);
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

    // ---------------------------------------------------------- header pass

    /// Pin every template's parent list down to real symbols, before any
    /// signature in the run is typed.
    ///
    /// The namer records parents by bare name (`rough_parents`), and
    /// `class_sym_of` resolves such a name in whatever scope happens to be
    /// current when it is asked. The signature pass walks the units in
    /// command-line order, so a class whose superclass chain is defined in a
    /// *later* file used to have its grandparents looked up in the wrong
    /// scope: the chain broke there and every type inherited past that point
    /// went missing (`not found: type Table` for slick's cake-pattern
    /// profiles). Resolving the parents of every unit first, each in the
    /// scope of its own definition, makes `enter_inherited_members` see the
    /// whole linearization regardless of file order.
    ///
    /// Returns `true` when a parent was pinned down, so the caller can
    /// iterate: an inner class may extend a name that its *outer* class
    /// inherits, which only becomes visible once the outer parents are known.
    fn parents_pass(&mut self, tree: &mut Tree, ctors: bool) -> bool {
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
                let mut changed = false;
                for s in stats.iter_mut() {
                    changed |= self.parents_pass(s, ctors);
                }
                self.st.pop_scope();
                changed
            }
            // Parents are named through the imports of their own file.
            TreeKind::Import { .. } => {
                self.type_import(tree);
                false
            }
            TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => {
                self.parents_pass_tmpl(tree, ctors)
            }
            _ => false,
        }
    }

    fn parents_pass_tmpl(&mut self, tree: &mut Tree, ctors: bool) -> bool {
        let id = match &tree.kind {
            TreeKind::ClassDef { .. } => tree.sym,
            TreeKind::ModuleDef { .. } => match self.st.get(tree.sym).ty {
                Type::ModuleRef(c) => c,
                _ => tree.sym,
            },
            _ => return false,
        };
        if id.is_none() {
            return false;
        }
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        self.st.owner = id;
        self.st.this_class = id;
        self.st.push_scope();
        for m in self.st.get(id).members.clone() {
            let n = self.st.get(m).name.clone();
            self.st.enter_in_current(&n, m);
        }
        let parent_trees: Vec<Tree> = match &tree.kind {
            TreeKind::ClassDef { impl_, .. } | TreeKind::ModuleDef { impl_, .. } => {
                impl_.parents.clone()
            }
            _ => Vec::new(),
        };
        let mut changed = false;
        let mut rough = self.st.get(id).parents.clone();
        // `rough_parents` substitutes `AnyRef` for an empty `extends`; only a
        // one-for-one list came from the source and can be matched up.
        if rough.len() == parent_trees.len() {
            for (slot, p) in rough.iter_mut().zip(parent_trees.iter()) {
                if !matches!(slot, Type::Named { .. }) {
                    continue;
                }
                if let Some(sym) = self.parent_head_sym(p) {
                    if sym != id {
                        *slot = Type::Class {
                            sym,
                            args: Vec::new(),
                        };
                        changed = true;
                    }
                }
            }
            if changed {
                self.st.get_mut(id).parents = rough;
            }
        }
        // An inner template may extend a name inherited by this one.
        self.enter_inherited_members(id);
        if ctors {
            self.header_ctor_sig(tree, id);
        }
        let body = match &mut tree.kind {
            TreeKind::ClassDef { impl_, .. } | TreeKind::ModuleDef { impl_, .. } => &mut impl_.body,
            _ => {
                self.st.pop_scope();
                self.st.owner = saved_owner;
                self.st.this_class = saved_this;
                return changed;
            }
        };
        for stt in body.iter_mut() {
            if matches!(
                stt.kind,
                TreeKind::Import { .. } | TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
            ) {
                changed |= self.parents_pass(stt, ctors);
            }
        }
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        changed
    }

    /// Give `id`'s primary constructor its real parameter types.
    ///
    /// `extends Table[Int](n)` is checked against the parent's `<init>`, and
    /// the namer only knows that constructor's *arity*: its parameter types
    /// arrive with the parent's own signature pass. A subclass in a file
    /// listed before its parent therefore saw untyped parameters and reported
    /// `no matching overload for constructor`. Running this for every unit
    /// before any parent clause is checked removes the file-order dependency.
    /// The signature pass redoes the work (and adds the evidence parameters
    /// a context bound needs), so this only ever runs ahead of it.
    fn header_ctor_sig(&mut self, tree: &mut Tree, id: SymbolId) {
        let TreeKind::ClassDef { vparamss, .. } = &mut tree.kind else {
            return;
        };
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

    /// The class symbol a parent clause names, or `None` when the name does
    /// not resolve to a class yet. Deliberately narrow: an abstract type
    /// member or type parameter is *not* accepted as a parent symbol here,
    /// because `class_sym_of` would chase its bound and pin down a class the
    /// source never named.
    fn parent_head_sym(&mut self, p: &Tree) -> Option<SymbolId> {
        let mut t = p;
        loop {
            match &t.kind {
                TreeKind::Apply { fun, .. } => t = fun,
                TreeKind::New { tpt } => t = tpt,
                _ => break,
            }
        }
        fn head(ty: &Type) -> Option<SymbolId> {
            match ty {
                Type::Class { sym, .. } | Type::ModuleRef(sym) => Some(*sym),
                Type::Applied { ctor, .. } => head(ctor),
                _ => None,
            }
        }
        let ty = self.tree_to_type(t);
        let sym = head(&ty)?;
        if self.st.get(sym).is_class_like() {
            Some(sym)
        } else {
            None
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
        if let TreeKind::ClassDef { mods, vparamss, .. } = &tree.kind {
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
                // scalac 2.13.16 does not warn for an `implicit class` even
                // under `-feature`; only an implicit conversion method does.
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
        // The signature pass and the body pass walk the same tree; the
        // evidence clause must be appended by only one of them.
        let ev_fresh = tree.id == scala_rs_parser::NodeId(0)
            || self.sig_done.insert((self.file_index, tree.id));
        let evidence = if is_trait || !ev_fresh {
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
                    // `type_val_sig` sets the `DEFAULTPARAM` flag but (unlike
                    // `type_def_sig` for ordinary methods) never captures the
                    // default value tree itself — a ctor param default
                    // (`class Foo(x: Int, y: Int = 5)`) would otherwise never
                    // get filled in at a `new Foo(1)` call site.
                    if let TreeKind::ValDef { mods, rhs, .. } = &p.kind {
                        if mods.flags.contains(Flags::DEFAULTPARAM) && !rhs.is_empty() {
                            self.st.get_mut(p.sym).default_rhs = Some((**rhs).clone());
                        }
                    }
                    ids.push(p.sym);
                    all_ctor_params.push(p.sym);
                }
            }
            paramss_ty.push(ct);
            paramss_ids.push(ids);
        }
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
                    // Unlike an ordinary method's `name$default$N` getters, a
                    // constructor default can't be an instance method on the
                    // class being constructed (there is no receiver yet at
                    // `new Foo(1)`; nsc emits these on the companion instead).
                    // `default_getter_apply`'s receiver logic assumes an
                    // existing instance, so it doesn't fit constructors —
                    // deliberately not calling `synthesize_default_getters`
                    // here. `fill_defaults_and_implicits` still fills the
                    // omitted arg via the raw `default_rhs` fallback above,
                    // typed directly at the call site; this covers simple
                    // defaults (literals, `null`, ...) but not one referring
                    // to an earlier ctor param, which would need real
                    // companion-based getters (not implemented here).
                }
            }
        }
        // `copy`'s parameter symbols (allocated in `synthesize_case_members`,
        // during the namer pass, before ctor param types are known) still hold
        // `Type::NoType` until now. Re-sync them from the just-resolved field
        // types, then (re)build `copy$default$N` — this is also the first time
        // `synthesize_default_getters` runs for `copy`, since doing it any
        // earlier would have baked in the same `NoType` placeholders.
        if !id.is_none() && self.st.get(id).flags.contains(Flags::CASE) {
            let all_ctor_param_tys: Vec<Type> = paramss_ty.iter().flatten().cloned().collect();
            if let Some(copy_id) = self.st.get(id).members.iter().copied().find(|&m| {
                self.st.get(m).kind == SymKind::Method
                    && self.st.get(m).name == "copy"
                    && self.st.get(m).flags.contains(Flags::SYNTHETIC)
            }) {
                let copy_params = self.st.get(copy_id).params.clone();
                if copy_params.len() == all_ctor_param_tys.len() {
                    for (pid, ty) in copy_params.iter().zip(all_ctor_param_tys.iter()) {
                        self.st.get_mut(*pid).ty = ty.clone();
                    }
                    self.st.get_mut(copy_id).ty = Type::Method {
                        paramss: vec![all_ctor_param_tys],
                        ret: Box::new(Type::Class {
                            sym: id,
                            args: vec![],
                        }),
                    };
                    self.synthesize_default_getters(id, copy_id, "copy", &[], &[copy_params]);
                }
            }
        }
        let mut pts = Vec::new();
        let saved_parent_ctx = self.parent_ctx.replace((id, saved_this));
        for p in parents.iter_mut() {
            self.type_parent(p);
            pts.push(p.ty.clone());
        }
        self.parent_ctx = saved_parent_ctx;
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
        if !self.sigs_only {
            for stt in body.iter_mut() {
                self.type_member_body(stt);
            }
        }
        self.finish_case_apply(id, &paramss_ty, &paramss_ids);
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

    /// Fill in the signatures of the `apply` / `unapply` that
    /// `synthesize_case_members` allocated for a case class, now that the
    /// constructor's parameter types are known. Only those synthetic members
    /// are touched: a companion may also declare `apply`/`unapply` of its own
    /// (`object LiteralNode { def apply(tp: Type, v: Any, vol: Boolean = false) }`),
    /// and overwriting those with the constructor's signature would hide them.
    ///
    /// `unapply` extracts from the *first* parameter section only (nsc: extra
    /// curried sections, e.g. `case class F(name: String)(val opts: X)`, are
    /// not part of the pattern); `apply` mirrors every section, curried,
    /// exactly like the primary constructor.
    fn finish_case_apply(
        &mut self,
        class_id: SymbolId,
        paramss_ty: &[Vec<Type>],
        paramss_ids: &[Vec<SymbolId>],
    ) {
        if class_id.is_none() || !self.st.get(class_id).flags.contains(Flags::CASE) {
            return;
        }
        let ctor_param_tys = paramss_ty.first().cloned().unwrap_or_default();
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
            // nsc synthesizes `apply[A](a: A): Box[A]`, so `Box(1)` infers
            // `A`. The class's own parameters stand in for the method's.
            let tps = self.st.get(class_id).tparams.clone();
            let class_ty = Type::Class {
                sym: class_id,
                args: tps.iter().map(|t| Type::TypeParam(*t)).collect(),
            };
            let unapply_ret = match ctor_param_tys.len() {
                0 => Type::Boolean,
                1 => Type::Class {
                    sym: self.st.option_sym,
                    args: vec![ctor_param_tys[0].clone()],
                },
                _ => Type::Class {
                    sym: self.st.option_sym,
                    args: vec![Type::Tuple(ctor_param_tys.clone())],
                },
            };
            for mem in self.st.get(cls).members.clone() {
                if !self.st.get(mem).flags.contains(Flags::SYNTHETIC) {
                    continue;
                }
                let n = self.st.get(mem).name.clone();
                if n == "apply" {
                    self.st.get_mut(mem).tparams = tps.clone();
                    self.st.get_mut(mem).ty = Type::Method {
                        paramss: paramss_ty.to_vec(),
                        ret: Box::new(class_ty.clone()),
                    };
                    self.st.get_mut(mem).paramss = paramss_ids.to_vec();
                    self.st.get_mut(mem).params = paramss_ids.iter().flatten().copied().collect();
                } else if n == "unapply" {
                    self.st.get_mut(mem).tparams = tps.clone();
                    self.st.get_mut(mem).ty = Type::Method {
                        paramss: vec![vec![class_ty.clone()]],
                        ret: Box::new(unapply_ret.clone()),
                    };
                }
            }
        }
        // Primary constructor only. Auxiliary `def this` already has a Method
        // type from `type_def_sig`; overwriting it with the primary arity would
        // make `new C(1)` and `extends C(1)` miss the aux overload.
        for mem in self.st.get(class_id).members.clone() {
            if self.st.get(mem).name == "<init>" && self.st.get(mem).ty.is_no_type() {
                self.st.get_mut(mem).ty = Type::Method {
                    paramss: paramss_ty.to_vec(),
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
        let saved_parent_ctx = self.parent_ctx.replace((cls, saved_this));
        for p in parents.iter_mut() {
            // Parents are types: `object B extends B` extends the *trait* B,
            // not itself. Typing them as expressions picks the module.
            self.type_parent(p);
            pts.push(p.ty.clone());
        }
        pts.retain(|t| !matches!(t, Type::ModuleRef(m) if *m == cls));
        self.parent_ctx = saved_parent_ctx;
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
        if !self.sigs_only {
            for stt in body.iter_mut() {
                self.type_member_body(stt);
            }
        }
        self.check_self_conformance(cls, tree.span);
        self.check_type_member_kind_override(cls, tree.span);
        self.st.pop_scope();
        self.st.owner = saved_owner;
        self.st.this_class = saved_this;
        self.return_meth = saved_ret;
        tree.ty = Type::ModuleRef(cls);
    }

    /// Resolve one `type T = rhs` on demand, right-hand side and all, the way
    /// the enclosing template's own pass would have. Used by `complete_lazy_sig`
    /// when a reference from another unit reaches the alias first.
    pub(crate) fn complete_type_alias_tree(&mut self, tree: &mut Tree) {
        self.type_type_member(tree);
        self.finish_one_type_alias(tree);
    }

    fn type_type_member(&mut self, tree: &mut Tree) {
        if self.take_lazy_done(tree) {
            return;
        }
        self.drop_lazy_sig(tree.sym);
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
                        // nsc compares the two declarations after aligning the
                        // parent's type parameters with the child's, so
                        // `type C[T] <: TypedType[T]` overridden by
                        // `type C[T] = JdbcType[T]` compares `JdbcType[T_child]`
                        // against `TypedType[T_child]`, not `TypedType[T_parent]`.
                        let align = |st: &crate::symbol::SymbolTable, t: Option<Type>| {
                            let ptps = st.get(m).tparams.clone();
                            let ctps = st.get(mid).tparams.clone();
                            let t = if ptps.is_empty() || ptps.len() != ctps.len() {
                                t
                            } else {
                                let args: Vec<Type> =
                                    ctps.iter().map(|&c| Type::TypeParam(c)).collect();
                                t.map(|t| crate::symbol::subst_tparams_slice(&ptps, &args, &t))
                            };
                            // The bound is stated in the parent's terms, so a
                            // sibling member it mentions (`type B[T] <: C[T]`)
                            // must be re-read as the child implements it, and
                            // a `this.type` in it (`type Self >: this.type`)
                            // means *the child's* `this` at the override site.
                            let t = t.map(|t| retarget_this(&t, class_id));
                            t.map(|t| st.expand_type_members(class_id, &t))
                        };
                        let parent_hi = align(&self.st, self.st.get(m).bound_hi.clone());
                        let parent_lo = align(&self.st, self.st.get(m).bound_lo.clone());
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
    /// `finish_type_aliases` for a single definition, for the on-demand path.
    fn finish_one_type_alias(&mut self, stt: &mut Tree) {
        let mut alias_ids = HashSet::new();
        if let TreeKind::TypeDef { rhs, .. } = &stt.kind {
            if !rhs.is_empty() && !stt.sym.is_none() {
                alias_ids.insert(stt.sym.0);
            }
        }
        if alias_ids.is_empty() {
            return;
        }
        self.expand_one_alias(stt, &alias_ids);
    }

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
            self.expand_one_alias(stt, &alias_ids);
        }
    }

    fn expand_one_alias(&mut self, stt: &mut Tree, alias_ids: &HashSet<u32>) {
        let TreeKind::TypeDef { name, rhs, .. } = &stt.kind else {
            return;
        };
        if rhs.is_empty() || stt.sym.is_none() || stt.ty.is_error() {
            return;
        }
        let name = name.clone();
        let span = stt.span;
        let mut seen = Vec::new();
        match expand_alias_type(&self.st, &stt.ty, alias_ids, &mut seen) {
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

    fn bind_self_type(
        &mut self,
        class_id: SymbolId,
        self_name: Option<String>,
        self_tpt: Option<&Tree>,
    ) {
        let st = match self_tpt {
            Some(tpt) => {
                let t = self.tree_to_type(tpt);
                if t.is_error() {
                    return;
                }
                self.st.get_mut(class_id).self_type = Some(t.clone());
                t
            }
            // `trait T { self => … }` names `this` without narrowing it.
            None => match &self_name {
                Some(_) => Type::ThisType(class_id),
                None => return,
            },
        };
        if let Some(cls) = self.st.class_sym_of(&st) {
            self.enter_members_of(cls);
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
                self.enter_members_of(pid);
                work.extend(self.st.get(pid).parents.clone());
            }
        }
        if let Some(name) = self_name {
            if name != "this" {
                // The signature pass and the body pass both bind the alias;
                // allocating twice would make `self` look like an overload.
                let sid = self.st.get(class_id).self_alias.unwrap_or_else(|| {
                    self.st
                        .alloc(&name, class_id, SymKind::Term, Flags::SYNTHETIC, "")
                });
                self.st.get_mut(class_id).self_alias = Some(sid);
                self.st.get_mut(sid).ty = st;
                self.st.enter_in_current(&name, sid);
            }
        }
    }

    /// Bring `cls`'s members into the current scope, minus the ones another
    /// template never inherits: its constructor, its compiler-made `$` names
    /// and its self alias (see `Symbol::self_alias`).
    fn enter_members_of(&mut self, cls: SymbolId) {
        let alias = self.st.get(cls).self_alias;
        for m in self.st.get(cls).members.clone() {
            if Some(m) == alias {
                continue;
            }
            let n = self.st.get(m).name.clone();
            if n.ends_with('$') || n == "<init>" {
                continue;
            }
            self.st.enter_in_current(&n, m);
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
            let alias = self.st.get(pid).self_alias;
            for m in self.st.get(pid).members.clone() {
                let n = self.st.get(m).name.clone();
                if n.ends_with('$') || n == "<init>" {
                    continue;
                }
                // A parent's type parameters are not inherited names; entering
                // them shadows an enclosing `A` of the same name
                // (`def wrap[A](…) = new Show[A] { … }`).
                if self.st.get(m).kind == SymKind::TypeParam {
                    continue;
                }
                // Neither is a parent's self alias (see `Symbol::self_alias`).
                if Some(m) == alias {
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
            // nsc does not variance-check what it synthesized: `case class
            // Box[+A](a: A)` is legal even though its `copy(a: A)` puts `A`
            // in a contravariant position.
            if self.st.get(m).flags.contains(Flags::SYNTHETIC) {
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
            // `Array` is invariant; `=> T` and `T*` keep the position.
            Type::Array(t) => {
                self.check_variance_ty(vars, t, 0, span, where_);
            }
            Type::ByName(t) | Type::Repeated(t) => {
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
        if self.take_lazy_done(tree) {
            return;
        }
        // Signature work synthesizes evidence parameters and default getters,
        // so it must run exactly once per member even though the signature
        // pass and the body pass both walk the same tree.
        if tree.id != scala_rs_parser::NodeId(0)
            && matches!(tree.kind, TreeKind::ValDef { .. } | TreeKind::DefDef { .. })
            && !self.sig_done.insert((self.file_index, tree.id))
        {
            return;
        }
        match &tree.kind {
            TreeKind::ValDef { .. } => self.type_val_sig(tree),
            TreeKind::DefDef { .. } => self.type_def_sig(tree),
            TreeKind::ClassDef { .. } => self.type_class(tree),
            TreeKind::ModuleDef { .. } => self.type_module(tree),
            TreeKind::TypeDef { .. } => self.type_type_member(tree),
            _ => {}
        }
        if matches!(
            &tree.kind,
            TreeKind::ValDef { .. } | TreeKind::DefDef { .. }
        ) {
            self.register_typed_sig(tree);
        }
    }

    fn type_member_body(&mut self, tree: &mut Tree) {
        if self.take_lazy_done(tree) {
            return;
        }
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

    pub(crate) fn type_val_sig(&mut self, tree: &mut Tree) {
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

    pub(crate) fn type_val_body(&mut self, tree: &mut Tree) {
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
        // `var x: T = _` is the zero of `T` (nsc's default initializer).
        if matches!(rhs.kind, TreeKind::Wildcard) {
            if declared.is_no_type() {
                self.error(tree.span, "unbound placeholder parameter");
                tree.ty = Type::Error;
                return;
            }
            let lit = match declared.widen_constant() {
                Type::Int => Lit::Int(0),
                Type::Long => Lit::Long(0),
                Type::Double => Lit::Double(0.0),
                Type::Float => Lit::Float(0.0),
                Type::Short | Type::Byte | Type::Char => Lit::Int(0),
                Type::Boolean => Lit::Boolean(false),
                Type::Unit => Lit::Unit,
                _ => Lit::Null,
            };
            let span = rhs.span;
            **rhs = Tree {
                id: rhs.id,
                span,
                kind: TreeKind::Literal { lit },
                ty: declared.clone(),
                sym: SymbolId::NONE,
                postfix: false,
            };
            self.type_expr(rhs, &declared);
            tree.ty = declared;
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
        // The signature is settled; nothing may complete this value again.
        // Unlike a method it is *not* locked while its own right-hand side is
        // typed: scalac reports `val x = y; val y = x` on the reference to `y`.
        self.drop_lazy_sig(tree.sym);
        self.check_stored_annotations(tree);
    }

    pub(crate) fn type_def_sig(&mut self, tree: &mut Tree) {
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
        let mut bound_work: Vec<(SymbolId, Option<Tree>, Option<Tree>)> = Vec::new();
        for (i, tp) in tparams.iter().enumerate() {
            if let TreeKind::TypeDef {
                views,
                ctx_bounds,
                tparams: inner,
                lo,
                hi,
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
                if lo.is_some() || hi.is_some() {
                    bound_work.push((
                        tp_ids[i],
                        lo.as_ref().map(|t| (**t).clone()),
                        hi.as_ref().map(|t| (**t).clone()),
                    ));
                }
            }
        }
        // `[B >: A]` / `[A <: Named]`: remember the bounds on the type parameter
        // symbol so inference can widen to the lower bound and check the upper one.
        for (tp_id, lo, hi) in bound_work {
            let lo_ty = lo.map(|t| self.tree_to_type(&t));
            let hi_ty = hi.map(|t| self.tree_to_type(&t));
            if let Some(t) = lo_ty {
                if !t.is_error() {
                    self.st.get_mut(tp_id).bound_lo = Some(t);
                }
            }
            if let Some(t) = hi_ty {
                if !t.is_error() {
                    self.st.get_mut(tp_id).bound_hi = Some(t);
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
                    // nsc: `xs: T*` is a `Seq[T]` inside the body; the method
                    // type keeps `Repeated` so call sites still wrap arguments.
                    let sym_ty = match &p.ty {
                        Type::Repeated(inner) => self
                            .seq_of(inner)
                            .unwrap_or_else(|| Type::Repeated(inner.clone())),
                        other => other.clone(),
                    };
                    self.st.get_mut(p.sym).ty = sym_ty;
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
        for (tp_id, bounds, span, _hk) in ctx_work {
            // nsc 2.13.16 accepts a context bound on a higher-kinded parameter
            // (`def f[F[_]: Async]` desugars to `(implicit ev: Async[F])`); only
            // *view* bounds on such a parameter are rejected.
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

    pub(crate) fn type_def_body(&mut self, tree: &mut Tree) {
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
                self.drop_lazy_sig(tree.sym);
                self.check_stored_annotations(tree);
                return;
            }
            if matches!(rhs.kind, TreeKind::MacroRhs { .. }) {
                // A macro def has no body to type. Record the binding to the
                // implementation instead; the reference is resolved in the
                // enclosing scope, so no parameter scope is pushed.
                self.type_macro_def(tree);
                self.check_stored_annotations(tree);
                return;
            }
            // nsc locks a method while its result type is being inferred, so a
            // definition completed from this body that refers back reports
            // `recursive method f needs result type` at that reference.
            let locked = ret_pt.is_no_type() && self.lock_lazy_sig(tree.sym);
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
            self.unlock_lazy_sig(locked);
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
                let targs: Vec<Type> = match &tree.ty {
                    Type::Class { args, .. } => args.clone(),
                    _ => Vec::new(),
                };
                match self.pick_ctor_at(id, &targs, &[], None) {
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
        // `extends A(1)(2)` arrives as nested Applies; the constructor takes
        // one flat argument list on the JVM, so flatten the clauses.
        loop {
            let flat = match &mut tree.kind {
                TreeKind::Apply { fun, args } => match &mut fun.kind {
                    TreeKind::Apply {
                        fun: inner_fun,
                        args: inner_args,
                    } => {
                        let mut all = std::mem::take(inner_args);
                        all.append(args);
                        Some((
                            std::mem::replace(inner_fun, Box::new(Tree::dummy(TreeKind::Empty))),
                            all,
                        ))
                    }
                    _ => None,
                },
                _ => return,
            };
            match flat {
                Some((fun, args)) => tree.kind = TreeKind::Apply { fun, args },
                None => break,
            }
        }
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
        let targs: Vec<Type> = match &class_ty {
            Type::Class { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        match self.pick_ctor_at(class_id, &targs, &arg_tys, None) {
            OverloadPick::Found(sym, param_tys, _) => {
                // `class Sub[T](y: T) extends Base[T](y)`: the constructor's
                // parameters are stated in `Base`'s own `T`. Check the
                // arguments at the type arguments the `extends` clause
                // actually wrote -- otherwise both sides of the mismatch
                // print `T` and neither one is the other.
                let param_tys: Vec<Type> = match &class_ty {
                    Type::Class { args: targs, .. } if !targs.is_empty() => param_tys
                        .iter()
                        .map(|p| self.st.subst_tparams(class_id, targs, p))
                        .collect(),
                    _ => param_tys,
                };
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
        self.pick_ctor_at(class_id, &[], arg_tys, skip)
    }

    /// `targs` are the type arguments the constructor is applied at
    /// (`extends TypedRep[T](tt)`): a parameter declared as `TT[T]` in terms of
    /// the *parent's* type parameter has to be read as `TT[T]` of the subclass
    /// before the arguments are matched against it.
    fn pick_ctor_at(
        &self,
        class_id: SymbolId,
        targs: &[Type],
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
        // `extends A(1)(2)` and `new A(1)(2)` pass one flat argument list, so a
        // multi-clause constructor is matched against its flattened clauses.
        let flatten = |ty: Type| -> Type {
            let ty = if targs.is_empty() {
                ty
            } else {
                self.st.subst_tparams(class_id, targs, &ty)
            };
            match ty {
                Type::Method { paramss, ret } if paramss.len() > 1 => Type::Method {
                    paramss: vec![paramss.into_iter().flatten().collect()],
                    ret,
                },
                other => other,
            }
        };
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
                flatten(ty)
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
                            flatten(ty)
                        }
                    })
                    .collect(),
            )
        };
        match self.resolve_overload(&fun_ty, fun_sym, arg_tys, &Type::NoType) {
            OverloadPick::Found(sym, _, _) if Some(sym) == skip => OverloadPick::None,
            // `resolve_overload` re-reads the alternatives off their symbols, so
            // the picked clause comes back in terms of the class's own type
            // parameters. Instantiate it here too.
            OverloadPick::Found(sym, ps, ret) if !targs.is_empty() => OverloadPick::Found(
                sym,
                ps.iter()
                    .map(|p| self.st.subst_tparams(class_id, targs, p))
                    .collect(),
                self.st.subst_tparams(class_id, targs, &ret),
            ),
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
            // Local `class` / `object` inside a block. `type_expr` routes these
            // back here, so they must not fall through to it again.
            TreeKind::ClassDef { .. } => {
                if tree.sym.is_none() {
                    self.namer_class(tree);
                }
                self.type_class(tree);
            }
            TreeKind::ModuleDef { .. } => {
                if tree.sym.is_none() {
                    self.namer_module(tree);
                }
                self.type_module(tree);
            }
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
        let span = tree.span;
        match &mut expr.kind {
            TreeKind::Select { qual, name } if name == "_" => {
                let owners = self.import_prefix(qual, span);
                self.import_wildcard(&owners, &[], span);
            }
            TreeKind::Select { qual, name } if name.starts_with('{') => {
                let sels = decode_import_selectors(name);
                let owners = self.import_prefix(qual, span);
                let hidden: Vec<String> = sels
                    .iter()
                    .filter(|(from, to)| from != "_" && to == "_")
                    .map(|(from, _)| from.clone())
                    .collect();
                for (from, to) in &sels {
                    if from == "_" || to == "_" {
                        continue;
                    }
                    self.import_named(&owners, from, to, span);
                }
                if sels.iter().any(|(from, _)| from == "_") {
                    self.import_wildcard(&owners, &hidden, span);
                }
            }
            TreeKind::Select { qual, name } => {
                let n = name.clone();
                let owners = self.import_prefix(qual, span);
                self.import_named(&owners, &n, &n, span);
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

    /// The symbol whose members an import's selectors name.
    ///
    /// Packages, objects and package objects are resolved symbolically, one
    /// path segment at a time, so a jar-only package such as `cats.syntax`
    /// never has to be typed as an expression (it has no type). Only a real
    /// term prefix (`import someVal.field._`) falls back to the typer.
    fn import_prefix(&mut self, qual: &mut Tree, span: Span) -> Vec<SymbolId> {
        let syms = self.import_path_syms(qual, span);
        if !syms.is_empty() {
            qual.sym = syms[0];
            return syms.into_iter().map(|s| self.as_type_owner(s)).collect();
        }
        self.type_expr(qual, &Type::NoType);
        if !qual.sym.is_none() {
            let id = qual.sym;
            return vec![match self.st.get(id).kind {
                SymKind::Module | SymKind::ModuleClass => self.st.module_class_of(id),
                _ => id,
            }];
        }
        match self.st.class_sym_of(&qual.ty) {
            Some(c) => vec![self.st.module_class_of(c)],
            None => Vec::new(),
        }
    }

    /// Resolve `a.b.c` to package / object / class symbols without typing it.
    ///
    /// More than one can answer to the same name: a `case class C` whose
    /// `object C` is named later in the file gets a synthetic companion of its
    /// own, so `import C.member` has to look in both.
    fn import_path_syms(&mut self, t: &Tree, span: Span) -> Vec<SymbolId> {
        match &t.kind {
            TreeKind::Ident { name } if name == "_root_" => vec![self.st.root],
            TreeKind::Ident { name } => {
                self.expose_unqualified(name, span);
                let mut found = self.st.lookup(name);
                if found.is_empty() {
                    found = self.st.lookup_member(self.st.root, name);
                }
                self.rank_import_prefixes(found)
            }
            TreeKind::Select { qual, name } => {
                for owner in self.import_path_syms(qual, span) {
                    let owner = self.as_type_owner(owner);
                    self.complete_binary_member(owner, name, span);
                    let found = self.st.lookup_member(owner, name);
                    let ranked = self.rank_import_prefixes(found);
                    if !ranked.is_empty() {
                        return ranked;
                    }
                    let Some(po) = self.package_object_of(owner, span) else {
                        continue;
                    };
                    self.complete_binary_member(po, name, span);
                    let found = self.st.lookup_member(po, name);
                    let ranked = self.rank_import_prefixes(found);
                    if !ranked.is_empty() {
                        return ranked;
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// An import prefix is a stable identifier, so an object always wins over
    /// a class of the same name (`import scala.util.control.Breaks._` names
    /// the object, not the trait it also inherits from), and a written object
    /// wins over the synthetic companion a `case class` was given.
    fn rank_import_prefixes(&self, found: Vec<SymbolId>) -> Vec<SymbolId> {
        let rank = |k: SymKind| match k {
            SymKind::Package => Some(0),
            SymKind::Module => Some(1),
            SymKind::ModuleClass => Some(2),
            SymKind::Class => Some(3),
            _ => None,
        };
        let mut out: Vec<(u8, u8, SymbolId)> = found
            .into_iter()
            .filter_map(|s| {
                let sym = self.st.get(s);
                let synthetic = u8::from(sym.flags.contains(Flags::SYNTHETIC));
                rank(sym.kind).map(|r| (r, synthetic, s))
            })
            .collect();
        out.sort_by_key(|&(r, syn, s)| (r, syn, s.0));
        out.dedup_by_key(|&mut (_, _, s)| s);
        // Only the best kind answers: a trait and its companion share a name,
        // and `import C._` names the object alone.
        let best = out.first().map(|&(r, _, _)| r);
        out.into_iter()
            .filter(|&(r, _, _)| Some(r) == best)
            .map(|(_, _, s)| s)
            .collect()
    }

    /// `package p { ... }`'s package object, compiled to `p/package$`. Its
    /// members are members of `p` itself. Same-run package objects are folded
    /// into the package by the namer; this covers the ones read from a jar.
    fn package_object_of(&mut self, owner: SymbolId, span: Span) -> Option<SymbolId> {
        if self.st.get(owner).kind != SymKind::Package || owner == self.st.root {
            return None;
        }
        let pkg_jvm = self.st.get(owner).jvm_name.clone();
        if pkg_jvm.is_empty() {
            return None;
        }
        if let Some(id) = self
            .st
            .lookup_member(owner, "package")
            .into_iter()
            .find(|&s| matches!(self.st.get(s).kind, SymKind::Module | SymKind::ModuleClass))
        {
            return Some(self.st.module_class_of(id));
        }
        if !self.load_binary_into(&format!("{pkg_jvm}/package$"), owner, span, true) {
            return None;
        }
        let id = self
            .st
            .lookup_member(owner, "package")
            .into_iter()
            .find(|&s| matches!(self.st.get(s).kind, SymKind::Module | SymKind::ModuleClass))?;
        let mcls = self.st.module_class_of(id);
        // A package object's members are the package's members.
        for mem in self.st.get(mcls).members.clone() {
            if !self.st.get(owner).members.contains(&mem) {
                self.st.get_mut(owner).members.push(mem);
            }
        }
        self.install_pickled_package_aliases(owner, span);
        Some(mcls)
    }

    /// A package object's `type` aliases never reach its classfile: scalac
    /// writes them only into the `ScalaSignature` pickle. Folding
    /// `<pkg>/package$`'s *members* into the package therefore leaves
    /// `scala.NoSuchElementException` and `cats.effect.Ref` unresolvable.
    /// Read them from the pickle and enter them as type members of the
    /// package, which is where source code names them from.
    ///
    /// Lazy by construction: this runs when a package object is first needed,
    /// so no package is read ahead of time. An alias whose right-hand side
    /// cannot be rebuilt is *not* installed -- a type member pointing at
    /// nothing would silently mean `Any` -- but it is remembered, so the name
    /// reports why it is missing instead of a bare "not found".
    fn install_pickled_package_aliases(&mut self, pkg: SymbolId, span: Span) {
        if !self.pkg_aliases_done.insert(pkg.0) {
            return;
        }
        let pkg_jvm = self.st.get(pkg).jvm_name.clone();
        if pkg_jvm.is_empty() {
            return;
        }
        let dotted = pkg_jvm.replace('/', ".");
        let Ok(aliases) = self
            .pickle
            .package_object_aliases(&mut self.binary, &format!("{dotted}.package"))
        else {
            // No pickle (a Java-only package, or one compiled in this run):
            // there is nothing to supply, and nothing is claimed.
            return;
        };
        for a in aliases {
            // Anything already there wins: the hand-written prelude, and any
            // real class of the same name. This only fills a hole.
            if a.name.is_empty() || !self.type_owner_members(pkg, &a.name).is_empty() {
                continue;
            }
            match self.pickled_alias_type(&a, span) {
                Some((id, ty)) => {
                    self.st.get_mut(id).ty = ty;
                    self.st.get_mut(id).owner = pkg;
                    self.st.get_mut(pkg).members.push(id);
                }
                None => {
                    self.pkg_alias_gaps
                        .entry(a.name.clone())
                        .or_insert_with(|| {
                            format!(
                                "not found: type {} -- package object {} declares it as an alias \
                             for {}, which this compiler cannot express",
                                a.name,
                                dotted,
                                scala_rs_pickle::sym::render(&a.rhs)
                            )
                        });
                }
            }
        }
    }

    /// Build the symbol for one pickled package-object alias: its own type
    /// parameters first, then its right-hand side.
    ///
    /// The classes the right-hand side names are loaded from the classpath on
    /// demand. The pickle reader can only reach `scala.*` on its own, so
    /// `cats.effect.kernel.Ref` has to be resolved here, where the whole
    /// classpath is available; each round loads what the last one asked for
    /// and tries again, and stops as soon as a round resolves nothing new.
    ///
    /// The symbol is allocated ownerless, so declining leaves the package
    /// untouched.
    fn pickled_alias_type(
        &mut self,
        a: &crate::pickle_supply::PickledAlias,
        span: Span,
    ) -> Option<(SymbolId, Type)> {
        let id = self.st.alloc(
            &a.name,
            SymbolId::NONE,
            SymKind::TypeMember,
            Flags::EMPTY,
            "",
        );
        let mut scope: HashMap<String, Type> = HashMap::new();
        let mut tps = Vec::new();
        for tp in &a.tparams {
            let t = self
                .st
                .alloc(&tp.name, id, SymKind::TypeParam, Flags::EMPTY, "");
            self.st.get_mut(t).ty = Type::TypeParam(t);
            // `type Ref[F[_], A]`: `F` is itself a constructor, and getting its
            // arity right is what keeps `Ref[F, A]` from reporting "does not
            // take type parameters" at the use site.
            let inner: Vec<SymbolId> = (0..tp.arity)
                .map(|i| {
                    let x =
                        self.st
                            .alloc(format!("_${i}"), t, SymKind::TypeParam, Flags::EMPTY, "");
                    self.st.get_mut(x).ty = Type::TypeParam(x);
                    x
                })
                .collect();
            self.st.get_mut(t).tparams = inner;
            scope.insert(tp.name.clone(), Type::TypeParam(t));
            tps.push(t);
        }
        self.st.get_mut(id).tparams = tps;
        let mut asked: HashSet<String> = HashSet::new();
        for _ in 0..8 {
            if let Some(ty) =
                self.pickle
                    .convert_pickled_type(&mut self.st, &mut self.binary, &scope, &a.rhs)
            {
                return Some((id, ty));
            }
            let mut progress = false;
            for m in self.pickle.take_unresolved_refs() {
                if asked.insert(m.clone()) && self.resolve_dotted_class(&m, span).is_some() {
                    progress = true;
                }
            }
            if !progress {
                return None;
            }
        }
        None
    }

    /// The class a pickled dotted name denotes (`cats.effect.kernel.Ref`),
    /// loading it from the classpath if it is not in the table yet.
    ///
    /// Walked one segment at a time rather than turned into a JVM name in one
    /// go, so an object in the middle of the path
    /// (`cats.effect.kernel.Par.ParallelF`) comes out as the nested class it
    /// is, not as a package that does not exist.
    fn resolve_dotted_class(&mut self, dotted: &str, span: Span) -> Option<SymbolId> {
        let mut cur = self.st.root;
        for seg in dotted.split('.') {
            if seg.is_empty() {
                return None;
            }
            let owner = self.as_type_owner(cur);
            // The classfile this segment names, given where the walk is: a
            // member of a package is `p/Seg`, one nested in a class `Outer$Seg`.
            let want = if owner == self.st.root {
                seg.to_string()
            } else {
                let base = self
                    .st
                    .get(owner)
                    .jvm_name
                    .trim_end_matches('$')
                    .to_string();
                match self.st.get(owner).kind {
                    SymKind::Package => format!("{base}/{seg}"),
                    _ => format!("{base}${seg}"),
                }
            };
            self.complete_binary_member(owner, seg, span);
            let mut found = self.type_owner_members(owner, seg);
            // A companion's classfile can already hold the simple name with
            // the module class's JVM name (`Outcome` -> `.../Outcome$`), which
            // carries none of the trait's type parameters. Insist on the
            // classfile the path really names, and read it if the table has
            // not seen it: a type constructor of the wrong arity would make
            // every use of the alias an error.
            if !found.iter().any(|&s| self.st.get(s).jvm_name == want)
                && self.load_binary_into(&want, owner, span, true)
            {
                found = self.type_owner_members(owner, seg);
            }
            cur = found
                .iter()
                .copied()
                .find(|&s| self.st.get(s).jvm_name == want)
                .or_else(|| found.into_iter().next())?;
        }
        Some(cur)
    }

    /// `import p.n` / `import p.{n => alias}`.
    fn import_named(&mut self, owners: &[SymbolId], from: &str, to: &str, span: Span) {
        let mut entered = false;
        for &owner in owners {
            if owner.is_none() {
                continue;
            }
            self.complete_binary_member(owner, from, span);
            let mut found = self.st.lookup_member(owner, from);
            if found.is_empty() {
                if let Some(po) = self.package_object_of(owner, span) {
                    self.complete_binary_member(po, from, span);
                    found = self.st.lookup_member(po, from);
                }
            }
            for m in found {
                self.st.enter_in_current(to, m);
                entered = true;
            }
        }
        if entered {
            return;
        }
        let Some(&owner) = owners.iter().find(|o| !o.is_none()) else {
            // The prefix itself did not resolve; it reported its own error.
            return;
        };
        // nsc: `import p.Nope` is an error at the selector, not only at the
        // later use of the name.
        self.error(
            span,
            format!("value {from} is not a member of {}", self.owner_desc(owner)),
        );
    }

    /// `package p` / `object O` / `class C`, for a diagnostic.
    fn owner_desc(&self, owner: SymbolId) -> String {
        let s = self.st.get(owner);
        let name = if s.jvm_name.is_empty() {
            s.name.clone()
        } else {
            s.jvm_name.replace('/', ".")
        };
        match s.kind {
            SymKind::Package => format!("package {name}"),
            SymKind::Module | SymKind::ModuleClass => {
                format!("object {}", name.trim_end_matches('$'))
            }
            _ => name,
        }
    }

    /// `import p._` / `import p.*`. Members already known are entered eagerly;
    /// the owner is also recorded so that a name only reachable by reading a
    /// classfile is still found later (see `expose_unqualified`).
    fn import_wildcard(&mut self, owners: &[SymbolId], hidden: &[String], span: Span) {
        let mut all: Vec<SymbolId> = Vec::new();
        for &owner in owners {
            if owner.is_none() || all.contains(&owner) {
                continue;
            }
            all.push(owner);
            if let Some(po) = self.package_object_of(owner, span) {
                if !all.contains(&po) {
                    all.push(po);
                }
            }
        }
        for o in all {
            for m in self.st.get(o).members.clone() {
                let n = self.st.get(m).name.clone();
                if n.ends_with('$') || n == "<init>" || hidden.iter().any(|h| h == &n) {
                    continue;
                }
                self.st.enter_in_current(&n, m);
            }
            self.st.enter_wildcard_in_current(o, hidden);
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
        } else if matches!(&tree.kind, TreeKind::Typed { tpt, .. } if is_eta_marker(tpt)) {
            self.type_eta(tree);
        } else {
            self.type_expr_inner(tree, pt);
        }
        self.adapt_implicit_apply(tree, pt);
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
                    tree.ty = self.st.self_type_of_class(id);
                }
            }
            TreeKind::Select { .. } => self.type_select(tree, pt),
            TreeKind::Apply { .. } => self.type_apply(tree, pt),
            TreeKind::TypeApply { fun, args } => {
                self.type_expr(fun, &Type::NoType);
                let mut targs = Vec::new();
                for a in args.iter_mut() {
                    let t = self.tree_to_type(a);
                    // The backend reads the argument's type for
                    // `isInstanceOf` / `asInstanceOf`.
                    a.ty = t.clone();
                    targs.push(t);
                }
                if !fun.sym.is_none() {
                    let mut sym = fun.sym;
                    let mut base_ty = fun.ty.clone();
                    // `Module[T1, T2]` with no explicit `.apply` written still
                    // means the type args target the module's generic `apply`
                    // factory (`HashMap[String, Int]()`, `List[Int]()`) — the
                    // module symbol itself has no tparams, so naively
                    // substituting against it is a no-op and the caller sees
                    // an un-substituted `HashMap[K, V]`.
                    if self.st.get(sym).tparams.is_empty()
                        && self.st.get(sym).kind == SymKind::Module
                    {
                        let cls = self.st.module_class_of(sym);
                        let candidates: Vec<SymbolId> = self
                            .st
                            .lookup_member(cls, "apply")
                            .into_iter()
                            .filter(|id| self.st.get(*id).tparams.len() == targs.len())
                            .collect();
                        if let [only] = candidates[..] {
                            sym = only;
                            base_ty = self.st.get(sym).ty.clone();
                        }
                    }
                    // nsc (SLS 6.26.3): explicit type arguments first narrow an
                    // overloaded reference to the alternatives that take that
                    // many type parameters. Without this, `f.typed[Boolean](x)`
                    // keeps the whole overload as its type and the implicit
                    // clause is searched for the *uninstantiated* `TT[T]`.
                    if matches!(base_ty, Type::Overload(_)) {
                        if let Some(only) = self.only_alt_with_tparams(sym, targs.len()) {
                            sym = only;
                            base_ty = self.st.get(only).ty.clone();
                        }
                    }
                    tree.sym = sym;
                    tree.ty = self.st.subst_tparams(sym, &targs, &base_ty);
                    // Codegen's `peel_fun` walks straight through this
                    // TypeApply to the underlying Select/Ident and uses
                    // *that* node's `.sym`/`.ty` — propagate the redirect
                    // (module → its `apply` method) down so it sees the
                    // method, not the module itself.
                    if sym != fun.sym {
                        fun.sym = sym;
                        fun.ty = tree.ty.clone();
                    }
                    self.check_explicit_tparam_bounds(fun, &targs, tree.span);
                    match self.st.get(fun.sym).intrinsic {
                        crate::symbol::Intrinsic::AsInstanceOf => {
                            tree.ty = targs.first().cloned().unwrap_or(Type::Any);
                            return;
                        }
                        crate::symbol::Intrinsic::IsInstanceOf => {
                            tree.ty = Type::Boolean;
                            return;
                        }
                        _ => {}
                    }
                } else {
                    tree.ty = fun.ty.clone();
                }
                self.adapt_implicit_apply(tree, pt);
            }
            TreeKind::Block { stats, expr } => {
                self.st.push_scope();
                // Local classes are visible to the whole block, including
                // statements that precede their definition.
                for s in stats.iter_mut() {
                    if matches!(
                        s.kind,
                        TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
                    ) && s.sym.is_none()
                    {
                        self.namer(s);
                    }
                }
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
                // (`Some` stays `Some`, not `Option`). Prefer the expected type when
                // the typer has one; otherwise fall back to `SymbolTable::lub`, which
                // (unlike the old structural-only `lub` below) walks the parent chain
                // — needed for e.g. `if (c) None else Some(x)` with no ascription,
                // whose branches share no direct subtype relation but do share
                // `Option[X]` as a common ancestor (sgap fixture; slick's
                // `PositionedResult.nextXOption()` methods rely on exactly this).
                tree.ty = pt_or_lub(pt, self.st.lub(&thenp.ty, &elsep.ty));
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
                // nsc: `x.f = v` where `f` is a *getter* (not a field) is
                // `x.f_=(v)`. Assigning the field directly compiles and then
                // throws `NoSuchFieldError` at the caller.
                if self.setter_assign_lhs(lhs) {
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
                self.check_reassignment(lhs);
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
                    self.type_new_prefix(tpt);
                } else if let TreeKind::Ident { name } = &tpt.kind {
                    let n = name.clone();
                    self.expose_unqualified(&n, tpt.span);
                    let found = self.st.lookup(&n);
                    if let Some(id) = found
                        .iter()
                        .copied()
                        .find(|s| self.st.get(*s).kind == SymKind::Class)
                    {
                        tpt.sym = id;
                        tpt.ty = Type::Class {
                            sym: id,
                            args: vec![],
                        };
                    } else if let Some(alias) = self.new_alias_target(&found, tpt.span) {
                        // `new A(…)` where `type A = C`: nsc constructs the
                        // alias's right-hand side. The alias symbol has no
                        // constructor of its own, so leaving it bound here
                        // reports "no matching overload for constructor A".
                        // The qualified form (`new p.A(…)`) already dealiases
                        // through `class_sym_of`; this is the unqualified one.
                        tpt.sym = self.st.class_sym_of(&alias).unwrap_or(SymbolId::NONE);
                        tpt.ty = alias;
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
                // `xs: _*` passes the sequence straight through to a repeated
                // parameter instead of wrapping the argument list.
                if matches!(ascr, Type::Repeated(_)) {
                    self.type_expr(expr, &Type::NoType);
                    let elem = match &expr.ty {
                        Type::Class { args, .. } if !args.is_empty() => args[0].clone(),
                        Type::Array(t) => (**t).clone(),
                        _ => Type::Any,
                    };
                    tree.ty = Type::Repeated(Box::new(elem));
                    return;
                }
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
                // nsc takes the lub of the body and the handlers. A body that
                // always throws contributes `Nothing`, so `val n = try throw e
                // catch h` has the handler's type, not `Nothing`.
                tree.ty = if matches!(block.ty, Type::Nothing) {
                    catches
                        .iter()
                        .map(|c| c.body.ty.clone())
                        .reduce(|a, b| self.lub_ty(&a, &b))
                        .unwrap_or_else(|| block.ty.clone())
                } else {
                    block.ty.clone()
                };
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
                    tree.ty = self.super_prefix_type(this_id, parent);
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
    fn java_lang_package(&self) -> Option<SymbolId> {
        let java = self
            .st
            .lookup_member(self.st.root, "java")
            .into_iter()
            .find(|&s| self.st.get(s).kind == SymKind::Package)?;
        self.st
            .lookup_member(java, "lang")
            .into_iter()
            .find(|&s| self.st.get(s).kind == SymKind::Package)
    }

    /// The class type `new <name>` builds when `name` binds a *type alias*.
    ///
    /// `None` for anything else, an abstract `type A <: Bound` included:
    /// `new A` is not a program, and constructing the bound instead would be a
    /// different one.
    fn new_alias_target(&mut self, found: &[SymbolId], span: Span) -> Option<Type> {
        let alias = found
            .iter()
            .copied()
            .find(|&s| self.st.get(s).kind == SymKind::TypeMember)?;
        self.complete_lazy_sig(alias, span);
        let target = self.st.dealias(&Type::TypeMember(alias));
        if matches!(target, Type::TypeMember(_)) {
            return None;
        }
        self.st.class_sym_of(&target).map(|_| target)
    }

    /// `not found: <what> <name>`, unless a package object declares `name` as
    /// an alias we could not rebuild -- then say so, rather than let the user
    /// hunt for a name that is really there.
    fn not_found_error(&mut self, span: Span, what: &str, name: &str) {
        match self.pkg_alias_gaps.get(name).cloned() {
            Some(msg) => self.error(span, msg),
            None => self.error(span, format!("not found: {what} {name}")),
        }
    }

    /// The `scala` package, for the implicit `import scala._`.
    fn scala_package(&self) -> Option<SymbolId> {
        self.st
            .lookup_member(self.st.root, "scala")
            .into_iter()
            .find(|&s| self.st.get(s).kind == SymKind::Package)
    }

    fn expose_unqualified(&mut self, name: &str, span: Span) {
        if name.is_empty() || !self.st.lookup(name).is_empty() {
            return;
        }
        let from = if !self.st.this_class.is_none() {
            self.st.this_class
        } else {
            self.st.owner
        };
        // A name is visible from every enclosing package, not just the
        // innermost: `slick.jdbc.meta` sees `slick.jdbc`'s members.
        let mut pkg = self.enclosing_package(from);
        loop {
            self.complete_binary_member(pkg, name, span);
            for id in self.st.lookup_member(pkg, name) {
                self.st.enter_in_current(name, id);
            }
            if !self.st.lookup(name).is_empty() || pkg == self.st.root {
                break;
            }
            let owner = self.st.get(pkg).owner;
            if owner.is_none() || owner == pkg {
                break;
            }
            pkg = owner;
        }
        let pkg = self.enclosing_package(from);
        if self.st.lookup(name).is_empty() {
            // Every Scala source has an implicit `import scala._`, which ranks
            // above `java.lang._`. Almost every name it offers is already in
            // the prelude, so what this reaches in practice is the `scala`
            // package object's pickled type aliases -- `NoSuchElementException`,
            // `Seq`, `Iterable` -- which `complete_binary_member` installs on
            // the package the first time one is asked for.
            if let Some(sp) = self.scala_package() {
                self.complete_binary_member(sp, name, span);
                for id in self.st.lookup_member(sp, name) {
                    self.st.enter_in_current(name, id);
                }
            }
        }
        if self.st.lookup(name).is_empty() {
            // Every Scala source has an implicit `import java.lang._`.
            if let Some(jl) = self.java_lang_package() {
                self.complete_binary_member(jl, name, span);
                for id in self.st.lookup_member(jl, name) {
                    self.st.enter_in_current(name, id);
                }
            }
        }
        if self.st.lookup(name).is_empty() && pkg != self.st.root {
            self.complete_binary_member(self.st.root, name, span);
            for id in self.st.lookup_member(self.st.root, name) {
                self.st.enter_in_current(name, id);
            }
        }
        if self.st.lookup(name).is_empty() {
            // `import p._` where `p` is a jar package: its classes are read one
            // at a time, so the name is only reachable now.
            for owner in self.st.wildcard_owners_for(name) {
                self.complete_binary_member(owner, name, span);
                let found = self.st.lookup_member(owner, name);
                if found.is_empty() {
                    continue;
                }
                for id in found {
                    self.st.enter_in_current(name, id);
                }
                break;
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
        // A scope that binds the name only in the *type* namespace does not
        // hide a term of that name further out: `import syntax._` bringing a
        // `type HNil` alias into scope leaves `object HNil` reachable.
        if !found.iter().any(|s| {
            matches!(
                self.st.get(*s).kind,
                SymKind::Module | SymKind::ModuleClass | SymKind::Method | SymKind::Term
            )
        }) {
            let terms = self.st.lookup_term(&name);
            if !terms.is_empty() {
                found = terms;
            }
        }
        if found.is_empty() {
            found = self.st.lookup_member(self.st.root, &name);
        }
        if found.is_empty() {
            self.not_found_error(tree.span, "value", &name);
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
        let ref_span = tree.span;
        for s in found.iter().copied() {
            self.complete_lazy_sig(s, ref_span);
        }
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
            // An inherited member is seen through this class: `find` declared
            // on `Repo[A]` is `(User => Boolean) => Option[User]` inside
            // `class UserStore extends Repo[User]`.
            if !self.st.this_class.is_none() {
                let owner = self.st.get(s).owner;
                if owner != self.st.this_class && !owner.is_none() {
                    let this_ty = Type::Class {
                        sym: self.st.this_class,
                        args: self
                            .st
                            .get(self.st.this_class)
                            .tparams
                            .iter()
                            .map(|t| Type::TypeParam(*t))
                            .collect(),
                    };
                    ty = self.st.subst_as_seen_from(&this_ty, &ty);
                }
            }
            ty = self.maybe_auto_apply(ty, pt);
            if !self.st.this_class.is_none() {
                ty = self.st.expand_type_members(self.st.this_class, &ty);
            }
            tree.ty = ty;
            return;
        }
        // The same member can be reached twice (inherited through two parents,
        // or entered by both a package and its package object). Alternatives
        // that agree on their type are one member, not an overload.
        let first_ty = self.st.get(found[0]).ty.clone();
        if !first_ty.is_no_type() && found.iter().all(|&s| self.st.get(s).ty == first_ty) {
            found.truncate(1);
            let s = found[0];
            tree.sym = s;
            tree.ty = self.maybe_auto_apply(first_ty, pt);
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
                .or_else(|| {
                    found
                        .iter()
                        .copied()
                        .find(|&s| self.is_parameterless_sym(s))
                })
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
                // Alternatives that are the same type are one member, not an
                // overload: a member reaches a class through every inheritance
                // route it has, and a diamond yields it twice.
                let mut distinct: Vec<&Type> = Vec::new();
                for a in alts {
                    if !distinct.contains(&a) {
                        distinct.push(a);
                    }
                }
                if let [only] = distinct.as_slice() {
                    return self.maybe_auto_apply((*only).clone(), pt);
                }
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
                    return (**ret).clone();
                }
                // nsc (SLS 6.26.3): an overloaded reference in value position
                // keeps only the alternatives that take no parameters. A `val`
                // is not a method type at all, so `object Lib { val == = … }`
                // reads as the value, not as the inherited `Any.==(x: Any)`.
                let mut values = alts.iter().filter(|a| !matches!(a, Type::Method { .. }));
                match (values.next(), values.next()) {
                    (Some(v), None) if nullary.is_empty() => v.clone(),
                    _ => ty,
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

    /// The alternatives `maybe_auto_apply` keeps in value position: a nullary
    /// method or a `val`/`object` whose type is not a method type at all.
    fn is_parameterless_sym(&self, id: SymbolId) -> bool {
        !matches!(&self.st.get(id).ty, Type::Method { .. }) || self.is_nullary_method_sym(id)
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
        // Wait for TypeApply when the method still has unsubstituted tparams —
        // otherwise `implicitly[Int]` types the Ident first and searches for
        // `T`. The exception is nsc's *undetermined* parameters: when every
        // one of them sits strictly inside an implicit parameter's type
        // (`toMap[K, V](implicit ev: A <:< (K, V))`), only the witness can pin
        // them down, and the search does it.
        let undet = self.undetermined_tparams(tree, &first);
        if !self.st.get(tree.sym).tparams.is_empty()
            && !matches!(tree.kind, TreeKind::TypeApply { .. })
            && undet.is_empty()
        {
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
        // With undetermined parameters, commit only when the witness really
        // pins every one of them down. Otherwise leave the tree alone: a
        // `show[Int]` still has its `TypeApply` coming.
        let mut solved: Vec<(SymbolId, Type)> = Vec::new();
        let mut tys = tys;
        let mut ret = ret;
        if !undet.is_empty() {
            let Some(sol) = self.undet_solution(&tys, &undet) else {
                return;
            };
            let ids: Vec<SymbolId> = sol.iter().map(|(i, _)| *i).collect();
            let ts: Vec<Type> = sol.iter().map(|(_, t)| t.clone()).collect();
            tys = tys
                .iter()
                .map(|t| crate::symbol::subst_tparams_slice(&ids, &ts, t))
                .collect();
            ret = crate::symbol::subst_tparams_slice(&ids, &ts, &ret);
            solved = sol;
        }
        let mut args = Vec::new();
        self.fill_implicit_params(span, &mut args, &tys, &first);
        let mut inner = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
        if !solved.is_empty() {
            let ids: Vec<SymbolId> = solved.iter().map(|(i, _)| *i).collect();
            let ts: Vec<Type> = solved.iter().map(|(_, t)| t.clone()).collect();
            inner.ty = crate::symbol::subst_tparams_slice(&ids, &ts, &inner.ty);
        }
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

    /// Solve `undet` from the implicit parameter types alone, without emitting
    /// anything. `None` when a parameter has no (or no unique) witness, or when
    /// the witness leaves a parameter open — the caller then leaves the tree be.
    fn undet_solution(
        &self,
        param_tys: &[Type],
        undet: &[SymbolId],
    ) -> Option<Vec<(SymbolId, Type)>> {
        let mut solved: Vec<(SymbolId, Type)> = Vec::new();
        for pty in param_tys {
            let ids: Vec<SymbolId> = solved.iter().map(|(i, _)| *i).collect();
            let ts: Vec<Type> = solved.iter().map(|(_, t)| t.clone()).collect();
            let pty = crate::symbol::subst_tparams_slice(&ids, &ts, pty);
            let open: Vec<SymbolId> = undet
                .iter()
                .copied()
                .filter(|u| solved.iter().all(|(s, _)| s != u))
                .collect();
            let (found, bindings) = self.search_implicit_undet(&pty, &open, 0);
            if !found.is_found() {
                return None;
            }
            solved.extend(bindings);
        }
        undet
            .iter()
            .all(|u| solved.iter().any(|(s, _)| s == u))
            .then_some(solved)
    }

    /// nsc's undetermined type parameters: those of `tree.sym` that appear
    /// *strictly inside* an implicit parameter's type, so the implicit search
    /// is the only thing that can solve them. A parameter whose type is the
    /// bare type parameter itself (`implicitly[T](implicit e: T)`) is not one:
    /// every implicit in scope would match it.
    fn undetermined_tparams(&self, tree: &Tree, first: &[SymbolId]) -> Vec<SymbolId> {
        let tps = self.st.get(tree.sym).tparams.clone();
        if tps.is_empty() || matches!(tree.kind, TreeKind::TypeApply { .. }) {
            return Vec::new();
        }
        let ptys: Vec<Type> = match &tree.ty {
            Type::Method { paramss, .. } => paramss.first().cloned().unwrap_or_default(),
            _ => return Vec::new(),
        };
        if ptys.len() != first.len() || ptys.iter().any(|t| matches!(t, Type::TypeParam(_))) {
            return Vec::new();
        }
        if tps
            .iter()
            .all(|tp| ptys.iter().any(|t| type_mentions_tparam(t, *tp)))
        {
            tps
        } else {
            Vec::new()
        }
    }

    /// Whether a selection's qualifier names a *type* (or package / module)
    /// rather than a value. `java.lang.Integer.valueOf(3)` selects through the
    /// class symbol; `b.intValue` selects through a `val`. Only the first may
    /// reach a Java `static` member, exactly as in nsc, where those live on the
    /// companion object.
    fn is_type_qualifier(&self, qual: &Tree) -> bool {
        if qual.sym.is_none() {
            return false;
        }
        matches!(
            self.st.get(qual.sym).kind,
            SymKind::Class
                | SymKind::Module
                | SymKind::ModuleClass
                | SymKind::Package
                | SymKind::TypeMember
        )
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
        // nsc: `x.m` on `x: A` where `A <: T` resolves against `T`.
        // An alias member (`type Scope = Map[K, V]`) is dealiased first, or the
        // receiver's type arguments would be invisible to the substitution below.
        let mut recv_ty = self.st.dealias(&self.st.widen_type_param(&qual.ty));
        let refined_term = match &recv_ty {
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
            let mty = self.st.expand_in_type(&recv_ty, &mty);
            tree.ty = self.maybe_auto_apply(mty, pt);
            return;
        }
        // String concatenation via any2stringadd: handled at Apply of +
        let mut found = Vec::new();
        if let Type::Refined { parents, .. } = &recv_ty {
            for p in parents {
                if let Some(o) = self.st.class_sym_of(p) {
                    found.extend(self.st.lookup_member(o, &name));
                }
            }
        }
        // nsc: "Static Java members belong to companion objects in Scala; they
        // are not inherited". `b.parseInt("12")` on a `java.lang.Integer` value
        // is an error in scalac, and letting statics through here is not merely
        // lax: `java.lang.Integer.max(int,int)` competed with `RichInt.max` for
        // `1.max(2)` and left the extension search with no winner.
        let instance_receiver = !self.is_type_qualifier(qual);
        if found.is_empty() {
            if let Some(o) = self.st.class_sym_of(&recv_ty) {
                found = self.st.lookup_member(o, &name);
                if instance_receiver {
                    found.retain(|&m| !self.st.get(m).flags.contains(Flags::STATIC));
                }
                if found.is_empty() && matches!(&recv_ty, Type::Class { .. } | Type::ModuleRef(_)) {
                    // `asList(...).size()`: the receiver type is a Java stub until
                    // the classfile is completed. `qual.sym` is the method, not List.
                    // Skip `Type::String` / primitives so StringOps / RichChar views
                    // are not shadowed by `java.lang.String` / `Character` overloads.
                    self.ensure_java_loaded(o, tree.span);
                    found = self.st.lookup_member(o, &name);
                    if instance_receiver {
                        found.retain(|&m| !self.st.get(m).flags.contains(Flags::STATIC));
                    }
                }
            }
        }
        // Module: members of module class
        if found.is_empty() {
            if let Type::ModuleRef(id) = &recv_ty {
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
            if let Some((conv, member, to)) = self.search_extension(&recv_ty, &name, tree.span) {
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
                // The member now belongs to the conversion's result, so
                // substitution must see `to`, not the original receiver.
                recv_ty = to.clone();
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
            // Last resort: the hand-written prelude does not declare it, so
            // ask the library's own pickle. Nothing above has matched, so this
            // can only add members, never shadow one.
            found = self.supply_from_pickle(&recv_ty, &name);
        }
        if found.is_empty() {
            // nsc reports the cause once: a selection on a receiver that is
            // already an error adds nothing.
            if !qual.ty.is_error() {
                self.error(
                    tree.span,
                    format!(
                        "value {name} is not a member of {}",
                        self.st.display_type(&qual.ty)
                    ),
                );
            }
            tree.ty = Type::Error;
            return;
        }
        found = self.drop_overridden(found);
        // `x.toString` finds both `Any.toString` and `Int.toString`; they have
        // the same type, so this is one member, not an ambiguous overload.
        if found.len() > 1 {
            let first_ty = self.st.get(found[0]).ty.clone();
            if found
                .iter()
                .all(|&s| self.st.get(s).ty == first_ty && !first_ty.is_no_type())
            {
                found.truncate(1);
            }
        }
        found.retain(|s| self.accessible(*s, Some(qual.as_ref())));
        self.note_companion_access(&found);
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
        for s in found.iter().copied() {
            self.complete_lazy_sig(s, tree.span);
        }
        let subst_args: Vec<Type> = match &recv_ty {
            Type::Class { args, .. } => args.clone(),
            Type::Tuple(ts) => ts.clone(),
            _ => Vec::new(),
        };
        let subst = |ty: Type| -> Type {
            let ty = self.st.subst_as_seen_from(&recv_ty, &ty);
            if !subst_args.is_empty() {
                if let Some(owner) = found.first().map(|s| self.st.get(*s).owner) {
                    return self.st.subst_tparams(owner, &subst_args, &ty);
                }
            }
            ty
        };
        let expand = |ty: Type| -> Type { self.st.expand_in_type(&recv_ty, &ty) };
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
                    .or_else(|| {
                        found
                            .iter()
                            .copied()
                            .find(|&s| self.is_parameterless_sym(s))
                    })
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
            // A class and its companion object share private access, so at
            // every enclosing level the companion counts as the same scope.
            if c == owner || self.companion_scope(c) == Some(owner) {
                return true;
            }
            c = self.st.get(c).owner;
        }
        false
    }

    /// `nested_in` without the companion rule: true enclosure only.
    fn enclosed_by(&self, current: SymbolId, owner: SymbolId) -> bool {
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

    /// Mark a `private` member read across the companion boundary; the JVM
    /// would reject `ACC_PRIVATE` there, so the backend widens it.
    fn note_companion_access(&mut self, members: &[SymbolId]) {
        for &m in members {
            let s = self.st.get(m);
            if !s.flags.contains(Flags::PRIVATE) || s.flags.contains(Flags::LOCAL) {
                continue;
            }
            let owner = s.owner;
            if !self.enclosed_by(self.st.this_class, owner) {
                self.st.get_mut(m).access_widened = true;
            }
        }
    }

    /// The companion of a class (its module class) or of a module class (the
    /// class of the same name), for access checks.
    fn companion_scope(&self, c: SymbolId) -> Option<SymbolId> {
        let s = self.st.get(c);
        match s.kind {
            SymKind::Class => {
                let m = self.st.companion_module(c)?;
                Some(self.st.module_class_of(m))
            }
            SymKind::ModuleClass => {
                let name = s.name.trim_end_matches('$').to_string();
                let owner = s.owner;
                self.st
                    .get(owner)
                    .members
                    .iter()
                    .copied()
                    .find(|&m| self.st.get(m).kind == SymKind::Class && self.st.get(m).name == name)
            }
            _ => None,
        }
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
        if !qual_ty.is_error() {
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
        }
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

    /// nsc synthesizes `def copy(x: T = this.x, …): C` on a case class.
    /// Rewriting `p.copy(y = 3)` to a constructor call keeps the omitted
    /// fields coming from the receiver without emitting a synthetic method.
    fn try_rewrite_case_copy(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let is_copy = match &tree.kind {
            TreeKind::Apply { fun, .. } => {
                matches!(&fun.kind, TreeKind::Select { name, .. } if name == "copy")
            }
            _ => false,
        };
        if !is_copy {
            return false;
        }
        let class_id = {
            let TreeKind::Apply { fun, .. } = &mut tree.kind else {
                return false;
            };
            let TreeKind::Select { qual, .. } = &mut fun.kind else {
                return false;
            };
            if qual.ty.is_no_type() {
                self.type_expr(qual, &Type::NoType);
            }
            match self.st.class_sym_of(&qual.ty) {
                Some(c) => c,
                None => return false,
            }
        };
        if !self.st.get(class_id).flags.contains(Flags::CASE) {
            return false;
        }
        let fields = self.st.get(class_id).ctor_fields.clone();
        if fields.is_empty() {
            return false;
        }
        // A hand-written `copy` wins over the synthetic one.
        if self
            .st
            .lookup_member(class_id, "copy")
            .iter()
            .any(|&s| !self.st.get(s).flags.contains(Flags::SYNTHETIC))
        {
            return false;
        }
        let span = tree.span;
        let (fun, args) = match std::mem::replace(&mut tree.kind, TreeKind::Empty) {
            TreeKind::Apply { fun, args } => (*fun, args),
            _ => return false,
        };
        let qual = match fun.kind {
            TreeKind::Select { qual, .. } => *qual,
            _ => return false,
        };
        let names: Vec<String> = fields
            .iter()
            .map(|f| self.st.get(*f).name.clone())
            .collect();
        let (slots, extra, ok) = self.named_arg_slots(args, &names);
        if ok {
            for a in extra {
                self.error(a.span, "too many arguments");
            }
        }
        // The receiver is evaluated once, as nsc's `copy$default$n` does.
        let tmp = self.fresh("x$copy");
        let tmp_def = Tree::dummy(TreeKind::ValDef {
            mods: scala_rs_parser::Modifiers::default(),
            name: tmp.clone(),
            tpt: Box::new(Tree::dummy(TreeKind::Empty)),
            rhs: Box::new(qual),
        });
        let mut new_args = Vec::with_capacity(slots.len());
        for (i, slot) in slots.into_iter().enumerate() {
            new_args.push(match slot {
                Some(a) => a,
                None => Tree::dummy(TreeKind::Select {
                    qual: Box::new(Tree::dummy(TreeKind::Ident { name: tmp.clone() })),
                    name: names[i].clone(),
                }),
            });
        }
        let cls_name = self.st.get(class_id).name.clone();
        let new_tree = Tree::dummy(TreeKind::New {
            tpt: Box::new(Tree::dummy(TreeKind::Ident { name: cls_name })),
        });
        let ctor = Tree::dummy(TreeKind::Apply {
            fun: Box::new(new_tree),
            args: new_args,
        });
        tree.kind = TreeKind::Block {
            stats: vec![tmp_def],
            expr: Box::new(ctor),
        };
        tree.span = span;
        self.type_expr(tree, pt);
        true
    }

    fn type_apply(&mut self, tree: &mut Tree, pt: &Type) {
        if self.try_rewrite_case_copy(tree, pt) {
            return;
        }
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
            // `new C(b = 2, a = 1)`: named arguments must be put in parameter
            // order before the constructor overload is picked, since the pick
            // is driven by the argument types.
            if Self::has_named_arg(args) && !self.reorder_named_ctor_args(args, class_id, fun) {
                for a in args.iter_mut() {
                    self.type_expr(a, &Type::NoType);
                }
                tree.ty = Type::Error;
                return;
            }
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
            // Like the method path: type non-lambda args now so the ctor
            // overload can be picked, and leave function literals untyped
            // until their parameter type is known (`new S[String](x => …)`).
            let mut arg_tys: Vec<Type> = Vec::new();
            for a in args.iter_mut() {
                if let TreeKind::Function { vparams, .. } = &a.kind {
                    if is_annotated_lambda(a) {
                        self.type_expr(a, &Type::NoType);
                        arg_tys.push(a.ty.clone());
                        continue;
                    }
                    arg_tys.push(Type::Function {
                        params: vec![Type::NoType; vparams.len()],
                        ret: Box::new(Type::NoType),
                    });
                } else {
                    self.type_expr(a, &Type::NoType);
                    arg_tys.push(a.ty.clone());
                }
            }
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
                            self.unify_tparam_all(*tp, &unify_params, &arg_tys)
                                .filter(|t| !t.is_no_type())
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
                if a.ty.is_no_type() {
                    self.type_expr(a, &p);
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
        if !self.reorder_named_args(args, fun) {
            for a in args.iter_mut() {
                self.type_expr(a, &Type::NoType);
            }
            tree.ty = Type::Error;
            return;
        }

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
                if is_annotated_lambda(a) {
                    self.type_expr(a, &Type::NoType);
                    arg_tys.push(a.ty.clone());
                    continue;
                }
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

        if fun_name == "flatMap" && self.is_array_ops_ty(recv_ty.as_ref()) {
            self.bind_array_ops_flat_map(fun, args, recv_ty.as_ref(), &mut arg_tys);
        }
        let fun_ty = fun.ty.clone();
        let chosen = self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt);
        match chosen {
            OverloadPick::Found(sym, mut param_tys, mut ret) => {
                if !sym.is_none() {
                    fun.sym = sym;
                    tree.sym = sym;
                    // Remaining clauses (`Using.resources(a, b)(f)`) read `fun.ty`.
                    // Leave a Method type, not the Overload that selected this alt.
                    if matches!(&fun.ty, Type::Overload(_)) {
                        fun.ty = self.st.get(sym).ty.clone();
                    }
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
                        let inst = self.infer_method_tparams_in(
                            sym,
                            &param_tys,
                            &arg_tys,
                            recv_ty.as_ref(),
                        );
                        // nsc leaves `tryBreakable { throw … }`'s T undetermined
                        // (Nothing is bottom). `catchBreak { println }` then
                        // instantiates T from the handler, not from Nothing.
                        let inst: Vec<(SymbolId, Type)> = if self.st.get(sym).name == "tryBreakable"
                        {
                            inst.into_iter()
                                .filter(|(_, t)| !matches!(t, Type::Nothing))
                                .collect()
                        } else {
                            inst
                        };
                        // The expected type is a constraint too. Solve it here,
                        // before the implicit clauses are filled: slick's
                        // `def column[T](n: Node)(implicit tt: TypedType[T]): Rep[T]`
                        // gets `T` from nowhere else.
                        let inst = self.add_expected_constraints(sym, &ret, pt, inst);
                        self.check_tparam_bounds(sym, &inst, recv_ty.as_ref(), tree.span, true);
                        if !inst.is_empty() {
                            let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                            let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
                            param_tys = param_tys
                                .iter()
                                .map(|p| crate::symbol::subst_tparams_slice(&tps, &args_t, p))
                                .collect();
                            ret = crate::symbol::subst_tparams_slice(&tps, &args_t, &ret);
                            // The later clauses are read back off `fun.ty` by
                            // `fill_defaults_and_implicits`; leaving it raw
                            // would search `ClassTag[T]` after `T` is known.
                            fun.ty = crate::symbol::subst_tparams_slice(&tps, &args_t, &fun.ty);
                        }
                    }
                }
                if let Some(elem) = recv_ty.as_ref().and_then(|t| self.elem_type(t)) {
                    if matches!(
                        fun_name.as_str(),
                        "map" | "flatMap" | "foreach" | "withFilter" | "pipe" | "tap"
                    ) && !param_tys.is_empty()
                    {
                        if let Type::Function { ret: fr, .. } = &param_tys[0] {
                            // `List.flatMap[B](f: A => IterableOnce[B])`: B is
                            // only determined by the lambda body, so the body
                            // must not be checked against `IterableOnce[B]`.
                            let undetermined =
                                !sym.is_none() && mentions_tparam(fr, &self.st.get(sym).tparams);
                            let fret = if matches!(fr.as_ref(), Type::TypeParam(_)) || undetermined
                            {
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
                let own_tparams = (!sym.is_none()).then(|| self.st.get(sym).tparams.clone());
                for (i, a) in args.iter_mut().enumerate() {
                    let mut p = param_at(&param_tys, i).cloned().unwrap_or(Type::NoType);
                    // `Using.resource(r)(x => 10)`: A only appears in a later
                    // clause. Type the lambda against `R => Any` so the body
                    // is not checked against a raw type parameter.
                    if let Type::Function { params, ret } = &p {
                        if !params.is_empty() && matches!(ret.as_ref(), Type::TypeParam(_)) {
                            p = Type::Function {
                                params: params.clone(),
                                ret: Box::new(Type::Any),
                            };
                        }
                    }
                    // `xs.collect { case … }` is checked against
                    // `PartialFunction[Int, B]` with `B` still open. Nothing
                    // conforms to an uninstantiated parameter, so relax it to
                    // `Any` and let the literal's own result type pin `B`.
                    p = relax_open_tparams(&p, own_tparams.as_deref());
                    if a.ty.is_no_type() {
                        self.type_expr(a, &p);
                    }
                    if !p.is_no_type() {
                        self.adapt(a, &p);
                    }
                    if let TreeKind::Function { body, .. } = &a.kind {
                        let body_ty = body.ty.widen_constant();
                        if let Type::Function { params, ret } = &a.ty {
                            if matches!(ret.as_ref(), Type::Any | Type::NoType | Type::TypeParam(_))
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
                let using_infer = if !sym.is_none() {
                    let s = self.st.get(sym);
                    let n_tps = s.tparams.len();
                    let name = s.name.clone();
                    let owner_jvm = self.st.get(s.owner).jvm_name.clone();
                    n_tps > 0
                        && (name == "resource"
                            || name == "resources"
                            || (name == "apply" && owner_jvm.contains("Using")))
                } else {
                    false
                };
                if using_infer {
                    let now_args: Vec<Type> = args
                        .iter()
                        .map(|a| match &a.ty {
                            Type::Function { params, ret } if params.is_empty() => (**ret).clone(),
                            t => t.clone(),
                        })
                        .collect();
                    let orig_params: Vec<Type> = match &fun.ty {
                        Type::Method { paramss, .. } if !paramss.is_empty() => paramss[0]
                            .iter()
                            .map(|p| match p {
                                Type::ByName(inner) => (**inner).clone(),
                                other => other.clone(),
                            })
                            .collect(),
                        _ => param_tys.clone(),
                    };
                    let inst = self.infer_method_tparams(sym, &orig_params, &now_args);
                    let inst: Vec<(SymbolId, Type)> = inst
                        .into_iter()
                        .filter(|(_, t)| {
                            !matches!(t, Type::Nothing | Type::NoType)
                                && !matches!(t, Type::TypeParam(_))
                        })
                        .collect();
                    if !inst.is_empty() {
                        let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                        let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
                        param_tys = param_tys
                            .iter()
                            .map(|p| crate::symbol::subst_tparams_slice(&tps, &args_t, p))
                            .collect();
                        ret = crate::symbol::subst_tparams_slice(&tps, &args_t, &ret);
                        fun.ty = crate::symbol::subst_tparams_slice(&tps, &args_t, &fun.ty);
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
                if !sym.is_none()
                    && self.st.get(sym).name == "collect"
                    && self.is_array_ops_ty(recv_ty.as_ref())
                {
                    if let Some(a0) = args.first() {
                        let to = match &a0.ty {
                            Type::Class { args, .. } if args.len() >= 2 => args[1].clone(),
                            Type::Function { ret, .. } => (**ret).clone(),
                            _ => Type::NoType,
                        };
                        let to = to.widen_constant();
                        let tps = self.st.get(sym).tparams.clone();
                        if tps.len() == 1 && !to.is_no_type() && !to.is_error() {
                            let inst = vec![to];
                            param_tys = param_tys
                                .iter()
                                .map(|t| crate::symbol::subst_tparams_slice(&tps, &inst, t))
                                .collect();
                            fun.ty = crate::symbol::subst_tparams_slice(&tps, &inst, &fun.ty);
                            ret = crate::symbol::subst_tparams_slice(&tps, &inst, &ret);
                        }
                    }
                }
                if !sym.is_none()
                    && self.st.get(sym).name == "flatMap"
                    && self.is_array_ops_ty(recv_ty.as_ref())
                {
                    if let Some(a0) = args.first() {
                        if let Type::Function { ret: fr, .. } = &a0.ty {
                            let elem = match fr.as_ref() {
                                Type::Class { args, .. } if !args.is_empty() => args[0].clone(),
                                Type::Array(e) => e.as_ref().clone(),
                                other => other.clone(),
                            };
                            let elem = elem.widen_constant();
                            let tps = self.st.get(sym).tparams.clone();
                            if tps.len() == 1 && !elem.is_no_type() && !elem.is_error() {
                                let inst = vec![elem];
                                param_tys = param_tys
                                    .iter()
                                    .map(|t| crate::symbol::subst_tparams_slice(&tps, &inst, t))
                                    .collect();
                                fun.ty = crate::symbol::subst_tparams_slice(&tps, &inst, &fun.ty);
                                ret = crate::symbol::subst_tparams_slice(&tps, &inst, &ret);
                            } else if tps.len() == 2 && !elem.is_no_type() && !elem.is_error() {
                                let bs = fr.as_ref().clone();
                                let inst = vec![bs, elem];
                                param_tys = param_tys
                                    .iter()
                                    .map(|t| crate::symbol::subst_tparams_slice(&tps, &inst, t))
                                    .collect();
                                fun.ty = crate::symbol::subst_tparams_slice(&tps, &inst, &fun.ty);
                                ret = crate::symbol::subst_tparams_slice(&tps, &inst, &ret);
                            }
                        }
                    }
                }
                if !sym.is_none()
                    && self.st.get(sym).name == "catchBreak"
                    && self.is_try_block_ty(recv_ty.as_ref())
                {
                    if let Some(a0) = args.first() {
                        // `tryBreakable`'s T and `TryBlock`'s T are different
                        // symbols. If the op was `Nothing`, ret is still a
                        // method type param; fill it from the handler body.
                        let t = match &a0.kind {
                            TreeKind::Function { body, .. } => body.ty.widen_constant(),
                            _ => unwrap_fn0_or_byname(&a0.ty).widen_constant(),
                        };
                        if !t.is_no_type()
                            && !t.is_error()
                            && matches!(ret, Type::TypeParam(_) | Type::Nothing)
                        {
                            ret = t;
                        }
                    }
                }
                // The first pass typed lambda arguments with no expected type,
                // so a method type parameter that only shows up in a lambda's
                // *result* (`Either.fold[C]`, `Try.fold[U]`, `Option.fold[B]`)
                // is still uninstantiated. Now that the arguments carry their
                // real types, infer it once more.
                if !sym.is_none() {
                    let tps = self.st.get(sym).tparams.clone();
                    if matches!(&ret, Type::TypeParam(id) if tps.contains(id)) {
                        let now: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
                        let inst: Vec<(SymbolId, Type)> = self
                            .infer_method_tparams(sym, &param_tys, &now)
                            .into_iter()
                            .filter(|(_, t)| {
                                !t.is_no_type()
                                    && !t.is_error()
                                    && !matches!(t, Type::Nothing | Type::TypeParam(_))
                            })
                            .collect();
                        if !inst.is_empty() {
                            let ids: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                            let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
                            ret = crate::symbol::subst_tparams_slice(&ids, &args_t, &ret);
                        }
                    }
                }
                let leftover =
                    self.fill_defaults_and_implicits(tree.span, args, &param_tys, fun, pt);
                let method_name = if !sym.is_none() {
                    self.st.get(sym).name.clone()
                } else {
                    fun_name.clone()
                };
                // `::` is `[B >: A](elem: B): List[B]` (see prelude_lowbound);
                // its result comes from ordinary lower-bounded inference.
                if method_name == "->" {
                    if let Some(a0) = args.first() {
                        if let Some(t2) = self
                            .st
                            .lookup("Tuple2")
                            .into_iter()
                            .find(|id| self.st.get(*id).kind == crate::symbol::SymKind::Class)
                        {
                            let k = match &fun.kind {
                                TreeKind::Select { qual, .. } => match &qual.kind {
                                    TreeKind::Apply { args: wargs, .. } => wargs
                                        .first()
                                        .map(|a| a.ty.widen_constant())
                                        .unwrap_or_else(|| qual.ty.widen_constant()),
                                    _ => qual.ty.widen_constant(),
                                },
                                _ => Type::Any,
                            };
                            ret = Type::Class {
                                sym: t2,
                                args: vec![k, a0.ty.widen_constant()],
                            };
                        }
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
                    } else if let Some(t) = self.either_map_result(recv_ty.as_ref(), args) {
                        ret = t;
                    } else if !self.is_with_filter_ty(recv_ty.as_ref()) {
                        if let Some(a0) = args.first() {
                            if let Type::Function { ret: fr, .. } = &a0.ty {
                                // The declared result wins when it names another
                                // class: `Range.map` is an `IndexedSeq`, not a
                                // `Range`.
                                let declared = match &ret {
                                    Type::Class { sym, args } if args.len() == 1 => Some(*sym),
                                    _ => None,
                                };
                                let cls = declared.or_else(|| {
                                    recv_ty
                                        .as_ref()
                                        .and_then(|t| self.st.class_sym_of(t))
                                        .map(|c| self.collection_root(c))
                                });
                                if let Some(cls) = cls {
                                    ret = Type::Class {
                                        sym: cls,
                                        args: vec![fr.as_ref().widen_constant()],
                                    };
                                }
                            }
                        }
                    }
                } else if method_name == "pipe" {
                    if let Some(a0) = args.first() {
                        if let Type::Function { ret: fr, .. } = &a0.ty {
                            let t = fr.as_ref().widen_constant();
                            if !t.is_no_type() && !t.is_error() {
                                ret = t;
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
                            if self.is_array_ops_ty(recv_ty.as_ref()) {
                                ret = Type::Array(Box::new(to.widen_constant()));
                            } else if let Some(cls) = recv_ty
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
                } else if method_name == "zip" {
                    if self.is_array_ops_ty(recv_ty.as_ref()) {
                        if let Some(a0) = args.first() {
                            if let Some(b) = self.elem_type(&a0.ty) {
                                let a = recv_ty
                                    .as_ref()
                                    .and_then(|t| self.elem_type(t))
                                    .unwrap_or(Type::Any);
                                let t2 = self.tuple2_sym();
                                if !t2.is_none() {
                                    ret = Type::Array(Box::new(Type::Class {
                                        sym: t2,
                                        args: vec![a, b.widen_constant()],
                                    }));
                                }
                            }
                        }
                    }
                } else if method_name == "flatMap" {
                    if self.is_array_ops_ty(recv_ty.as_ref()) {
                        if let Some(a0) = args.first() {
                            if let Type::Function { ret: fr, .. } = &a0.ty {
                                let elem = match fr.as_ref() {
                                    Type::Class { args, .. } if !args.is_empty() => args[0].clone(),
                                    Type::Array(e) => e.as_ref().clone(),
                                    other => other.clone(),
                                };
                                ret = Type::Array(Box::new(elem.widen_constant()));
                            }
                        }
                    } else if let Some(a0) = args.first() {
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
                        // `List(Circle(1), Rect(2, 3))` is a `List[Shape]`:
                        // the element type is the lub of every argument.
                        if let Some(elem) = args
                            .iter()
                            .map(|a| a.ty.widen_constant())
                            .reduce(|acc, t| self.lub_ty(&acc, &t))
                        {
                            if let Some(cls) = self
                                .st
                                .lookup(owner_n.trim_end_matches('$'))
                                .into_iter()
                                .find(|id| self.st.get(*id).kind == crate::symbol::SymKind::Class)
                            {
                                // `List(circle, rect)` is a `List[Shape]`, so the
                                // element type is the lub of every argument.
                                ret = Type::Class {
                                    sym: cls,
                                    args: vec![elem],
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
                // An argument that already failed cannot pick an alternative;
                // the cause is reported at the argument.
                if !arg_tys.iter().any(|t| t.is_error()) {
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
                }
                tree.ty = Type::Error;
            }
            OverloadPick::None => {
                if self.rewrite_apply_extension(fun) {
                    let fun_ty = fun.ty.clone();
                    match self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt) {
                        OverloadPick::Found(sym, param_tys, ret) => {
                            fun.sym = sym;
                            tree.sym = sym;
                            let open_tps: Vec<SymbolId> = if sym.is_none() {
                                Vec::new()
                            } else {
                                self.st.get(sym).tparams.clone()
                            };
                            for (i, a) in args.iter_mut().enumerate() {
                                let p = param_at(&param_tys, i).cloned().unwrap_or(Type::NoType);
                                if matches!(a.kind, TreeKind::Function { .. }) || a.ty.is_no_type()
                                {
                                    self.type_expr(a, &p);
                                }
                                let own =
                                    (!sym.is_none()).then(|| self.st.get(sym).tparams.clone());
                                let p = relax_open_tparams(&p, own.as_deref());
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
                if fun_ty.is_error() {
                    // The receiver already failed; do not report it twice.
                } else if !has_apply {
                    self.error(
                        tree.span,
                        format!(
                            "value apply is not a member of {}",
                            self.st.display_type(&fun_ty)
                        ),
                    );
                } else if !arg_tys.iter().any(|t| t.is_error()) {
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
        } else if n == "Left" || n == "Right" {
            self.scala_class_named("Either").unwrap_or(id)
        } else if n == "Success" || n == "Failure" || n == "Try$WithFilter" {
            self.scala_class_named("Try").unwrap_or(id)
        } else {
            id
        }
    }

    /// `e.map(f)` on an `Either[A, B]` keeps the left type: `Either[A, C]`.
    /// `e.left.map(f)` on a `LeftProjection[A, B]` keeps the right type.
    /// Returns `None` for every other receiver so the generic single-parameter
    /// collection rule still applies.
    fn either_map_result(&self, recv_ty: Option<&Type>, args: &[Tree]) -> Option<Type> {
        let Type::Function { ret: fr, .. } = &args.first()?.ty else {
            return None;
        };
        let to = fr.as_ref().widen_constant();
        let (sym, targs) = match recv_ty? {
            Type::Class { sym, args } if args.len() == 2 => (*sym, args),
            _ => return None,
        };
        let name = self.st.get(sym).name.as_str();
        let either = self.scala_class_named("Either")?;
        if is_right_biased_either(&self.st, sym) {
            Some(Type::Class {
                sym: either,
                args: vec![targs[0].clone(), to],
            })
        } else if name == "LeftProjection" {
            Some(Type::Class {
                sym: either,
                args: vec![to, targs[1].clone()],
            })
        } else {
            None
        }
    }

    /// A class symbol from the `scala` package by name (`Either`, `Try`, …).
    fn scala_class_named(&self, name: &str) -> Option<SymbolId> {
        self.st
            .get(self.st.scala_pkg)
            .members
            .iter()
            .copied()
            .find(|id| {
                self.st.get(*id).name == name
                    && self.st.get(*id).kind == crate::symbol::SymKind::Class
            })
    }

    fn is_with_filter_ty(&self, ty: Option<&Type>) -> bool {
        let Some(ty) = ty else {
            return false;
        };
        let Some(id) = self.st.class_sym_of(ty) else {
            return false;
        };
        let n = self.st.get(id).name.as_str();
        n == "WithFilter" || n == "Option$WithFilter" || n == "Try$WithFilter"
    }

    fn is_array_ops_ty(&self, ty: Option<&Type>) -> bool {
        ty.and_then(|t| self.st.class_sym_of(t))
            .is_some_and(|id| self.st.get(id).name == "ArrayOps")
    }

    fn is_try_block_ty(&self, ty: Option<&Type>) -> bool {
        ty.and_then(|t| self.st.class_sym_of(t)).is_some_and(|id| {
            self.st.get(id).name == "TryBlock"
                && self.st.jvm_internal(id) == "scala/util/control/Breaks$TryBlock"
        })
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
            // 2.13's `Either` is right-biased: `map` / `flatMap` / `foreach`
            // see the `B` of `Either[A, B]`, not the `A`.
            Type::Class { sym, args }
                if args.len() == 2 && is_right_biased_either(&self.st, *sym) =>
            {
                Some(args[1].clone())
            }
            Type::Class { sym, .. } if self.st.get(*sym).name == "Range" => Some(Type::Int),
            Type::Class { sym, .. } if self.st.get(*sym).name == "BitSet" => Some(Type::Int),
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

    /// Least upper bound, with the numeric widenings nsc applies before lubbing
    /// (`lub(Int, Long) = Long`).
    pub(crate) fn lub_ty(&self, a: &Type, b: &Type) -> Type {
        if let Some(t) = numeric_widen(a, b).or_else(|| numeric_widen(b, a)) {
            return t;
        }
        self.st.lub(a, b)
    }

    /// Unify `tp` against every argument, not just the first match, and join the
    /// results. Needed for repeated parameters (`List(Circle(1), Rect(2, 3))`)
    /// and for `def f[A](x: A, y: A)`.
    fn unify_tparam_all(&self, tp: SymbolId, params: &[Type], args: &[Type]) -> Option<Type> {
        let mut acc: Option<Type> = None;
        for (i, a) in args.iter().enumerate() {
            let Some(p) = param_at(params, i) else {
                break;
            };
            if let Some(t) = unify_one(tp, p, a) {
                acc = Some(match acc {
                    None => t,
                    Some(prev) => self.lub_ty(&prev, &t),
                });
            }
        }
        acc
    }

    /// nsc's `instantiateExpecting`: the expected type constrains a method's
    /// type parameters just as the arguments do, so `Array("x", "y")` checked
    /// against `Array[AnyRef]` is an `Array[AnyRef]`, and
    /// `Library.CountAll.column(n): Rep[Int]` knows `T = Int` before its
    /// `implicit TypedType[T]` is searched for.
    ///
    /// Merges into the argument solution `inst` rather than replacing it:
    /// nsc prefers the arguments' lower bound, and only an *invariant*
    /// occurrence in the result forces the expected type to win. A covariant
    /// occurrence is a mere upper bound -- `def cov[T]: List[T]` checked
    /// against `List[Any]` leaves `T = Nothing` in nsc, not `Any` -- so it is
    /// not allowed to instantiate anything here either.
    fn add_expected_constraints(
        &self,
        method: SymbolId,
        ret: &Type,
        pt: &Type,
        inst: Vec<(SymbolId, Type)>,
    ) -> Vec<(SymbolId, Type)> {
        if pt.is_no_type() || pt.is_error() || ret.is_no_type() || ret.is_error() {
            return inst;
        }
        let tps = self.st.get(method).tparams.clone();
        if tps.is_empty() {
            return inst;
        }
        let mut found: Vec<(SymbolId, Type, bool)> = Vec::new();
        self.collect_expected(&tps, ret, pt, 1, 0, &mut found);
        if found.is_empty() {
            return inst;
        }
        let mut inst = inst;
        for (tp, ty, strong) in found {
            match inst.iter_mut().find(|(id, _)| *id == tp) {
                // The arguments already pinned it. Only an invariant position
                // overrides them, and only when the argument solution actually
                // conforms -- otherwise the call is ill-typed and the argument
                // type is what the user needs to see in the message.
                Some(slot) => {
                    if strong && slot.1 != ty && self.st.is_sub_type(&slot.1, &ty) {
                        slot.1 = ty;
                    }
                }
                None => inst.push((tp, ty)),
            }
        }
        inst
    }

    /// Walk the method result type against the expected type, recording what
    /// each type parameter is forced to in a non-covariant position.
    /// `variance` is `1` covariant, `-1` contravariant, `0` invariant; the
    /// `bool` marks an invariant occurrence, which outranks the arguments.
    fn collect_expected(
        &self,
        tps: &[SymbolId],
        ret: &Type,
        pt: &Type,
        variance: i8,
        depth: u32,
        out: &mut Vec<(SymbolId, Type, bool)>,
    ) {
        if depth > 12 {
            return;
        }
        match (ret, pt) {
            (Type::Annotated { tpe, .. }, _) => {
                self.collect_expected(tps, tpe, pt, variance, depth + 1, out)
            }
            (_, Type::Annotated { tpe, .. }) => {
                self.collect_expected(tps, ret, tpe, variance, depth + 1, out)
            }
            (Type::TypeParam(id), _) if tps.contains(id) => {
                if variance != 1 {
                    if let Some(t) = self.expected_solution(tps, pt) {
                        out.push((*id, t, variance == 0));
                    }
                }
            }
            // `Array` is invariant, and its element must stay an element:
            // rebuilding it from the expected type as a whole would turn
            // `Type::Array` into `Type::Class { array_sym }`, whose JVM name is
            // the pseudo-name `[java/lang/Object`.
            (Type::Array(a), Type::Array(b)) => self.collect_expected(tps, a, b, 0, depth + 1, out),
            (Type::Array(a), Type::Class { sym, args })
                if *sym == self.st.array_sym && args.len() == 1 =>
            {
                self.collect_expected(tps, a, &args[0], 0, depth + 1, out)
            }
            (
                Type::Function {
                    params: rp,
                    ret: rr,
                },
                Type::Function {
                    params: pp,
                    ret: pr,
                },
            ) if rp.len() == pp.len() => {
                for (a, b) in rp.iter().zip(pp) {
                    self.collect_expected(tps, a, b, flip_variance(variance), depth + 1, out);
                }
                self.collect_expected(tps, rr, pr, variance, depth + 1, out);
            }
            (Type::Tuple(a), Type::Tuple(b)) if a.len() == b.len() => {
                for (x, y) in a.iter().zip(b) {
                    self.collect_expected(tps, x, y, variance, depth + 1, out);
                }
            }
            (Type::Tuple(a), Type::Class { args, .. }) if a.len() == args.len() => {
                for (x, y) in a.iter().zip(args) {
                    self.collect_expected(tps, x, y, variance, depth + 1, out);
                }
            }
            (Type::Class { args, .. }, Type::Tuple(b)) if args.len() == b.len() => {
                for (x, y) in args.iter().zip(b) {
                    self.collect_expected(tps, x, y, variance, depth + 1, out);
                }
            }
            (Type::Class { sym: rs, args: ras }, Type::Class { sym: ps, args: pas }) => {
                if rs == ps {
                    if ras.len() != pas.len() {
                        return;
                    }
                    let tparams = self.st.get(*rs).tparams.clone();
                    for (i, (x, y)) in ras.iter().zip(pas).enumerate() {
                        let v = tparams
                            .get(i)
                            .map(|&tp| {
                                let f = self.st.get(tp).flags;
                                if f.contains(Flags::COVARIANT) {
                                    1
                                } else if f.contains(Flags::CONTRAVARIANT) {
                                    -1
                                } else {
                                    0
                                }
                            })
                            .unwrap_or(0);
                        self.collect_expected(
                            tps,
                            x,
                            y,
                            compose_variance(variance, v),
                            depth + 1,
                            out,
                        );
                    }
                } else if let Some(base) = self.base_type_instance(ret, *ps, 0) {
                    // `def f[T]: List[T]` against `Seq[Any]`: line the result
                    // up with the expected type's class first.
                    if !matches!(&base, Type::Class { sym, .. } if sym == rs) {
                        self.collect_expected(tps, &base, pt, variance, depth + 1, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// The type an expected-type position forces a parameter to, or `None`
    /// when it says nothing usable.
    fn expected_solution(&self, tps: &[SymbolId], pt: &Type) -> Option<Type> {
        let pt = pt.widen_constant();
        match pt {
            Type::NoType
            | Type::Error
            | Type::Nothing
            | Type::Null
            | Type::Wildcard
            | Type::BoundedWildcard { .. }
            | Type::Method { .. }
            | Type::Overload(_)
            | Type::ByName(_)
            | Type::Repeated(_) => return None,
            _ => {}
        }
        // Still open: the expected type is itself expressed in terms of the
        // very parameters being solved.
        if mentions_tparam(&pt, tps) {
            return None;
        }
        Some(unarrayify(&pt, self.st.array_sym))
    }

    /// The declared lower bound of `tp`, as seen from the receiver type
    /// (`List[Rect].::[B >: A]` has `B >: Rect`). `None` when there is no bound
    /// or when it is still expressed in terms of unresolved type parameters.
    fn tparam_lower_bound(
        &self,
        method: SymbolId,
        tp: SymbolId,
        recv: Option<&Type>,
    ) -> Option<Type> {
        let lo = self.st.get(tp).bound_lo.clone()?;
        let lo = match recv {
            Some(Type::Class { args, .. }) if !args.is_empty() => {
                let owner = self.st.get(method).owner;
                self.st.subst_tparams(owner, args, &lo)
            }
            _ => lo,
        };
        if lo.is_no_type()
            || lo.is_error()
            || matches!(lo, Type::Nothing)
            || mentions_any_tparam(&lo)
        {
            return None;
        }
        Some(lo)
    }

    fn infer_method_tparams(
        &self,
        method: SymbolId,
        param_tys: &[Type],
        arg_tys: &[Type],
    ) -> Vec<(SymbolId, Type)> {
        self.infer_method_tparams_in(method, param_tys, arg_tys, None)
    }

    /// Method type-parameter inference. `recv` is the receiver type, used to
    /// read `[B >: A]` lower bounds as seen from the receiver.
    fn infer_method_tparams_in(
        &self,
        method: SymbolId,
        param_tys: &[Type],
        arg_tys: &[Type],
        recv: Option<&Type>,
    ) -> Vec<(SymbolId, Type)> {
        let tps = self.st.get(method).tparams.clone();
        let mut out = Vec::new();
        for tp in tps {
            let inferred = self.unify_tparam_all(tp, param_tys, arg_tys);
            let lo = self.tparam_lower_bound(method, tp, recv);
            match (inferred, lo) {
                (Some(t), Some(lo)) => out.push((tp, self.lub_ty(&t, &lo))),
                (Some(t), None) => out.push((tp, t)),
                (None, Some(lo)) => out.push((tp, lo)),
                (None, None) => {}
            }
        }
        out
    }

    fn as_tuple_args(&self, ty: &Type) -> Option<Vec<Type>> {
        match ty {
            Type::Tuple(ts) => Some(ts.clone()),
            Type::Class { sym, args } if !args.is_empty() => {
                let n = self.st.get(*sym).name.clone();
                (n.starts_with("Tuple") && n[5..].parse::<usize>() == Ok(args.len()))
                    .then(|| args.clone())
            }
            _ => None,
        }
    }

    /// nsc's "inferred type arguments … do not conform to … type parameter bounds".
    fn check_tparam_bounds(
        &mut self,
        method: SymbolId,
        inst: &[(SymbolId, Type)],
        recv: Option<&Type>,
        span: Span,
        inferred: bool,
    ) {
        let tps = self.st.get(method).tparams.clone();
        if tps.is_empty() || inst.is_empty() {
            return;
        }
        let owner = self.st.get(method).owner;
        let recv_args: Vec<Type> = match recv {
            Some(Type::Class { args, .. }) => args.clone(),
            _ => Vec::new(),
        };
        let ids: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
        let vals: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
        let mut bad = false;
        for (tp, actual) in inst {
            if actual.is_error() || actual.is_no_type() {
                continue;
            }
            for (bound, upper) in [
                (self.st.get(*tp).bound_hi.clone(), true),
                (self.st.get(*tp).bound_lo.clone(), false),
            ] {
                let Some(bound) = bound else { continue };
                let bound = if recv_args.is_empty() {
                    bound
                } else {
                    self.st.subst_tparams(owner, &recv_args, &bound)
                };
                let bound = crate::symbol::subst_tparams_slice(&ids, &vals, &bound);
                if bound.is_error() || bound.is_no_type() || mentions_any_tparam(&bound) {
                    continue;
                }
                let ok = if upper {
                    self.st.is_sub_type(actual, &bound)
                } else {
                    self.st.is_sub_type(&bound, actual)
                };
                if !ok {
                    bad = true;
                }
            }
        }
        if !bad {
            return;
        }
        let args_s = inst
            .iter()
            .map(|(_, t)| self.st.display_type(t))
            .collect::<Vec<_>>()
            .join(",");
        let bounds_s = tps
            .iter()
            .map(|tp| self.tparam_bounds_string(*tp))
            .collect::<Vec<_>>()
            .join(",");
        let name = self.st.get(method).name.clone();
        let what = if inferred {
            "inferred type arguments"
        } else {
            "type arguments"
        };
        self.error(
            span,
            format!(
                "{what} [{args_s}] do not conform to method {name}'s type parameter bounds [{bounds_s}]"
            ),
        );
    }

    /// `f[Int](…)` written out: check the explicit type arguments against the
    /// method's declared bounds.
    fn check_explicit_tparam_bounds(&mut self, fun: &Tree, targs: &[Type], span: Span) {
        let sym = fun.sym;
        if self.st.get(sym).kind != crate::symbol::SymKind::Method {
            return;
        }
        let tps = self.st.get(sym).tparams.clone();
        if tps.is_empty() || tps.len() != targs.len() {
            return;
        }
        let recv = match &fun.kind {
            TreeKind::Select { qual, .. } => Some(qual.ty.clone()),
            _ => None,
        };
        let inst: Vec<(SymbolId, Type)> = tps.iter().copied().zip(targs.iter().cloned()).collect();
        self.check_tparam_bounds(sym, &inst, recv.as_ref(), span, false);
    }

    fn tparam_bounds_string(&self, tp: SymbolId) -> String {
        let mut s = self.st.get(tp).name.clone();
        if let Some(lo) = self.st.get(tp).bound_lo.clone() {
            s.push_str(&format!(" >: {}", self.st.display_type(&lo)));
        }
        if let Some(hi) = self.st.get(tp).bound_hi.clone() {
            s.push_str(&format!(" <: {}", self.st.display_type(&hi)));
        }
        s
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

    fn has_named_arg(args: &[Tree]) -> bool {
        args.iter().any(|a| Self::named_arg_parts(a).is_some())
    }

    /// nsc's `NamesDefaults.removeNames`: place `name = value` arguments at
    /// their parameter positions.
    ///
    /// A named argument that already sits at its own position keeps later
    /// positional arguments legal (`f(a = 1, 2)` compiles); one that moves an
    /// argument makes every following positional argument an error. Returns one
    /// slot per parameter plus the positional overflow, which only a repeated
    /// final parameter can absorb.
    fn named_arg_slots(
        &mut self,
        args: Vec<Tree>,
        names: &[String],
    ) -> (Vec<Option<Tree>>, Vec<Tree>, bool) {
        let mut slots: Vec<Option<Tree>> = names.iter().map(|_| None).collect();
        let mut arg_pos: Vec<Option<usize>> = args.iter().map(|_| None).collect();
        let mut extra: Vec<Tree> = Vec::new();
        let mut positional_allowed = true;
        let mut ok = true;
        for (arg_index, a) in args.into_iter().enumerate() {
            let Some((n, rhs)) = Self::named_arg_parts(&a) else {
                if !positional_allowed {
                    self.error(a.span, "positional after named argument.");
                    ok = false;
                } else if arg_index < slots.len() {
                    arg_pos[arg_index] = Some(arg_index);
                    slots[arg_index] = Some(a);
                } else {
                    extra.push(a);
                }
                continue;
            };
            let Some(pos) = names.iter().position(|p| p == &n) else {
                self.error(a.span, format!("unknown parameter name: {n}"));
                ok = false;
                continue;
            };
            match arg_pos.iter().position(|p| *p == Some(pos)) {
                Some(prev) => {
                    self.error(
                        a.span,
                        format!(
                            "parameter '{n}' is already specified at parameter position {}",
                            prev + 1
                        ),
                    );
                    ok = false;
                }
                None => {
                    arg_pos[arg_index] = Some(pos);
                    slots[pos] = Some(rhs);
                }
            }
            if pos != arg_index {
                positional_allowed = false;
            }
        }
        (slots, extra, ok)
    }

    /// The 1-based index a `name$default$n` getter carries: nsc numbers
    /// defaults across *all* parameter clauses, so `f(a, b = 1)(c, d = 2)`
    /// gets `f$default$2` and `f$default$4`, not `$default$2` twice.
    fn default_getter_index(&self, fun: &Tree, param: SymbolId) -> usize {
        let sym = fun.sym;
        if sym.is_none() {
            return 1;
        }
        let s = self.st.get(sym);
        let flat: Vec<SymbolId> = if s.paramss.is_empty() {
            s.params.clone()
        } else {
            s.paramss.iter().flatten().copied().collect()
        };
        flat.iter().position(|&p| p == param).map_or(1, |i| i + 1)
    }

    /// Whether the callee is already erroneous, so named arguments cannot be
    /// resolved and any diagnostic here would only be a cascade.
    fn callee_is_erroneous(&self, fun: &Tree) -> bool {
        matches!(fun.ty, Type::Error) || (fun.sym.is_none() && fun.ty.is_no_type())
    }

    /// Drop the `name =` wrapper without reordering, so an argument whose
    /// parameter could not be resolved is not typed as an assignment to a
    /// non-existent variable.
    fn strip_named_args(args: &mut [Tree]) {
        for a in args.iter_mut() {
            if let Some((_, rhs)) = Self::named_arg_parts(a) {
                *a = rhs;
            }
        }
    }

    /// The first parameter clause of `m`, and whether it ends in a repeated
    /// parameter. A repeated parameter's *symbol* has type `Seq[T]` (its type
    /// inside the body), so only the method type still says `Repeated`.
    fn first_clause_of(&self, m: SymbolId) -> (Vec<SymbolId>, bool) {
        let s = self.st.get(m);
        let ids = if s.paramss.is_empty() {
            s.params.clone()
        } else {
            s.paramss.first().cloned().unwrap_or_default()
        };
        let repeated = match &s.ty {
            Type::Method { paramss, .. } => paramss
                .first()
                .and_then(|c| c.last())
                .is_some_and(|t| matches!(t, Type::Repeated(_))),
            _ => false,
        };
        (ids, repeated)
    }

    /// The types of the named arguments, typed speculatively: the diagnostics
    /// are rolled back and the call site's own trees are untouched, so this
    /// only serves to tell overloaded alternatives apart.
    fn probe_named_arg_types(&mut self, args: &[Tree]) -> Vec<(String, Type)> {
        let named: Vec<(String, Tree)> = args.iter().filter_map(Self::named_arg_parts).collect();
        let mark = self.diags.len();
        let mut out = Vec::with_capacity(named.len());
        for (name, mut rhs) in named {
            // A function literal needs an expected type to say anything useful.
            if matches!(rhs.kind, TreeKind::Function { .. }) {
                out.push((name, Type::NoType));
                continue;
            }
            self.type_expr(&mut rhs, &Type::NoType);
            out.push((name, rhs.ty.clone()));
        }
        self.diags.truncate(mark);
        out
    }

    /// The alternative among `alts` that declares every name the call site used
    /// — nsc narrows an overloaded callee by parameter name, then by argument
    /// type. `h(s: String, n: Int)` and `h(n: Int, s: String)` both declare
    /// `s` and `n`, so the types decide which one `h(n = 1, s = "x")` means.
    fn alt_for_named_args(
        &self,
        alts: &[SymbolId],
        named: &[(String, Type)],
        nargs: usize,
    ) -> Option<(Vec<SymbolId>, bool)> {
        let cands: Vec<(Vec<SymbolId>, bool)> = alts
            .iter()
            .filter(|&&m| self.st.get(m).kind == SymKind::Method)
            .map(|&m| self.first_clause_of(m))
            .filter(|(ids, _)| !ids.is_empty())
            .collect();
        let covers = |ids: &[SymbolId]| {
            named.iter().all(|(n, _)| {
                ids.iter()
                    .any(|i| self.st.get(*i).name.as_str() == n.as_str())
            })
        };
        let conforms = |ids: &[SymbolId]| {
            named.iter().all(|(n, t)| {
                if t.is_no_type() || t.is_error() {
                    return true;
                }
                match ids
                    .iter()
                    .find(|i| self.st.get(**i).name.as_str() == n.as_str())
                {
                    Some(&p) => self.arg_conforms(t, &self.st.get(p).ty, true),
                    None => false,
                }
            })
        };
        let pick = |f: &dyn Fn(&[SymbolId]) -> bool| -> Option<&(Vec<SymbolId>, bool)> {
            cands
                .iter()
                .find(|(ids, _)| ids.len() >= nargs && f(ids))
                .or_else(|| cands.iter().find(|(ids, _)| f(ids)))
        };
        pick(&|ids| covers(ids) && conforms(ids))
            .or_else(|| pick(&covers))
            .or_else(|| cands.first())
            .cloned()
    }

    /// Move each `name = value` into its parameter slot and fill the gaps left
    /// by omitted defaults. Shared by the method, constructor and `apply`
    /// paths; `defaults_inline` inlines a parameter's default expression
    /// instead of calling its `name$default$n` getter, which is what a
    /// constructor needs (there is no receiver yet at `new C(…)`).
    fn place_named_args(
        &mut self,
        args: &mut Vec<Tree>,
        fun: &Tree,
        ids: &[SymbolId],
        repeated_last: bool,
        defaults_inline: bool,
    ) -> bool {
        let names: Vec<String> = ids.iter().map(|id| self.st.get(*id).name.clone()).collect();
        let taken = std::mem::take(args);
        let (slots, extra, ok) = self.named_arg_slots(taken, &names);
        let last = slots.len().saturating_sub(1);
        let mut out = Vec::new();
        for (i, slot) in slots.into_iter().enumerate() {
            if let Some(t) = slot {
                out.push(t);
                continue;
            }
            let pid = ids[i];
            let flags = self.st.get(pid).flags;
            let default_rhs = self.st.get(pid).default_rhs.clone();
            if defaults_inline {
                if let Some(rhs) = default_rhs {
                    out.push(rhs);
                    continue;
                }
            } else if flags.contains(Flags::DEFAULTPARAM) {
                let idx = self.default_getter_index(fun, pid);
                if let Some(filled) = self.default_getter_apply(fun, pid, idx, &out) {
                    out.push(filled);
                } else if let Some(rhs) = default_rhs {
                    out.push(rhs);
                }
                continue;
            }
            if flags.contains(Flags::IMPLICIT) {
                // Leave a hole; `fill_defaults_and_implicits` searches for it.
                break;
            }
            if repeated_last && i == last {
                // `def f(a: Int, rest: Int*)` called as `f(a = 1)`.
                break;
            }
            // nsc reports one error per bad application; the missing slot is a
            // consequence of the name error already reported.
            if ok {
                self.error(
                    fun.span,
                    format!("missing argument for parameter `{}`", names[i]),
                );
            }
        }
        if repeated_last {
            out.extend(extra);
        } else if ok {
            for a in extra {
                self.error(a.span, "too many arguments");
            }
        }
        *args = out;
        ok
    }

    /// `new C(b = 2, a = 1)`. Constructors are picked by argument type, so the
    /// names have to be resolved first — and against the overload that
    /// actually declares them.
    fn reorder_named_ctor_args(
        &mut self,
        args: &mut Vec<Tree>,
        class_id: Option<SymbolId>,
        fun: &Tree,
    ) -> bool {
        let Some(class_id) = class_id else {
            Self::strip_named_args(args);
            return true;
        };
        let alts = self.st.lookup_member(class_id, "<init>");
        let named = if alts.len() > 1 {
            self.probe_named_arg_types(args)
        } else {
            args.iter()
                .filter_map(|a| Self::named_arg_parts(a).map(|(n, _)| (n, Type::NoType)))
                .collect()
        };
        let (ids, repeated_last) = self
            .alt_for_named_args(&alts, &named, args.len())
            .unwrap_or_else(|| (self.st.get(class_id).ctor_fields.clone(), false));
        if ids.is_empty() {
            Self::strip_named_args(args);
            self.error(
                args.first().map(|a| a.span).unwrap_or(fun.span),
                "unimplemented syntax: named arguments (constructor parameters not resolved)",
            );
            return false;
        }
        self.place_named_args(args, fun, &ids, repeated_last, true)
    }

    /// The parameters to map named arguments onto, and whether the clause ends
    /// in a repeated parameter.
    fn named_arg_param_ids(&mut self, fun: &Tree, args: &[Tree]) -> (Vec<SymbolId>, bool) {
        if matches!(fun.ty, Type::Overload(_)) && !fun.sym.is_none() {
            let name = self.st.get(fun.sym).name.clone();
            let owner = self.st.get(fun.sym).owner;
            let mut alts = self.drop_overridden(self.st.lookup_member(owner, &name));
            if alts.is_empty() {
                alts = self.st.lookup(&name);
            }
            let named = self.probe_named_arg_types(args);
            if let Some(found) = self.alt_for_named_args(&alts, &named, args.len()) {
                return found;
            }
        }
        let ids = self.first_clause_ids(fun);
        // `fun.ty` may already have shed earlier clauses (`f(1)(b = 2)`), so
        // match the clause by length rather than taking the first.
        let repeated = match &fun.ty {
            Type::Method { paramss, .. } => paramss
                .iter()
                .find(|c| c.len() == ids.len())
                .and_then(|c| c.last())
                .is_some_and(|t| matches!(t, Type::Repeated(_))),
            _ => false,
        };
        (ids, repeated)
    }

    fn reorder_named_args(&mut self, args: &mut Vec<Tree>, fun: &Tree) -> bool {
        if !Self::has_named_arg(args) {
            return true;
        }
        let (ids, repeated_last) = self.named_arg_param_ids(fun, args);
        if ids.is_empty() {
            // Strip `name =` so the argument is not typed as an assignment to a
            // non-existent variable, which would bury the real error under a
            // "not found: value name" cascade.
            Self::strip_named_args(args);
            if !self.callee_is_erroneous(fun) {
                self.error(
                    args.first().map(|a| a.span).unwrap_or(fun.span),
                    "unimplemented syntax: named arguments (method parameters not resolved)",
                );
            }
            return false;
        }
        self.place_named_args(args, fun, &ids, repeated_last, false)
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
        // A repeated parameter accepts zero arguments (`count()`), so a call
        // that stops right before it is not short at all. Only this clause is
        // settled by that: the clauses after it still need filling, which is
        // what `f()(implicit …)` on `def f(xs: Int*)(implicit t: T)` needs.
        let short_first = args.len() < first.len()
            && !(first.len() - args.len() == 1
                && param_tys
                    .last()
                    .is_some_and(|t| matches!(t, Type::Repeated(_))));
        if short_first {
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
                for pid in rest.iter() {
                    let idx = self.default_getter_index(fun, *pid);
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
        // The method type keeps `Repeated`; the parameter symbols carry `Seq`
        // (their type inside the body), which would not unify with an argument.
        let sig_first: Vec<Type> = match &self.st.get(sym).ty {
            Type::Method { paramss, .. } => paramss.first().cloned().unwrap_or_default(),
            _ => Vec::new(),
        };
        let orig_first: Vec<Type> = if sig_first.len() == first.len() {
            sig_first
        } else {
            first.iter().map(|id| self.st.get(*id).ty.clone()).collect()
        };
        // By-name params are adapted to `() => T`. Infer `T`, not Function0,
        // so later clauses see `R2` rather than `() => R2`.
        let orig_for_infer: Vec<Type> = orig_first
            .iter()
            .map(|p| match p {
                Type::ByName(inner) => (**inner).clone(),
                other => other.clone(),
            })
            .collect();
        let arg_tys: Vec<Type> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                if matches!(orig_first.get(i), Some(Type::ByName(_))) {
                    unwrap_fn0_or_byname(&a.ty)
                } else {
                    a.ty.clone()
                }
            })
            .collect();
        let inst = self.infer_method_tparams(sym, &orig_for_infer, &arg_tys);
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
        prior: &[Tree],
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
        let mut preceding = Self::applied_clause_args(fun);
        preceding.extend_from_slice(prior);
        let preceding = &preceding[..];
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

    /// The arguments of the parameter clauses already applied to `fun`. A
    /// `name$default$n` getter for a later clause takes all of them
    /// (`def f(a: Int)(b: Int = a)` gives `f$default$2(a: Int)`).
    fn applied_clause_args(fun: &Tree) -> Vec<Tree> {
        match &fun.kind {
            TreeKind::Apply { fun, args } => {
                let mut v = Self::applied_clause_args(fun);
                v.extend(args.iter().cloned());
                v
            }
            TreeKind::TypeApply { fun, .. } | TreeKind::Typed { expr: fun, .. } => {
                Self::applied_clause_args(fun)
            }
            _ => Vec::new(),
        }
    }

    fn method_receiver(&self, fun: &Tree) -> Tree {
        match &fun.kind {
            TreeKind::Apply { fun, .. }
            | TreeKind::TypeApply { fun, .. }
            | TreeKind::Typed { expr: fun, .. } => self.method_receiver(fun),
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

    /// The tree for a resolved implicit. A derivation rule
    /// (`implicit def listShow[A](implicit s: Show[A]): Show[List[A]]`) is
    /// applied to its own implicits, which are resolved the same way.
    fn implicit_tree(&mut self, id: SymbolId, pt: &Type, span: Span, depth: usize) -> Tree {
        let (paramss, ret) = match self.implicit_candidate_ty(id) {
            Type::Method { paramss, ret } => (paramss, (*ret).clone()),
            _ => return self.ref_implicit(id, span),
        };
        let tps = self.st.get(id).tparams.clone();
        // The solved type arguments of a polymorphic implicit
        // (`<:<.refl[A]` fitted to `Int <:< Any` gives `A = Int`), so the tree
        // carries the instantiated type rather than the declared `=:=[A, A]`.
        let targs = self
            .implicit_fit_at(id, pt, depth, &[])
            .map(|f| f.targs)
            .or_else(|| self.implicit_targs(id, &ret, pt))
            .unwrap_or_default();
        if paramss.iter().all(|c| c.is_empty()) || depth >= crate::implicits::MAX_IMPLICIT_DEPTH {
            let mut t = self.ref_implicit(id, span);
            if targs.len() == tps.len() && !tps.is_empty() {
                t.ty = crate::symbol::subst_tparams_slice(&tps, &targs, &ret);
            }
            return t;
        }
        let inst = |t: &Type| -> Type {
            if targs.len() == tps.len() && !tps.is_empty() {
                crate::symbol::subst_tparams_slice(&tps, &targs, t)
            } else {
                t.clone()
            }
        };
        let mut tree = Tree {
            id: scala_rs_parser::NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: self.st.get(id).name.clone(),
            },
            ty: inst(&ret),
            sym: id,
            postfix: false,
        };
        for clause in &paramss {
            let mut cargs = Vec::with_capacity(clause.len());
            for p in clause {
                let want = inst(p);
                match self.search_implicit(&want) {
                    ImplicitSearch::Found(inner) => {
                        cargs.push(self.implicit_tree(inner, &want, span, depth + 1))
                    }
                    _ => {
                        let diverged = self.diverged_implicit.borrow().clone();
                        self.error(span, self.missing_implicit_message(&want, diverged));
                        return tree;
                    }
                }
            }
            let ty = tree.ty.clone();
            tree = Tree {
                id: scala_rs_parser::NodeId(0),
                span,
                kind: TreeKind::Apply {
                    fun: Box::new(tree),
                    args: cargs,
                },
                ty,
                sym: id,
                postfix: false,
            };
        }
        tree.ty = inst(&ret);
        tree
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
                    let mut r = self.implicit_tree(id, &pty, span, 0);
                    self.adapt(&mut r, &pty);
                    args.push(r);
                }
                ImplicitSearch::None => {
                    // Read the divergence record before the fallbacks run their
                    // own searches and reset it.
                    let diverged = self.diverged_implicit.borrow().clone();
                    if let Some(ct) = self.classtag_apply_fallback(&pty, span) {
                        args.push(ct);
                    } else if let Some(lam) = self.identity_view(&pty, span) {
                        args.push(lam);
                    } else if let Some(lam) = self.array_wrap_view(&pty, span) {
                        args.push(lam);
                    } else {
                        self.error(span, self.missing_implicit_message(&pty, diverged));
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

    /// Prefer 4-arg `flatMap[BS, B]` when the lambda returns `Array`, else 3-arg.
    fn bind_array_ops_flat_map(
        &mut self,
        fun: &mut Tree,
        args: &mut [Tree],
        recv_ty: Option<&Type>,
        arg_tys: &mut [Type],
    ) {
        let Some(a0) = args.first_mut() else {
            return;
        };
        if matches!(a0.kind, TreeKind::Function { .. }) && a0.ty.is_no_type() {
            let elem = recv_ty.and_then(|t| self.elem_type(t)).unwrap_or(Type::Any);
            let pt = Type::Function {
                params: vec![elem.clone()],
                ret: Box::new(Type::Any),
            };
            self.type_expr(a0, &pt);
            if let TreeKind::Function { body, .. } = &a0.kind {
                let body_ty = body.ty.widen_constant();
                if !body_ty.is_no_type() && !body_ty.is_error() {
                    a0.ty = Type::Function {
                        params: vec![elem],
                        ret: Box::new(body_ty),
                    };
                }
            }
            if let Some(slot) = arg_tys.first_mut() {
                *slot = a0.ty.clone();
            }
        }
        let lambda_ret = match arg_tys.first() {
            Some(Type::Function { ret, .. }) => ret.as_ref(),
            _ => return,
        };
        let want_four = matches!(lambda_ret, Type::Array(_));
        let Some(owner) = recv_ty.and_then(|t| self.st.class_sym_of(t)) else {
            return;
        };
        let methods = self.st.lookup_member(owner, "flatMap");
        let Some(picked) = methods.into_iter().find(|m| {
            let n = self.st.get(*m).tparams.len();
            if want_four {
                n >= 2
            } else {
                n == 1
            }
        }) else {
            return;
        };
        fun.sym = picked;
        let mut ty = self.st.get(picked).ty.clone();
        if let Some(Type::Class { args, .. }) = recv_ty {
            if !args.is_empty() {
                ty = self.st.subst_tparams(owner, args, &ty);
            }
        }
        fun.ty = ty;
    }

    /// nsc `implicit asIterable: Array[Int] => Iterable[Int]` is `Predef.wrapIntArray`.
    fn array_wrap_view(&mut self, pt: &Type, span: Span) -> Option<Tree> {
        let Type::Function { params, ret } = pt else {
            return None;
        };
        if params.len() != 1 {
            return None;
        }
        let Type::Array(elem) = &params[0] else {
            return None;
        };
        if !matches!(elem.as_ref(), Type::Int) {
            return None;
        }
        let wrap = {
            let from_scope = self.st.lookup("wrapIntArray");
            if let Some(id) = from_scope.into_iter().next() {
                id
            } else {
                let cls = match &self.st.get(self.st.predef).ty {
                    Type::ModuleRef(id) => *id,
                    _ => return None,
                };
                self.st
                    .lookup_member(cls, "wrapIntArray")
                    .into_iter()
                    .next()?
            }
        };
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
        let wrap_fun = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: "wrapIntArray".into(),
            },
            ty: self.st.get(wrap).ty.clone(),
            sym: wrap,
            postfix: false,
        };
        let body = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(wrap_fun),
                args: vec![ident],
            },
            ty: to.clone(),
            sym: wrap,
            postfix: false,
        };
        let mut lam = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Function {
                vparams: vec![param],
                body: Box::new(body),
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

    /// The single overloaded alternative of `sym` that takes `n` type
    /// parameters, if there is exactly one.
    fn only_alt_with_tparams(&self, sym: SymbolId, n: usize) -> Option<SymbolId> {
        if sym.is_none() {
            return None;
        }
        let name = self.st.get(sym).name.clone();
        let owner = self.st.get(sym).owner;
        let mut alts = self.drop_overridden(self.st.lookup_member(owner, &name));
        if alts.is_empty() {
            alts = self.st.lookup(&name);
        }
        let mut hits = alts
            .into_iter()
            .filter(|m| self.st.get(*m).tparams.len() == n);
        let first = hits.next()?;
        hits.next().is_none().then_some(first)
    }

    fn resolve_overload(
        &self,
        fun_ty: &Type,
        fun_sym: SymbolId,
        arg_tys: &[Type],
        _pt: &Type,
    ) -> OverloadPick {
        let mut cands: Vec<(SymbolId, Vec<Type>, Type)> = Vec::new();
        // Which parameter clause these candidates come from: `f(a)(b = 1)`
        // applied as `f(1)()` leaves a residual method type whose only clause
        // is the *second* one, and that is where the defaults live.
        let mut clause = 0usize;
        match fun_ty {
            Type::Method { paramss, ret } => {
                let ps = paramss.first().cloned().unwrap_or_default();
                if !fun_sym.is_none() {
                    let all = self.st.get(fun_sym).paramss.len();
                    clause = all.saturating_sub(paramss.len());
                }
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
                .filter(|(sym, ps, _)| self.is_applicable(*sym, clause, ps, arg_tys, false))
                .cloned()
                .collect();
            if !no_view.is_empty() {
                no_view
            } else {
                cands
                    .into_iter()
                    .filter(|(sym, ps, _)| self.is_applicable(*sym, clause, ps, arg_tys, true))
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
                // The same method can reach us by two routes -- a package and
                // its package object both carry `math.max`. Identical
                // signatures are one alternative, not an ambiguity.
                let mut winners = winners;
                winners.dedup_by(|a, b| a.1 == b.1 && a.2 == b.2);
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
        self.is_applicable(SymbolId::NONE, 0, b_ps, a_ps, true)
    }

    fn is_applicable(
        &self,
        sym: SymbolId,
        clause: usize,
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
        if args.len() < params.len()
            && !self.trailing_omissible(sym, clause, args.len(), params.len())
        {
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
                // Narrowing an `Int` literal (`take(3)` on a `Byte` parameter)
                // is a fallback, like a view: without that, `sb.append(42)`
                // would match `append(Char)` as readily as `append(Int)`.
                self.st.narrows_to(arg, param)
                    // nsc view: `wrapString` makes String applicable to Seq.
                    || !matches!(self.search_conversion(arg, param), ImplicitSearch::None)
            }
            None => false,
        }
    }

    fn trailing_omissible(&self, sym: SymbolId, clause: usize, given: usize, total: usize) -> bool {
        if sym.is_none() || given >= total {
            return false;
        }
        let s = self.st.get(sym);
        // `params` is every clause flattened, so a residual clause (`f(1)()`
        // for `def f(a: Int)(b: Int = 2)`) has to be read out of `paramss` or
        // the defaults of the wrong clause are consulted. `pick_ctor` flattens
        // a multi-clause constructor into one clause, though, so fall back to
        // the flat list whenever the clause does not have the expected arity.
        let ids = s
            .paramss
            .get(clause)
            .filter(|c| c.len() >= total)
            .cloned()
            .unwrap_or_else(|| {
                if s.params.is_empty() {
                    s.paramss.first().cloned().unwrap_or_default()
                } else {
                    s.params.clone()
                }
            });
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

    /// `Seq[T]` for a repeated parameter's element type, when the prelude has
    /// `Seq` (it does in both modes).
    fn seq_of(&self, elem: &Type) -> Option<Type> {
        let sym = self
            .st
            .lookup("Seq")
            .into_iter()
            .find(|s| self.st.get(*s).kind == SymKind::Class)?;
        Some(Type::Class {
            sym,
            args: vec![elem.clone()],
        })
    }

    fn arg_score(&self, arg: &Type, param: &Type) -> Option<i32> {
        if let Type::ByName(inner) = param {
            return self.arg_score(arg, inner);
        }
        // A `xs: _*` argument is already the sequence the parameter wants.
        if let Type::Repeated(inner) = arg {
            return self.arg_score(inner, param);
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
        // A `{ case … }` literal reaches overload resolution as a one-parameter
        // function; it inhabits a `PartialFunction[A, B]` parameter
        // (`Try.recover`, `Option.collect`, `List.collect`). A literal that is
        // not typed yet scores better, so `collect` can infer `B` from the
        // case bodies rather than from an open type parameter.
        if let Type::Function { params, ret } = arg {
            if params.len() == 1 && partial_function_type(&self.st, param).is_some() {
                return if params[0].is_no_type() && ret.is_no_type() {
                    Some(6)
                } else {
                    Some(7)
                };
            }
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

    /// Parameter types for an expanded placeholder section `f(_, x)`, read off
    /// the callee's own signature. nsc only does this when every placeholder is
    /// a bare argument of one application and the callee resolves to a single
    /// monomorphic method: `poly(_, 3)` (undetermined type parameters) and
    /// `"abc".substring(_)` (overloaded) keep the missing-parameter error.
    /// Returns `NoType` per parameter when the section does not qualify.
    fn section_param_types(&mut self, vparams: &[Tree], body: &Tree) -> Vec<Type> {
        let none = vec![Type::NoType; vparams.len()];
        let TreeKind::Apply { fun, args } = &body.kind else {
            return none;
        };
        let names: Vec<&str> = vparams.iter().filter_map(|p| p.name()).collect();
        if names.len() != vparams.len() || names.is_empty() {
            return none;
        }
        // Every placeholder must be one of this application's arguments.
        let mut at = vec![usize::MAX; vparams.len()];
        for (ai, a) in args.iter().enumerate() {
            if let TreeKind::Ident { name } = &a.kind {
                if let Some(pi) = names.iter().position(|n| *n == name.as_str()) {
                    at[pi] = ai;
                }
            }
        }
        if at.iter().any(|i| *i == usize::MAX) {
            return none;
        }
        // Probe the callee without keeping any diagnostics it produces.
        let mark = self.diags.len();
        let mut probe = (**fun).clone();
        let dummy_method = Type::Method {
            paramss: vec![],
            ret: Box::new(Type::NoType),
        };
        self.type_expr(&mut probe, &dummy_method);
        self.diags.truncate(mark);
        if probe.sym.is_none() || matches!(probe.ty, Type::Overload(_)) {
            return none;
        }
        if !self.st.get(probe.sym).tparams.is_empty() {
            return none;
        }
        let Type::Method { paramss, .. } = &probe.ty else {
            return none;
        };
        let Some(params) = paramss.first() else {
            return none;
        };
        if params.len() != args.len() {
            return none;
        }
        let mut out = none;
        for (pi, ai) in at.iter().enumerate() {
            let t = match &params[*ai] {
                Type::ByName(inner) => (**inner).clone(),
                other => other.clone(),
            };
            if !t.is_no_type()
                && !t.is_error()
                && !matches!(t, Type::TypeParam(_) | Type::Repeated(_))
            {
                out[pi] = t;
            }
        }
        out
    }

    /// Rewrite `x$pf => x$pf match { … }` into `(x$1, …, x$n) => (x$1, …, x$n)
    /// match { … }`, the way nsc adapts a pattern-matching anonymous function
    /// to a `FunctionN`.
    fn expand_case_block_to_arity(&mut self, vparams: &mut Vec<Tree>, body: &mut Tree, n: usize) {
        let TreeKind::Match { selector, .. } = &mut body.kind else {
            return;
        };
        let span = vparams[0].span;
        let mut names = Vec::new();
        let mut params = Vec::new();
        for i in 0..n {
            self.gensym += 1;
            let name = format!("x$pm{}${}", self.gensym, i);
            names.push(name.clone());
            let mut p = Tree::dummy(TreeKind::ValDef {
                mods: scala_rs_parser::Modifiers {
                    flags: Flags::PARAM,
                    ..Default::default()
                },
                name,
                tpt: Box::new(Tree::dummy(TreeKind::Empty)),
                rhs: Box::new(Tree::dummy(TreeKind::Empty)),
            });
            p.span = span;
            params.push(p);
        }
        let args: Vec<Tree> = names
            .iter()
            .map(|nm| {
                let mut t = Tree::dummy(TreeKind::Ident { name: nm.clone() });
                t.span = span;
                t
            })
            .collect();
        let mut fun = Tree::dummy(TreeKind::Ident {
            name: format!("Tuple{n}"),
        });
        fun.span = span;
        let mut tup = Tree::dummy(TreeKind::Apply {
            fun: Box::new(fun),
            args,
        });
        tup.span = span;
        **selector = tup;
        *vparams = params;
    }

    fn type_function(&mut self, vparams: &mut Vec<Tree>, body: &mut Tree, pt: &Type) -> Type {
        // Only a `{ case … }` literal inhabits a `PartialFunction`; the parser
        // encodes one as `x$pf => x$pf match { … }`. A total function literal
        // must still be rejected, the way nsc rejects
        // `t.recover((x: Int) => x + 1)`.
        // nsc: `{ case (a, b) => … }` where a `FunctionN` is expected takes N
        // parameters and matches the N-tuple of them, not one parameter.
        if is_case_block_literal(vparams, body) {
            if let Some(n) = expected_function_arity(pt) {
                if n > 1 {
                    self.expand_case_block_to_arity(vparams, body, n);
                }
            }
        }
        let pf_result = if is_case_block_literal(vparams, body) {
            partial_function_type(&self.st, pt)
        } else {
            None
        };
        let sam = if pf_result.is_none() {
            self.st.sam_sig(pt)
        } else {
            None
        };
        let (pts, ret_pt) = if let Some((from, to)) = &pf_result {
            // The prelude spells the result of `collect` / `recover` as `Any`
            // because the real signature is polymorphic. Leave it open so the
            // case bodies supply it and `Option[String]` survives.
            let to = if matches!(to, Type::Any) {
                Type::NoType
            } else {
                to.clone()
            };
            (vec![from.clone()], to)
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
        // `xs.collect { case … }` reaches here with `PartialFunction[Int, B]`,
        // `B` still undetermined. Typing the body against a bare type parameter
        // pins it to that parameter (and later to `Any`); let the body speak.
        let ret_pt = if matches!(ret_pt, Type::TypeParam(_)) {
            Type::NoType
        } else {
            ret_pt
        };
        // `f(_, x)`: the expected type says nothing, but `f`'s own signature
        // does. nsc only takes this route for an unambiguous monomorphic
        // callee — `poly(_, 3)` and `"abc".substring(_)` stay errors.
        let from_section = if pts.iter().any(|t| t.is_no_type()) {
            self.section_param_types(vparams, body)
        } else {
            vec![Type::NoType; vparams.len()]
        };
        self.st.push_scope();
        let mut param_tys = Vec::new();
        for (i, p) in vparams.iter_mut().enumerate() {
            self.type_val_sig(p);
            if p.ty.is_no_type() {
                p.ty = pts.get(i).cloned().unwrap_or(Type::NoType);
            }
            if p.ty.is_no_type() {
                p.ty = from_section.get(i).cloned().unwrap_or(Type::NoType);
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
        let body_ty = body.ty.widen_constant();
        self.st.pop_scope();
        if let Some((from, _to)) = &pf_result {
            // Keep the expected `PartialFunction` shape, but fill in a result
            // type the caller still has to infer (`xs.collect { case … }`'s `B`,
            // which arrives here as a bare `Any`) from the case bodies. `B` is
            // covariant, so a more precise result still conforms to `pt`.
            if let Type::Class { sym, .. } = pt {
                if !body_ty.is_no_type() && !body_ty.is_error() {
                    return Type::Class {
                        sym: *sym,
                        args: vec![from.clone(), body_ty],
                    };
                }
            }
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
            res = self.st.lub(&res, &c.body.ty);
            self.st.pop_scope();
        }
        let span = tree.span;
        tree.ty = pt_or_lub(pt, res);
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

    /// `::` is entered both as the class `$colon$colon` and as an alias symbol
    /// carrying its type. Patterns need the real class, which holds the
    /// constructor fields.
    fn follow_class_alias(&self, id: SymbolId) -> SymbolId {
        if !self.st.get(id).ctor_fields.is_empty() {
            return id;
        }
        if let Type::Class { sym, .. } = &self.st.get(id).ty {
            if *sym != id && self.st.get(*sym).kind == SymKind::Class {
                return *sym;
            }
        }
        id
    }

    /// Recover a constructor pattern's type arguments from the scrutinee:
    /// matching `Option[Int]` against `Some` gives `Some[Int]`. Walks the
    /// pattern class's base types to find the scrutinee's class.
    fn pattern_class_args(&self, class_id: SymbolId, sel_ty: &Type) -> Vec<Type> {
        let tps = self.st.get(class_id).tparams.clone();
        if tps.is_empty() {
            return Vec::new();
        }
        // A tuple scrutinee is `Type::Tuple`, which has no class symbol; its
        // element types are the pattern class's arguments directly.
        if let Type::Tuple(ts) = sel_ty {
            if ts.len() == tps.len() {
                return ts.clone();
            }
        }
        let Some(sel_sym) = self.st.class_sym_of(sel_ty) else {
            return Vec::new();
        };
        let self_ty = Type::Class {
            sym: class_id,
            args: tps.iter().map(|t| Type::TypeParam(*t)).collect(),
        };
        let mut cands = vec![self_ty];
        let mut work: Vec<Type> = self
            .st
            .get(class_id)
            .parents
            .iter()
            .map(|p| self.st.subst_tparams(class_id, &[], p))
            .collect();
        let mut seen = std::collections::HashSet::new();
        while let Some(p) = work.pop() {
            let Some(psym) = self.st.class_sym_of(&p) else {
                continue;
            };
            if !seen.insert(psym.0) {
                continue;
            }
            for q in self.st.get(psym).parents.clone() {
                work.push(self.st.subst_as_seen_from(&p, &q));
            }
            cands.push(p);
        }
        for c in cands {
            if self.st.class_sym_of(&c) != Some(sel_sym) {
                continue;
            }
            let args: Vec<Type> = tps
                .iter()
                .map(|tp| unify_one(*tp, &c, sel_ty).unwrap_or(Type::Any))
                .collect();
            if args.iter().any(|a| !matches!(a, Type::Any)) {
                return args;
            }
        }
        Vec::new()
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
                // A `Byte`/`Short`/`Char` scrutinee is an `int` on the stack and
                // the pattern is only compared with `==`, so nsc accepts an
                // `Int` constant there (`case DatabaseMetaData.functionNoTable`
                // against `r.nextShort()`). Demanding conformance to the
                // scrutinee would reject it for no runtime reason.
                let pt = match sel_ty.widen_constant() {
                    Type::Byte | Type::Short | Type::Char => Type::Int,
                    _ => sel_ty.clone(),
                };
                self.type_expr(pat, &pt);
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
                        .map(|s| self.follow_class_alias(s))
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
                // `def unapply(n: Nd) = Some((n.v, n.tag))` has no result type
                // of its own; without completing it the pattern would see
                // `<notype>` and count one sub-pattern instead of two.
                for u in unapply.iter().chain(unapply_seq.iter()) {
                    self.complete_lazy_sig(*u, pat.span);
                }
                let use_ctor = !has_star
                    && class_id.is_some_and(|c| {
                        let s = self.st.get(c);
                        s.flags.contains(Flags::CASE) || !s.ctor_fields.is_empty()
                    });
                if use_ctor {
                    let class_id = class_id.unwrap();
                    let fields = self.st.get(class_id).ctor_fields.clone();
                    // `case Some(x)` on an `Option[Int]` binds `x: Int`: recover
                    // the pattern class's arguments from the scrutinee.
                    let cargs = self.pattern_class_args(class_id, sel_ty);
                    let class_ty = Type::Class {
                        sym: class_id,
                        args: cargs.clone(),
                    };
                    for (i, a) in args.iter_mut().enumerate() {
                        let ft = fields
                            .get(i)
                            .map(|f| self.st.get(*f).ty.clone())
                            .unwrap_or(Type::Any);
                        let ft = if cargs.is_empty() {
                            ft
                        } else {
                            self.st.subst_tparams(class_id, &cargs, &ft)
                        };
                        self.type_pattern(a, &ft);
                    }
                    pat.ty = class_ty;
                    pat.sym = class_id;
                } else if let Some(u) = unapply.filter(|_| !has_star) {
                    let extracted = self.unapply_extracted_types(u);
                    let extracted = self.subst_unapply_tparams(u, sel_ty, extracted);
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
                    let targs = self.pattern_class_targs(c, sel_ty);
                    for (i, a) in args.iter_mut().enumerate() {
                        let ft = fields
                            .get(i)
                            .map(|f| self.st.get(*f).ty.clone())
                            .unwrap_or(Type::Any);
                        let ft = if targs.is_empty() {
                            ft
                        } else {
                            self.st.subst_tparams(c, &targs, &ft)
                        };
                        self.type_pattern(a, &ft);
                    }
                    pat.ty = Type::Class {
                        sym: c,
                        args: targs,
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

    /// The class whose parents a bare `super` names. Normally the enclosing
    /// class; inside a template's own parent list it is the class *around*
    /// that template, since a class cannot name its own `super` there.
    fn super_owner(&self, qual: Option<&str>) -> SymbolId {
        let base = match self.parent_ctx {
            Some((inner, outer)) if inner == self.st.this_class => outer,
            _ => self.st.this_class,
        };
        match qual {
            Some(name) => self.st.enclosing_class_named(base, name).unwrap_or(base),
            None => base,
        }
    }

    /// The type `super` stands for: the ancestor *as seen from* the current
    /// class, not the ancestor named on its own.
    ///
    /// `class Sub[A] extends Act[A]` inherits `Act`'s members at `A`; naming
    /// the bare class instead leaves `Act`'s own `R` in the member types, and
    /// `super.id` then reads as `Act[R]` where `Act[A]` is wanted — a mismatch
    /// whose two sides print the same when `Sub` also happens to call its
    /// parameter `R`.
    fn super_prefix_type(&self, this_id: SymbolId, parent: SymbolId) -> Type {
        let self_ty = self.st.self_type_of_class(this_id);
        self.st
            .base_type_seq(&self_ty)
            .into_iter()
            .find(|t| matches!(t, Type::Class { sym, args } if *sym == parent && !args.is_empty()))
            .unwrap_or_else(|| self.st.type_of_class(parent))
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

    /// The instance of `target` among `ty`'s base classes: `Some[Int]` seen as
    /// `Option` is `Option[Int]`. `None` when `target` is not a base class.
    pub(crate) fn base_type_instance(
        &self,
        ty: &Type,
        target: SymbolId,
        depth: u32,
    ) -> Option<Type> {
        if depth > 16 {
            return None;
        }
        let Type::Class { sym, args } = ty else {
            return None;
        };
        if *sym == target {
            return Some(ty.clone());
        }
        for p in self.st.get(*sym).parents.clone() {
            let p = if args.is_empty() {
                p
            } else {
                self.st.subst_tparams(*sym, args, &p)
            };
            if let Some(found) = self.base_type_instance(&p, target, depth + 1) {
                return Some(found);
            }
        }
        None
    }

    /// Type arguments for a constructor pattern's class, read off the scrutinee:
    /// matching `Option[Int]` with `case Some(v)` binds `v: Int`, because
    /// `Some[A] <: Option[A]` forces `A = Int`. Empty when nothing constrains
    /// them, which leaves the declared (unsubstituted) field types in place.
    fn pattern_class_targs(&self, cls: SymbolId, sel_ty: &Type) -> Vec<Type> {
        let tps = self.st.get(cls).tparams.clone();
        if tps.is_empty() {
            return Vec::new();
        }
        let (sel_sym, sel_args) = match sel_ty {
            Type::Class { sym, args } => (*sym, args.clone()),
            _ => return Vec::new(),
        };
        if sel_args.is_empty() {
            return Vec::new();
        }
        if sel_sym == cls {
            return if sel_args.len() == tps.len() {
                sel_args
            } else {
                Vec::new()
            };
        }
        let open = Type::Class {
            sym: cls,
            args: tps.iter().map(|t| Type::TypeParam(*t)).collect(),
        };
        let Some(base) = self.base_type_instance(&open, sel_sym, 0) else {
            return Vec::new();
        };
        let Type::Class {
            args: base_args, ..
        } = &base
        else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(tps.len());
        for tp in &tps {
            match unify_tparam(*tp, base_args, &sel_args) {
                Some(t) if !t.is_no_type() => out.push(t),
                _ => return Vec::new(),
            }
        }
        out
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
                // `val SilentCast = new FunctionSymbol(…)` is an extractor
                // too: the `unapply` lives on the value's own type.
                _ => self.st.class_sym_of(&fun.ty).unwrap_or(SymbolId::NONE),
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
                // `val SilentCast = new FunctionSymbol(…)` is an extractor
                // too: the `unapply` lives on the value's own type.
                _ => self.st.class_sym_of(&fun.ty).unwrap_or(SymbolId::NONE),
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
                                    format!("parameter '{name}' is already specified"),
                                );
                            }
                            slots[i] = Some((**rhs).clone());
                        }
                        None => self.error(a.span, format!("unknown parameter name: {name}")),
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

    /// `Some.unapply[A](x: Option[A]): Option[A]` extracts `A`; the scrutinee
    /// says what `A` is. Without this the bound variable keeps the extractor's
    /// own type parameter and degrades to `Any`.
    fn subst_unapply_tparams(&self, unapply: SymbolId, sel_ty: &Type, out: Vec<Type>) -> Vec<Type> {
        let tps = self.st.get(unapply).tparams.clone();
        if tps.is_empty() || sel_ty.is_no_type() {
            return out;
        }
        let param = match &self.st.get(unapply).ty {
            Type::Method { paramss, .. } => paramss.first().and_then(|p| p.first()).cloned(),
            Type::Function { params, .. } => params.first().cloned(),
            _ => None,
        };
        let Some(param) = param else {
            return out;
        };
        let params = [param];
        let args = [sel_ty.clone()];
        let mut ids = Vec::new();
        let mut tys = Vec::new();
        for tp in tps {
            if let Some(t) = unify_tparam(tp, &params, &args) {
                if !t.is_no_type() && !t.is_error() {
                    ids.push(tp);
                    tys.push(t);
                }
            }
        }
        if ids.is_empty() {
            return out;
        }
        out.iter()
            .map(|t| crate::symbol::subst_tparams_slice(&ids, &tys, t))
            .collect()
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
                let name = name.clone();
                self.resolve_type_name_completing(&name, &[], tpt.span)
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
                    self.complete_lazy_sig(id, tpt.span);
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
                    // `<tuple>` is the parser's marker for a parenthesised
                    // type list; outside a function type it is just a tuple.
                    Some(n) if n.starts_with("Tuple") || n == "<tuple>" => Type::Tuple(as_),
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

    /// `new i.Deep()` names the enclosing instance of the class it creates.
    /// `tree_to_type` only reads the prefix as a path, so the backend would
    /// have no typed tree to evaluate; type it as an expression as well.
    /// A type or package prefix (`new scala.Foo`, `new Outer.Inner`) is left
    /// alone — there is nothing to evaluate there.
    fn type_new_prefix(&mut self, tpt: &mut Tree) {
        let qual = match &mut tpt.kind {
            TreeKind::Select { qual, .. } => qual,
            TreeKind::AppliedTypeTree { tpt, .. }
            | TreeKind::TypeApply { fun: tpt, .. }
            | TreeKind::AnnotatedTypeTree { tpt, .. } => {
                self.type_new_prefix(tpt);
                return;
            }
            _ => return,
        };
        if !qual.ty.is_no_type() || !self.is_stable_path(qual) {
            return;
        }
        let term = self.type_select_is_term_prefix(qual);
        let mut prefix = qual.as_ref().clone();
        // Speculative: a package prefix (`new scala.Foo`) is not a value and
        // must not leave "not found" diagnostics behind.
        let mark = self.diags.len();
        self.type_expr(&mut prefix, &Type::NoType);
        self.diags.truncate(mark);
        let usable = (term || matches!(prefix.ty, Type::ModuleRef(_)))
            && !prefix.ty.is_no_type()
            && !prefix.ty.is_error();
        if usable {
            **qual = prefix;
        }
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

    /// Complete the alias(es) a projection prefix names, then re-read it. A
    /// parameterized alias only folds into its right-hand side once that side
    /// is known, so `DSL.arg[B1, P1]#to` needs `arg` completed first.
    fn complete_prefix_aliases(&mut self, span: Span, prefix: &Type) -> Type {
        match prefix {
            Type::TypeMember(id) => {
                self.complete_lazy_sig(*id, span);
                prefix.clone()
            }
            Type::Applied { ctor, args } => {
                if let Type::TypeMember(id) = ctor.as_ref() {
                    self.complete_lazy_sig(*id, span);
                    return self
                        .st
                        .expand_applied_hk_alias(crate::symbol::apply_type_ctor(
                            (**ctor).clone(),
                            args.clone(),
                        ));
                }
                prefix.clone()
            }
            _ => prefix.clone(),
        }
    }

    fn project_from_prefix(&mut self, span: Span, prefix: &Type, name: &str) -> Type {
        // A projection out of a prefix that already failed reports nothing new.
        if prefix.is_error() {
            return Type::Error;
        }
        // `o#arg[…]`: the prefix may be an alias whose right-hand side lives in
        // a unit that has not been walked yet. Resolve it before projecting.
        let prefix = &self.complete_prefix_aliases(span, prefix);
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
            TreeKind::This { .. } | TreeKind::Super { .. } => true,
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

    /// The leftmost identifier of a path has to be brought into scope before
    /// the path can be resolved: `p.HNil.type` in package `p` only sees `p`
    /// once `expose_unqualified` has entered it.
    fn expose_path_head(&mut self, t: &Tree) {
        match &t.kind {
            TreeKind::Ident { name } => {
                let name = name.clone();
                self.expose_unqualified(&name, t.span);
            }
            TreeKind::Select { qual, .. } | TreeKind::SelectFromTypeTree { qual, .. } => {
                self.expose_path_head(qual)
            }
            _ => {}
        }
    }

    fn singleton_to_type(&mut self, span: Span, ref_: &Tree) -> Type {
        self.expose_path_head(ref_);
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
            TreeKind::Ident { name } => self.st.lookup_term(name).into_iter().find(|s| {
                matches!(
                    self.st.get(*s).kind,
                    SymKind::Term | SymKind::Module | SymKind::ModuleClass
                )
            }),
            TreeKind::Select { qual, name } | TreeKind::SelectFromTypeTree { qual, name, .. } => {
                let Some(qt) = self.term_path_type(qual) else {
                    // A package is not a value, so it has no type -- but it is
                    // still a legal path prefix: `p.q.HNil.type`.
                    let owner = self.path_owner_sym(qual)?;
                    return self.st.lookup_member(owner, name).into_iter().find(|s| {
                        matches!(
                            self.st.get(*s).kind,
                            SymKind::Term | SymKind::Module | SymKind::ModuleClass
                        )
                    });
                };
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
                let cls = self.path_member_owner(&qt)?;
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

    /// The owner a path prefix names when that prefix is a package or a
    /// module. Packages carry no type, so `term_path_type` has nothing to hand
    /// back for them, yet they are legal prefixes of a stable path.
    fn path_owner_sym(&self, t: &Tree) -> Option<SymbolId> {
        let pick = |st: &SymbolTable, cands: Vec<SymbolId>| -> Option<SymbolId> {
            cands
                .into_iter()
                .find(|s| {
                    matches!(
                        st.get(*s).kind,
                        SymKind::Package | SymKind::Module | SymKind::ModuleClass
                    )
                })
                .map(|s| match st.get(s).kind {
                    SymKind::Module => st.module_class_of(s),
                    _ => s,
                })
        };
        match &t.kind {
            TreeKind::Ident { name } => pick(&self.st, self.st.lookup_term(name)),
            TreeKind::Select { qual, name } | TreeKind::SelectFromTypeTree { qual, name, .. } => {
                let owner = self.path_owner_sym(qual)?;
                pick(&self.st, self.st.lookup_member(owner, name))
            }
            _ => None,
        }
    }

    /// The symbol whose members a path prefix offers. `object O { object I }`
    /// keeps `I` on the *module class* `O$`, so `O.I.type` has to look there
    /// and not on the module symbol itself.
    fn path_member_owner(&self, ty: &Type) -> Option<SymbolId> {
        let cls = self.st.class_sym_of(ty)?;
        Some(match self.st.get(cls).kind {
            SymKind::Module => self.st.module_class_of(cls),
            _ => cls,
        })
    }

    fn ident_is_stable(&self, name: &str) -> bool {
        let found = self.st.lookup_term(name);
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
            // `p.HNil` under a package prefix: the package has no type.
            return match self.path_owner_sym(qual) {
                Some(owner) => self.st.lookup_member(owner, name).iter().any(|s| {
                    let sy = self.st.get(*s);
                    match sy.kind {
                        SymKind::Module | SymKind::ModuleClass | SymKind::Package => true,
                        SymKind::Term => !sy.flags.contains(Flags::MUTABLE),
                        _ => false,
                    }
                }),
                None => false,
            };
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
        let Some(cls) = self.path_member_owner(&pty) else {
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
                    Some(self.st.self_type_of_class(self.st.this_class))
                }
            }
            // `super.T` in type position: the member is looked up in the
            // parent `super` names, so the prefix is that parent's type.
            TreeKind::Super { qual, mix } => {
                let parent = self.super_target(self.super_owner(qual.as_deref()), mix.as_deref());
                (!parent.is_none()).then(|| self.st.type_of_class(parent))
            }
            TreeKind::Ident { name } => {
                let found = self.st.lookup_term(name);
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
                let Some(qt) = self.term_path_type(qual) else {
                    // A package prefix (`p.HNil`) carries no type of its own.
                    let owner = self.path_owner_sym(qual)?;
                    return self
                        .st
                        .lookup_member(owner, name)
                        .into_iter()
                        .find_map(|s| {
                            let sy = self.st.get(s);
                            match sy.kind {
                                SymKind::Term | SymKind::Method => Some(sy.ty.clone()),
                                SymKind::Module | SymKind::ModuleClass => {
                                    Some(self.st.type_of_class(s))
                                }
                                _ => None,
                            }
                        });
                };
                if let Type::Refined { decls, .. } = &qt {
                    if let Some(t) = SymbolTable::refine_member_type(decls, name) {
                        return Some(self.st.expand_in_type(&qt, &t));
                    }
                }
                let cls = self.path_member_owner(&qt)?;
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
        for owner in self.qualified_type_owners(prefix) {
            self.complete_binary_member(owner, name, prefix.span);
            if let Some(id) = self.prefer_class_member(owner, name) {
                return Some(id);
            }
        }
        None
    }

    /// `p` in a type `p.T` denotes a *term* in Scala, so when a class and its
    /// companion share a name the module class owns the member (`C#T` is how a
    /// class projection is written). Java's static nested classes are still
    /// reached through the class, so it stays a candidate behind the module.
    fn type_owner_rank(&self, id: SymbolId) -> u8 {
        match self.st.get(id).kind {
            SymKind::Module | SymKind::ModuleClass => 0,
            SymKind::Package => 1,
            _ => 2,
        }
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
        self.type_owner_members(owner, name).into_iter().next()
    }

    /// Every member of `owner` called `name` that can carry a type, best
    /// first (see `prefer_class_member`).
    fn type_owner_members(&self, owner: SymbolId, name: &str) -> Vec<SymbolId> {
        let found = self.st.lookup_member(owner, name);
        let mut out: Vec<SymbolId> = found
            .iter()
            .copied()
            .filter(|&s| self.st.get(s).kind == SymKind::Class)
            .collect();
        for s in found {
            let ok = matches!(
                self.st.get(s).kind,
                SymKind::Package
                    | SymKind::Module
                    | SymKind::ModuleClass
                    | SymKind::TypeMember
                    | SymKind::TypeParam
            );
            if ok && !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }

    fn qualified_type_owner(&mut self, t: &Tree) -> Option<SymbolId> {
        self.qualified_type_owners(t).into_iter().next()
    }

    /// Every owner a `p.T` prefix can denote, best first (see `type_owner_rank`).
    fn qualified_type_owners(&mut self, t: &Tree) -> Vec<SymbolId> {
        let mut out: Vec<SymbolId> = Vec::new();
        match &t.kind {
            TreeKind::Ident { name } => {
                self.expose_unqualified(name, t.span);
                let mut found: Vec<SymbolId> = self
                    .st
                    .lookup(name)
                    .into_iter()
                    .filter(|id| {
                        matches!(
                            self.st.get(*id).kind,
                            SymKind::Package
                                | SymKind::Class
                                | SymKind::Module
                                | SymKind::ModuleClass
                        )
                    })
                    .collect();
                found.sort_by_key(|id| self.type_owner_rank(*id));
                for id in found {
                    let o = self.as_type_owner(id);
                    if !out.contains(&o) {
                        out.push(o);
                    }
                }
            }
            TreeKind::Select { qual, name } => {
                for owner in self.qualified_type_owners(qual) {
                    self.complete_binary_member(owner, name, t.span);
                    let mut found: Vec<SymbolId> = self
                        .st
                        .lookup_member(owner, name)
                        .into_iter()
                        .filter(|id| {
                            matches!(
                                self.st.get(*id).kind,
                                SymKind::Package
                                    | SymKind::Class
                                    | SymKind::Module
                                    | SymKind::ModuleClass
                            )
                        })
                        .collect();
                    found.sort_by_key(|id| self.type_owner_rank(*id));
                    for id in found {
                        let o = self.as_type_owner(id);
                        if !out.contains(&o) {
                            out.push(o);
                        }
                    }
                }
            }
            _ => {}
        }
        out
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
                return;
            }
            // `math.Pi` is a member of the package object `scala/math/package$`,
            // which the package itself only gains once that class is read.
            if self.st.lookup_member(owner, name).is_empty() {
                let _ = self.package_object_of(owner, span);
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

    /// Install `name` on the receiver's class from the library `ScalaSignature`
    /// and return whatever that made visible. Empty unless the receiver is a
    /// standard-library class *and* the member could be expressed faithfully.
    fn supply_from_pickle(&mut self, recv_ty: &Type, name: &str) -> Vec<SymbolId> {
        if !self.library_abi {
            return Vec::new();
        }
        let Some(cls) = self.st.class_sym_of(recv_ty) else {
            return Vec::new();
        };
        // Members found on a companion object land on that module class, not
        // on `cls`, so take what completion reports rather than re-looking-up.
        self.pickle
            .complete(&mut self.st, &mut self.binary, cls, name)
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

    /// `resolve_type_name`, after completing any alias the name binds to. A
    /// reference from an earlier unit must not see the alias's `<notype>`.
    fn resolve_type_name_completing(&mut self, name: &str, args: &[Type], span: Span) -> Type {
        for id in self.st.lookup_type(name) {
            if self.st.get(id).kind == SymKind::TypeMember {
                self.complete_lazy_sig(id, span);
            }
        }
        self.resolve_type_name(name, args)
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
                let found = self.st.lookup_type(name);
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

    /// Wrap `tree` in the `toLong` / `toDouble` / `toFloat` conversion that
    /// widens it to `to`, so the backend emits `i2l` / `i2d` / `i2f`.
    fn wrap_numeric_widen(&mut self, tree: &mut Tree, to: &Type) {
        let name = match to {
            Type::Long => "toLong",
            Type::Float => "toFloat",
            Type::Double => "toDouble",
            _ => {
                tree.ty = to.clone();
                return;
            }
        };
        let from = tree.ty.widen_constant();
        let conv = self
            .st
            .class_sym_of(&from)
            .map(|cls| self.st.lookup_member(cls, name))
            .unwrap_or_default()
            .into_iter()
            .find(|&s| {
                self.st.get(s).kind == SymKind::Method
                    && !matches!(self.st.get(s).intrinsic, crate::symbol::Intrinsic::None)
            });
        let Some(conv) = conv else {
            tree.ty = to.clone();
            return;
        };
        let span = tree.span;
        let inner = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
        let id = inner.id;
        *tree = Tree {
            id,
            span,
            kind: TreeKind::Select {
                qual: Box::new(inner),
                name: name.to_string(),
            },
            ty: to.clone(),
            sym: conv,
            postfix: false,
        };
    }

    fn adapt(&mut self, tree: &mut Tree, pt: &Type) {
        if matches!(pt, Type::Method { .. }) {
            return;
        }
        if pt.is_no_type() || tree.ty.is_error() || pt.is_error() {
            return;
        }
        // `xs: _*` is already the sequence a repeated parameter takes.
        if matches!(tree.ty, Type::Repeated(_)) {
            return;
        }
        self.complete_java_type(&tree.ty, tree.span);
        self.complete_java_type(pt, tree.span);
        // By-name wrap must run before `Nothing <: pt` (Nothing inhabits every
        // type, including `=> T`). Otherwise `tryBreakable { throw e }` would
        // skip Function0 and throw in the caller.
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
            // The JVM needs the conversion instruction; setting the type alone
            // left an `int` on the stack where a `double` was expected.
            self.wrap_numeric_widen(tree, &w);
            return;
        }
        // SLS 6.26.1: an `Int` *literal* in range narrows to `Byte`, `Short`
        // or `Char` (`val b: Byte = 1`). Only a constant; `val b: Byte = n`
        // stays an error.
        if let Type::Constant(Lit::Int(v)) = &tree.ty {
            let fits = match pt {
                Type::Byte => (-128..=127).contains(v),
                Type::Short => (-32768..=32767).contains(v),
                Type::Char => (0..=65535).contains(v),
                _ => false,
            };
            if fits {
                tree.ty = pt.clone();
                return;
            }
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
        if let Type::Method { paramss, ret } = &tree.ty {
            if is_function_pt(pt) || self.st.sam_sig(pt).is_some() {
                let mut params: Vec<Type> = paramss.iter().flatten().cloned().collect();
                let mut ret = (**ret).clone();
                // `val f: Node => Node = identity` eta-expands
                // `def identity[A](x: A): A`. Solve `A` from the expected
                // function type first: expanding the method as written yields
                // `A => A`, which conforms to nothing.
                if let Some((pt_params, pt_ret)) = function_sig(pt) {
                    let tps = if tree.sym.is_none() {
                        Vec::new()
                    } else {
                        self.st.get(tree.sym).tparams.clone()
                    };
                    if !tps.is_empty() && pt_params.len() == params.len() {
                        let mut sig = params.clone();
                        sig.push(ret.clone());
                        let mut want = pt_params;
                        want.push(pt_ret);
                        let inst = self.infer_method_tparams(tree.sym, &sig, &want);
                        if !inst.is_empty() {
                            let ids: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                            let vals: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
                            params = params
                                .iter()
                                .map(|p| crate::symbol::subst_tparams_slice(&ids, &vals, p))
                                .collect();
                            ret = crate::symbol::subst_tparams_slice(&ids, &vals, &ret);
                        }
                    }
                }
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
            // `type Self >: this.type <: Node`: the declaration says
            // `this.type <: Self`, so `this` conforms to `Self` even though
            // `Node` does not. Only the *lower* bound admits this, and only a
            // tree that really is that singleton -- `def id: Self = someNode`
            // still fails.
            Type::TypeMember(id) => {
                let Some(lo) = self.st.get(*id).bound_lo.clone() else {
                    return false;
                };
                if lo.is_no_type() || lo.is_error() || matches!(lo, Type::Nothing) {
                    return false;
                }
                if self.st.is_sub_type(&tree.ty, &lo) || self.adapt_singleton(tree, &lo) {
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
                let diverged = self.diverged_implicit.borrow().clone();
                self.error(span, self.missing_implicit_message(&ct_ty, diverged));
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

    /// `x.f = v` where `f` resolved to a getter and the receiver has an
    /// `f_=`: the assignment is that call, not a field store.
    fn setter_assign_lhs(&mut self, lhs: &Tree) -> bool {
        let TreeKind::Select { qual, name } = &lhs.kind else {
            return false;
        };
        if lhs.sym.is_none() || self.st.get(lhs.sym).kind != SymKind::Method {
            return false;
        }
        let Some(cls) = self.st.class_sym_of(&qual.ty) else {
            return false;
        };
        let setter = format!("{name}_=");
        self.st
            .lookup_member(cls, &setter)
            .into_iter()
            .any(|m| self.st.get(m).kind == SymKind::Method)
    }

    /// nsc's `reassignment to val`. Without it `d.v = 5` on a trait's `val`
    /// type-checks and then fails at run time: the mixin setter a trait `val`
    /// gets is not a setter a program may call.
    fn check_reassignment(&mut self, lhs: &Tree) {
        let id = lhs.sym;
        if id.is_none() {
            return;
        }
        let s = self.st.get(id);
        // Only a term (a field or local) is an l-value here; a `Method` lhs is
        // an already-resolved `x_=` setter or an unrelated resolution failure.
        if s.kind != SymKind::Term || s.flags.contains(Flags::MUTABLE) {
            return;
        }
        // Java fields carry no Scala mutability, and the compiler's own
        // synthetic terms (`$outer`, capture fields, …) are written by the
        // phases that create them.
        if s.flags.contains(Flags::JAVA) || s.flags.contains(Flags::SYNTHETIC) {
            return;
        }
        let name = s.name.clone();
        self.error(lhs.span, format!("reassignment to val {name}"));
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
            // Real scalac accepts `@inline`/`@noinline` on any definition (val, var,
            // class, type, ...): they are hints consumed only by the bytecode-level
            // optimizer (`-opt:...`), which scala-rs does not implement, and placement
            // is never validated by the typer. See sgap fixtures for confirmation
            // against scalac 2.13.16 (no error, no warning, even with -Xlint:_).
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

    fn missing_implicit_message(&self, ty: &Type, diverged: Option<(SymbolId, Type)>) -> String {
        // nsc reports the cut-off expansion rather than a plain "not found"
        // when the search ran into a diverging one.
        if let Some((sym, pt)) = diverged {
            return format!(
                "diverging implicit expansion for type {} starting with method {}",
                self.st.display_type(&pt),
                self.st.get(sym).name
            );
        }
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
    matches!(
        name,
        "Map" | "HashMap" | "LinkedHashMap" | "SortedMap" | "TreeMap" | "MapView"
    )
}

fn is_tailrec_annot(path: &str) -> bool {
    matches!(
        path,
        "tailrec" | "annotation.tailrec" | "scala.annotation.tailrec"
    )
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

/// The parameter and result types of an expected function type, for the
/// `Function1`/`FunctionN` spellings the typer produces.
fn function_sig(pt: &Type) -> Option<(Vec<Type>, Type)> {
    match pt {
        Type::Function { params, ret } => Some((params.clone(), (**ret).clone())),
        Type::Class { sym: _, args } if args.len() >= 2 && is_function_pt(pt) => {
            let (last, init) = args.split_last()?;
            Some((init.to_vec(), last.clone()))
        }
        _ => None,
    }
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

/// `scala.util.Either` and its two cases. 2.13 made them right-biased, so the
/// "element" of an `Either[A, B]` is its `B`. Matched by JVM name so a user
/// class that happens to be called `Either` keeps the ordinary rule.
fn is_right_biased_either(st: &SymbolTable, id: SymbolId) -> bool {
    if id.is_none() {
        return false;
    }
    matches!(
        st.get(id).jvm_name.as_str(),
        "scala/util/Either" | "scala/util/Left" | "scala/util/Right"
    )
}

/// The parser desugars `{ case … }` into `x$pf => x$pf match { case … }`.
fn is_case_block_literal(vparams: &[Tree], body: &Tree) -> bool {
    vparams.len() == 1
        && vparams[0].name() == Some(PF_PARAM)
        && matches!(&body.kind, TreeKind::Match { .. })
}

/// Name the parser gives the synthesized parameter of a `{ case … }` literal.
const PF_PARAM: &str = "x$pf";

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

fn numeric_widen(a: &Type, b: &Type) -> Option<Type> {
    let a = a.widen_constant();
    let b = b.widen_constant();
    match (&a, &b) {
        (Type::Int, Type::Long) => Some(Type::Long),
        (Type::Int, Type::Double) => Some(Type::Double),
        (Type::Long, Type::Double) => Some(Type::Double),
        (Type::Float, Type::Double) => Some(Type::Double),
        (Type::Int, Type::Float) => Some(Type::Float),
        (Type::Long, Type::Float) => Some(Type::Float),
        // SLS 3.5.3 weak conformance: `Byte <= Short <= Int <= Long <= Float
        // <= Double` and `Char <= Int`. `Byte`/`Short`/`Char` are `int` on the
        // stack, so widening to `Short` or `Int` needs no instruction and
        // `wrap_numeric_widen` just retypes the tree.
        (Type::Byte, Type::Short | Type::Int | Type::Long | Type::Float | Type::Double) => {
            Some(b.clone())
        }
        (Type::Short | Type::Char, Type::Int | Type::Long | Type::Float | Type::Double) => {
            Some(b.clone())
        }
        _ => None,
    }
}

/// Result type of an `if` / `match`: nsc uses the lub of the branches and then
/// adapts to the expected type. We prefer `pt` because a structural lub cannot
/// walk parents (`if (c) Some(1) else None` must stay `Option[Int]`), but only
/// when `pt` really says something. A lambda body is typed against a *stand-in*
/// `Any` whenever the method's result type parameter is still undetermined
/// (`xs.map(f)`'s `B`); adopting it there would make every `if`/`match` bodied
/// lambda infer `A => Any` and collapse `xs.map { case … }` to `List[Any]`.
fn pt_or_lub(pt: &Type, branches: Type) -> Type {
    if !pt.is_no_type() && !matches!(pt, Type::Nothing | Type::Any | Type::TypeParam(_)) {
        pt.clone()
    } else {
        branches
    }
}

/// Undo the parser's `{A,B=>C,_}` encoding of an import selector list.
/// Each entry is `(name, alias)`; `("_", "_")` is the wildcard, and an alias
/// of `_` hides the name.
fn decode_import_selectors(encoded: &str) -> Vec<(String, String)> {
    let inner = encoded.trim_matches(|c| c == '{' || c == '}');
    let mut out = Vec::new();
    for sel in inner.split(',') {
        let sel = sel.trim();
        if sel.is_empty() {
            continue;
        }
        let (from, to) = match sel.split_once("=>") {
            Some((f, t)) => (f.trim(), t.trim()),
            None => (sel, sel),
        };
        out.push((from.to_string(), to.to_string()));
    }
    out
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
        // `U: BaseColumnType` where `BaseColumnType` is a *parameterized type
        // member* (or another type parameter) still means `BaseColumnType[U]`.
        bound @ (Type::TypeMember(_) | Type::TypeParam(_)) => {
            crate::symbol::apply_type_ctor(bound, vec![Type::TypeParam(tp)])
        }
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

/// Whether `ty` still mentions a type parameter, i.e. is not a proper type yet.
fn mentions_any_tparam(ty: &Type) -> bool {
    match ty {
        Type::TypeParam(_) => true,
        Type::Class { args, .. } | Type::Tuple(args) => args.iter().any(mentions_any_tparam),
        Type::Applied { ctor, args } => {
            mentions_any_tparam(ctor) || args.iter().any(mentions_any_tparam)
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            mentions_any_tparam(t)
        }
        Type::Function { params, ret } => {
            params.iter().any(mentions_any_tparam) || mentions_any_tparam(ret)
        }
        _ => false,
    }
}

fn flip_variance(v: i8) -> i8 {
    -v
}

/// Variance of an occurrence nested `inner` deep inside an `outer` position.
/// An invariant position stays invariant however it is nested.
fn compose_variance(outer: i8, inner: i8) -> i8 {
    if outer == 0 || inner == 0 {
        0
    } else {
        outer * inner
    }
}

/// `scala.Array` reached through a classfile signature arrives as
/// `Class { sym: array_sym }`, whose JVM name is the pseudo-name
/// `[java/lang/Object`. Anything used as an inferred type argument has to be
/// in `Type::Array` form or the backend emits a method owner that no JVM can
/// load.
fn unarrayify(t: &Type, array_sym: SymbolId) -> Type {
    match t {
        Type::Class { sym, args } if *sym == array_sym && args.len() == 1 => {
            Type::Array(Box::new(unarrayify(&args[0], array_sym)))
        }
        Type::Class { sym, args } if !args.is_empty() => Type::Class {
            sym: *sym,
            args: args.iter().map(|a| unarrayify(a, array_sym)).collect(),
        },
        Type::Array(e) => Type::Array(Box::new(unarrayify(e, array_sym))),
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(|a| unarrayify(a, array_sym)).collect()),
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(|a| unarrayify(a, array_sym)).collect(),
            ret: Box::new(unarrayify(ret, array_sym)),
        },
        other => other.clone(),
    }
}

/// The first of `params` that pins `tp` against the matching `args` entry.
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

/// `ty` が `tps` のいずれかのメソッド型パラメータを含むか。
fn mentions_tparam(ty: &Type, tps: &[SymbolId]) -> bool {
    match ty {
        Type::TypeParam(id) => tps.contains(id),
        Type::Class { args, .. } => args.iter().any(|a| mentions_tparam(a, tps)),
        Type::Applied { ctor, args } => {
            mentions_tparam(ctor, tps) || args.iter().any(|a| mentions_tparam(a, tps))
        }
        Type::Function { params, ret } => {
            params.iter().any(|p| mentions_tparam(p, tps)) || mentions_tparam(ret, tps)
        }
        Type::Array(e) | Type::ByName(e) | Type::Repeated(e) => mentions_tparam(e, tps),
        Type::Annotated { tpe, .. } => mentions_tparam(tpe, tps),
        Type::Tuple(ts) => ts.iter().any(|t| mentions_tparam(t, tps)),
        _ => false,
    }
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

/// Whether `tp` occurs anywhere in `ty`.
pub(crate) fn type_mentions_tparam(ty: &Type, tp: SymbolId) -> bool {
    match ty {
        Type::TypeParam(id) => *id == tp,
        Type::Class { args, .. } | Type::Named { args, .. } | Type::Tuple(args) => {
            args.iter().any(|t| type_mentions_tparam(t, tp))
        }
        Type::Applied { ctor, args } => {
            type_mentions_tparam(ctor, tp) || args.iter().any(|t| type_mentions_tparam(t, tp))
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            type_mentions_tparam(t, tp)
        }
        Type::Function { params, ret } => {
            params.iter().any(|t| type_mentions_tparam(t, tp)) || type_mentions_tparam(ret, tp)
        }
        Type::Method { paramss, ret } => {
            paramss
                .iter()
                .flatten()
                .any(|t| type_mentions_tparam(t, tp))
                || type_mentions_tparam(ret, tp)
        }
        _ => false,
    }
}

pub(crate) fn unify_one(tp: SymbolId, pattern: &Type, actual: &Type) -> Option<Type> {
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
            // `Tuple2[K, V]` against `(Int, String)`: the tuple sugar and the
            // nominal class denote the same type (`is_sub_type` already treats
            // them as such), so unify positionally when the arity agrees.
            let aas = match actual {
                Type::Class { args, .. } => args,
                Type::Tuple(ts) if ts.len() == pas.len() => ts,
                _ => return None,
            };
            for (p, a) in pas.iter().zip(aas) {
                if let Some(t) = unify_one(tp, p, a) {
                    return Some(t);
                }
            }
            None
        }
        // `Show[(A, B)]` against `Show[Tuple2[Int, String]]`.
        Type::Tuple(pts) => {
            let aas = match actual {
                Type::Tuple(ts) if ts.len() == pts.len() => ts,
                Type::Class { args, .. } if args.len() == pts.len() => args,
                _ => return None,
            };
            for (p, a) in pts.iter().zip(aas) {
                if let Some(t) = unify_one(tp, p, a) {
                    return Some(t);
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

/// A function literal whose parameters all carry a type annotation
/// (`(x: String) => x.length`). Its type is known without an expected type,
/// so it can be typed eagerly and drive type-parameter inference.
fn is_annotated_lambda(tree: &Tree) -> bool {
    match &tree.kind {
        TreeKind::Function { vparams, .. } => {
            !vparams.is_empty()
                && vparams.iter().all(|p| match &p.kind {
                    TreeKind::ValDef { tpt, .. } => !tpt.is_empty(),
                    _ => false,
                })
        }
        _ => false,
    }
}

/// Drop diagnostics repeated verbatim at the same position, keeping the first.
fn dedup_diags(diags: &mut Vec<Diagnostic>) {
    let mut seen = std::collections::HashSet::new();
    diags.retain(|d| {
        seen.insert((
            d.file_index,
            d.span.lo,
            d.span.hi,
            d.level,
            d.message.clone(),
        ))
    });
}

/// A type argument that is still an uninstantiated type parameter admits
/// nothing, so relax it to `Any` before checking an argument against it. Only
/// arguments are relaxed: a parameter that *is* a type parameter (`def f[A](x:
/// A)`) is left alone, so `f[Int]("s")` is still an error.
/// `open` names the callee's own type parameters — the only ones the call can
/// still leave undetermined; `None` means they are unknown and all of them are
/// relaxed. A *class* type parameter in scope (`Rep[P1]` inside
/// `trait Base[P1]`), or one of an enclosing method, is a perfectly
/// determinate type and must be left alone: widening it to `Rep[Any]` made
/// `def f[T](a: Inv[T]) = g(a)` check `Inv[T]` against `Inv[Any]` -- a
/// mismatch for every invariant class and a silent widening for every
/// covariant one.
fn relax_open_tparams(ty: &Type, open: Option<&[SymbolId]>) -> Type {
    let is_open = |a: &Type| match a {
        Type::TypeParam(id) => open.is_none_or(|o| o.contains(id)),
        _ => false,
    };
    match ty {
        Type::Class { sym, args } if args.iter().any(is_open) => Type::Class {
            sym: *sym,
            args: args
                .iter()
                .map(|a| if is_open(a) { Type::Any } else { a.clone() })
                .collect(),
        },
        _ => ty.clone(),
    }
}

/// Re-read a parent's `this.type` as the overriding class's own.
///
/// `trait Nd { type Self >: this.type <: Nd }` overridden by
/// `class Leafy extends Nd { type Self = Leafy }` has to compare `Leafy`
/// against `Leafy.this.type`, not against `Nd.this.type`.
fn retarget_this(ty: &Type, cls: SymbolId) -> Type {
    match ty {
        Type::ThisType(_) => Type::ThisType(cls),
        Type::Class { sym, args } => Type::Class {
            sym: *sym,
            args: args.iter().map(|a| retarget_this(a, cls)).collect(),
        },
        Type::Refined { parents, decls } => Type::Refined {
            parents: parents.iter().map(|p| retarget_this(p, cls)).collect(),
            decls: decls.clone(),
        },
        _ => ty.clone(),
    }
}

/// Number of parameters a `FunctionN` expected type takes, if it is one.
fn expected_function_arity(pt: &Type) -> Option<usize> {
    match pt {
        Type::Function { params, .. } => Some(params.len()),
        Type::Named { name, args } if name.starts_with("Function") && name != "Function" => {
            (!args.is_empty()).then(|| args.len() - 1)
        }
        _ => None,
    }
}
