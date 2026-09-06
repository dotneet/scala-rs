#![allow(dead_code)]
//! Namer phase: the pass that enters symbols before anything is typed.
//!
//! Walks a unit and creates the symbol for every package, class, object,
//! trait, type member, `def` and `val` it meets, together with the synthetic
//! members a `case class` owes (`apply`, `copy`, `unapply`, `productN`) and
//! the companions that hold them. Type parameters and their bounds are
//! entered here too, then the header/parents pass resolves each template's
//! parents far enough that member signatures have somewhere to look.

use crate::check::*;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

impl Typer {
    // ------------------------------------------------------------------ namer
    pub(crate) fn namer(&mut self, tree: &mut Tree) {
        match &mut tree.kind {
            TreeKind::PackageDef { pid, stats } => {
                let pkg = self.enter_package_path(pid);
                let saved = self.st.owner;
                self.st.owner = pkg;
                self.pkg_nest.push(pkg);
                let opened = self.open_pkgs.entry(self.file_index).or_default();
                if !opened.contains(&pkg) {
                    opened.push(pkg);
                }
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
                let within = mods.private_within.clone();
                let id = self
                    .st
                    .alloc(name, self.st.owner, SymKind::Class, flags, &jvm);
                self.st.get_mut(id).annotations = annots;
                // `private[jdbc] class`/`object` kept only the PRIVATE flag,
                // so the access check read it as plain `private` and every
                // reference from elsewhere in the package was rejected
                // (slick's `GetResult.GetUpdateValue`).
                self.st.get_mut(id).private_within = within;
                // Before entering it: a source definition of a name the
                // prelude supplies replaces the prelude's symbol, which is
                // otherwise the one every lookup returns.
                self.st.shadow_supplied_by_source(id);
                self.st.enter_in_current(name, id);
                self.auto_import_scala_member(name, id);
                tree.sym = id;
                if mods.flags.contains(Flags::CASE) {
                    let class_jvm = jvm.clone();
                    self.ensure_companion(name, &class_jvm, id);
                }
            }
            TreeKind::ModuleDef { name, mods, .. } => {
                let annots = mods.annotations.clone();
                let mods_within = mods.private_within.clone();
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
                    if mods_within.is_some() {
                        self.st.get_mut(m).private_within = mods_within.clone();
                        self.st.get_mut(cls).private_within = mods_within;
                    }
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
                self.st.get_mut(m).private_within = mods_within.clone();
                self.st.get_mut(cls).private_within = mods_within;
                self.st.shadow_supplied_by_source(cls);
                self.st.shadow_supplied_by_source(m);
                self.st.enter_in_current(name, m);
                self.auto_import_scala_member(name, m);
                tree.sym = m;
            }
            TreeKind::PackageDef { .. } => {}
            _ => {}
        }
    }

    /// `scala._` is open around every unit, so a source definition that lands
    /// directly in package `scala` is in scope everywhere, not only in the
    /// files that write `package scala` themselves. The prelude's copy of the
    /// package's members was taken before any source was read, so the entry
    /// has to be made here as well; see
    /// [`SymbolTable::enter_in_prelude_scope`].
    fn auto_import_scala_member(&mut self, name: &str, id: SymbolId) {
        if self.st.owner == self.st.scala_pkg && !self.st.scala_pkg.is_none() {
            self.st.enter_in_prelude_scope(name, id);
        }
    }

    fn jvm_for_current(&mut self, name: &str) -> String {
        // A type's *simple* name goes through the same `NameTransformer`
        // encoding a method name does: slick's `object :@` nested in `object
        // TypeUtil` is `slick/ast/TypeUtil$$colon$at$` for nsc, and we wrote
        // the raw operator characters into the classfile name --
        // `TypeUtil$:@$.class`, which no consumer can name and which is not
        // even a portable file name.
        let name = &scala_rs_pickle::names::encode_method_name(name);
        // A class defined inside a method (`new S { … }`) has the method as
        // its owner; nsc still names it after the enclosing class.
        let mut owner = self.st.owner;
        // Did we have to walk *through* a term (a method, a `val`'s
        // initializer, a lambda) to reach the enclosing class? Then this is a
        // *local* declaration, and its simple name is only unique within that
        // one term: two methods of the same class may each declare a `trait
        // Same`. nsc appends a fresh index (`Main$Same$1`, `Main$Same$2`); we
        // did not, so the second classfile silently overwrote the first and
        // both call sites got the second one's code.
        let mut local = false;
        while !owner.is_none() {
            let ow = self.st.get(owner);
            if ow.kind == SymKind::Package {
                let base = if ow.name != "<_root_>"
                    && !ow.jvm_name.is_empty()
                    && ow.jvm_name != "scala/runtime"
                {
                    format!("{}/{}", ow.jvm_name, name)
                } else {
                    name.to_string()
                };
                return self.uniquify_local(base, local, name);
            }
            let base = ow.jvm_name.trim_end_matches('$');
            if !base.is_empty() {
                return self.uniquify_local(format!("{base}${name}"), local, name);
            }
            local = true;
            owner = ow.owner;
        }
        self.uniquify_local(name.to_string(), local, name)
    }

    /// nsc's local-class index. Anonymous classes already carry a fresh
    /// `$anon$N` in their simple name, so they are left alone.
    fn uniquify_local(&mut self, base: String, local: bool, simple: &str) -> String {
        if !local || simple.starts_with("$anon") {
            return base;
        }
        let n = self.local_class_n.entry(base.clone()).or_insert(0);
        *n += 1;
        format!("{base}${n}")
    }

    /// `class_jvm` is the companion's *class*'s binary name: a local case
    /// class carries an index (`Main$P$1`) that the companion has to reuse
    /// rather than draw a fresh one for.
    fn ensure_companion(&mut self, name: &str, class_jvm: &str, class_id: SymbolId) -> SymbolId {
        let existing = self
            .st
            .lookup(name)
            .into_iter()
            .find(|&s| self.st.get(s).kind == SymKind::Module);
        if let Some(e) = existing {
            return e;
        }
        let jvm = format!("{class_jvm}$");
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
        self.st.shadow_supplied_by_source(cls);
        self.st.shadow_supplied_by_source(m);
        self.st.enter_in_current(name, m);
        // apply / unapply filled after ctor params are known
        let _ = class_id;
        m
    }

    pub(crate) fn namer_class(&mut self, tree: &mut Tree) {
        let id = if tree.sym.is_none() {
            self.namer_enter_tmpl(tree);
            tree.sym
        } else {
            tree.sym
        };
        let (vparamss, body, parents, is_trait, is_case, name, tparams, ctor_mods) =
            match &mut tree.kind {
                TreeKind::ClassDef {
                    vparamss,
                    impl_,
                    mods,
                    name,
                    tparams,
                    ctor_mods,
                    ..
                } => (
                    vparamss,
                    &mut impl_.body,
                    impl_.parents.clone(),
                    mods.flags.contains(Flags::TRAIT),
                    mods.flags.contains(Flags::CASE),
                    name.clone(),
                    tparams,
                    CtorAccess::of(ctor_mods),
                ),
                _ => return,
            };
        self.st.get_mut(id).parents = self.rough_parents(&parents, is_trait);
        let saved_owner = self.st.owner;
        let saved_this = self.st.this_class;
        self.st.owner = id;
        self.st.this_class = id;
        self.st.push_scope();
        let tp_ids = self.enter_tparams_provisional(tparams, id);
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
            self.synthesize_case_members(id, &name, &ctor_mods);
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

    pub(crate) fn enter_tparams(&mut self, tparams: &mut [Tree], owner: SymbolId) -> Vec<SymbolId> {
        let mut ids = Vec::new();
        for tp in tparams.iter_mut() {
            let name = tp.name().unwrap_or("_").to_string();
            let (flags, annots) = match &tp.kind {
                TreeKind::TypeDef { mods, .. } => (mods.flags, mods.annotations.clone()),
                _ => (Flags::EMPTY, Vec::new()),
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
            // Keep the source annotation on the type-parameter symbol as
            // well as the normalized selection.  The post-pickler method
            // specializer consumes the latter; a separate scalac consumer
            // consumes the former from the generic method's pickle.
            self.st.get_mut(id).annotations = annots.clone();
            // `class C[@specialized(Int, Long) T]`: record what the annotation
            // selects. The method-owned post-pickler phase consumes the same
            // record; class and trait ownership remains for a later phase.
            self.st.record_specialization(id, &annots);
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
        self.resolve_tparam_bounds(tparams);
        ids
    }

    /// Enter type parameters without reporting anything their bounds cannot
    /// resolve yet.
    ///
    /// The namer runs before `import` clauses are processed, so a bound naming
    /// an imported type (`class C[T <: Rep[_]]` under `import slick.lifted._`)
    /// is not resolvable there. A `def`'s parameters are re-entered by
    /// `type_def_sig` and a type member's by `type_type_member`, both of which
    /// run with the imports in scope; `type_class` does the same for a
    /// template's own parameters via `resolve_tparam_bounds`. The namer's
    /// attempt is therefore only provisional and must stay silent, or every
    /// such bound draws a spurious `not found: type X`.
    fn enter_tparams_provisional(
        &mut self,
        tparams: &mut [Tree],
        owner: SymbolId,
    ) -> Vec<SymbolId> {
        let mark = self.diags.len();
        let ids = self.enter_tparams(tparams, owner);
        self.diags.truncate(mark);
        ids
    }

    /// Resolve the `>: lo <: hi` bounds of already-entered type parameters.
    ///
    /// Bounds resolve after every parameter is in scope, so F-bounded
    /// `A <: Comparable[A]` sees `A`.
    ///
    /// A **higher-kinded** parameter writes its bound in terms of its own
    /// parameters: `class PartialOrderFunctions[P[T] <: PartialOrder[T]]`.
    /// `T` belongs to `P`, not to the class, so it is not in the class scope
    /// this runs in from `type_class`; without putting it back, `PartialOrder[T]`
    /// resolved to `Type::Named { name: "T" }` — a name standing for nothing.
    /// `widen_type_param` then could not substitute `P[A]`'s argument into the
    /// bound, and every call through such an `ev: P[A]` reported the bound's
    /// own parameter back: `no matching overload for (T, T)Boolean with
    /// arguments (A, A)`. The namer's provisional pass got this right (it runs
    /// inside `enter_tparams`, where the inner parameters *are* in scope) and
    /// this pass overwrote the good answer with the broken one.
    /// Report the bounds that lead back to the type they bound, and drop them.
    ///
    /// Dropping is not cosmetic: `class_sym_of`, `widen_type_param` and
    /// erasure all replace an abstract type by its upper bound, and a bound
    /// that names its own parameter sends every one of them round for ever.
    /// They defend themselves as well (`symbol::enter_chase`), but a bound
    /// that says nothing is better removed than re-detected at each use.
    pub(crate) fn report_bound_cycles(&mut self, ids: &[(SymbolId, Span)]) {
        if ids.is_empty() {
            return;
        }
        let syms: Vec<SymbolId> = ids.iter().map(|(id, _)| *id).collect();
        for (id, msg) in crate::cyclic::bound_cycles(&self.st, &syms) {
            let Some(span) = ids.iter().find(|(s, _)| *s == id).map(|(_, sp)| *sp) else {
                continue;
            };
            self.error(span, msg);
            self.st.get_mut(id).bound_hi = None;
            self.st.get_mut(id).bound_lo = None;
        }
    }

    pub(crate) fn resolve_tparam_bounds(&mut self, tparams: &[Tree]) {
        for tp in tparams.iter() {
            let TreeKind::TypeDef { lo, hi, .. } = &tp.kind else {
                continue;
            };
            let id = tp.sym;
            if id.is_none() {
                continue;
            }
            let inner = self.st.get(id).tparams.clone();
            if !inner.is_empty() {
                self.st.push_scope();
                for iid in &inner {
                    let name = self.st.get(*iid).name.clone();
                    if name != "_" {
                        self.st.enter_in_current(&name, *iid);
                    }
                }
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
            if !inner.is_empty() {
                self.st.pop_scope();
            }
        }
        let ids: Vec<(SymbolId, Span)> = tparams
            .iter()
            .filter(|tp| !tp.sym.is_none())
            .map(|tp| (tp.sym, tp.span))
            .collect();
        self.report_bound_cycles(&ids);
    }

    /// Is `ty` the `scala.Function` module class?
    ///
    /// A bare `Function` in type position resolves to it, because the symbol
    /// table has the module (nsc's `Function.chain`/`untupled` live there) but
    /// not `Predef`'s `type Function[-A, +B] = Function1[A, B]`. Applied to two
    /// arguments the name means the alias, never the module.
    pub(crate) fn is_scala_function_module(&self, ty: &Type) -> bool {
        self.st
            .class_sym_of(ty)
            .is_some_and(|s| self.st.get(s).jvm_name == "scala/Function$")
    }

    /// Apply `args` to a type constructor, diagnosing kind mismatches.
    pub(crate) fn apply_types(&mut self, ctor: Type, args: Vec<Type>, span: Span) -> Type {
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
            // In a type *pattern* an unbounded wildcard stands for some type
            // of whatever kind the parameter has: nsc accepts
            // `case o: TypedCollectionTypeConstructor[?]` (slick's
            // `ast/Type.scala`) for a `C[_]` parameter. Everywhere else the
            // same wildcard is an existential over a proper type, and nsc
            // reports `_$1 takes no type parameters` -- so this stays inside
            // the pattern.
            if self.pattern_tpt && matches!(a, Type::Wildcard) {
                continue;
            }
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

    pub(crate) fn check_proper_type(&mut self, ty: &Type, span: Span) {
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

    /// The type a context bound's evidence parameter really has.
    ///
    /// `[U : BaseColumnType]` writes the bound as a bare name, so
    /// `tree_to_type` takes the un-applied path and never reaches the
    /// `expand_type_members` that every *written* parameter type goes through
    /// (`tpt_to_type`'s applied-constructor arm). Inside a cake the two then
    /// disagree: `def base[U : BaseColumnType]` in slick's
    /// `JdbcTypesComponent` (self-type `JdbcProfile`) got the *abstract*
    /// `RelationalTypesComponent#BaseColumnType`, while the body's
    /// `implicitly[BaseColumnType[U]]` got `JdbcProfile`'s alias
    /// `JdbcType[U] with BaseTypedType[U]` -- so the only candidate in scope
    /// was the one the search could not match.
    pub(crate) fn expand_bound_evidence(&self, ev_ty: Type) -> Type {
        if self.st.this_class.is_none() {
            return ev_ty;
        }
        self.st.expand_type_members(self.st.this_class, &ev_ty)
    }

    /// nsc: `class C[A <% V](x: A)` → extra implicit ctor clause `(implicit evidence$n: A => V)`.
    /// nsc: `class C[T: Ordering](x: T)` → extra implicit ctor clause `(implicit evidence$n: Ordering[T])`.
    /// Higher-kinded `F[_] <% V` / `F[_]: C` is illegal in scalac 2.13 (`type F takes type parameters`).
    pub(crate) fn class_bound_evidence(
        &mut self,
        class_id: SymbolId,
        tparams: &[Tree],
    ) -> Vec<Tree> {
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
                let ev_ty = self.expand_bound_evidence(apply_context_bound(bound_ty, tp_id));
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

    /// Give a `case class` / `case object` the `scala.Product` and
    /// `java.io.Serializable` parents nsc gives it. See
    /// `crates/typer/src/prelude_product.rs` for what was read off scalac's
    /// own classfiles, and why this is `library_abi`-only.
    pub(crate) fn link_case_product(&mut self, class_id: SymbolId) {
        if !self.library_abi || !crate::prelude_product::wants_product(&self.st, class_id) {
            return;
        }
        let mut syms = Vec::new();
        for jvm in crate::prelude_product::PRODUCT_PARENTS {
            match self.ensure_jvm_class(jvm) {
                Some(s) => syms.push(s),
                // A classpath without `scala.Product` is not one where a
                // half-linked `Product` would help.
                None => return,
            }
        }
        crate::prelude_product::add_parents(&mut self.st, class_id, &syms);
    }

    /// Give a case class's *synthetic* companion the
    /// `scala.runtime.AbstractFunctionN` parent that `P.tupled` / `P.curried`
    /// and `val f: (Int, String) => P = P` come from.
    pub(crate) fn link_case_companion(&mut self, class_id: SymbolId, paramss: &[Vec<Type>]) {
        if !self.library_abi {
            return;
        }
        let Some(module_cls) = crate::prelude_product::synthetic_companion(&self.st, class_id)
        else {
            // A companion the user wrote gets neither parent from here: nsc
            // leaves it alone, and `type_module` would overwrite the list
            // anyway. The backend still marks the classfile `Serializable`.
            return;
        };
        // Every case-class companion is `Serializable`, `AbstractFunctionN` or
        // not (`class E$Gen$ implements java.io.Serializable`).
        if let Some(ser) = self.ensure_jvm_class(crate::prelude_product::SERIALIZABLE) {
            crate::prelude_product::add_parents(&mut self.st, module_cls, &[ser]);
        }
        let Some(jvm) =
            crate::prelude_product::companion_function_class(&self.st, class_id, paramss)
        else {
            return;
        };
        let Some(abs_fn) = self.ensure_jvm_class(&jvm) else {
            return;
        };
        let params = paramss.first().cloned().unwrap_or_default();
        crate::prelude_product::link_companion_function(
            &mut self.st,
            module_cls,
            class_id,
            &params,
            abs_fn,
        );
    }

    /// Load `internal` from the classpath, if it is there, and return its
    /// symbol. Loading is memoized by `load_binary_into`.
    fn ensure_jvm_class(&mut self, internal: &str) -> Option<SymbolId> {
        let pkg = internal.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        let owner = crate::classpath::ensure_package(&mut self.st, pkg);
        self.load_binary_into(internal, owner, Span::new(0, 0), false);
        crate::classpath::find_by_jvm(&self.st, internal)
    }

    /// `productPrefix: String`, `productArity: Int`, `productElement(n: Int): Any`
    /// and `productElementName(n: Int): String`, the four `scala.Product`
    /// members nsc *overrides* in every `case class` and `case object`
    /// (`productIterator` and `productElementNames` it inherits instead). The
    /// backend emits all four; the first two fold to constants.
    ///
    /// Synthesized in both library modes: none of the four mentions a library
    /// type, so the private runtime backs them just as well.
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
        for (name, ret) in [
            ("productElement", Type::Any),
            ("productElementName", Type::String),
        ] {
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
            let p = self.st.alloc("n", id, SymKind::Term, Flags::PARAM, "");
            self.st.get_mut(p).ty = Type::Int;
            self.st.get_mut(id).params = vec![p];
            self.st.get_mut(id).paramss = vec![vec![p]];
            self.st.get_mut(id).ty = Type::Method {
                paramss: vec![vec![Type::Int]],
                ret: Box::new(ret),
            };
        }
    }

    fn synthesize_case_members(&mut self, class_id: SymbolId, name: &str, ctor: &CtorAccess) {
        // `-Xsource-features:case-apply-copy-access`. Off (the 2.13 default)
        // this is `CtorAccess::default()`'s effect: no flags, no qualifier.
        let inherit = self.source_features.case_apply_copy_access();
        let fields = self.st.get(class_id).ctor_fields.clone();
        let class_ty = Type::Class {
            sym: class_id,
            args: vec![],
        };
        // copy, productArity, toString, equals, hashCode as methods (backend will emit)
        let copy = self
            .st
            .alloc("copy", class_id, SymKind::Method, Flags::SYNTHETIC, "");
        if inherit {
            let flags = self.st.get(copy).flags.with(ctor.copy_flags());
            self.st.get_mut(copy).flags = flags;
            self.st.get_mut(copy).private_within = ctor.private_within.clone();
        }
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
            if inherit && ctor.apply_inherits() {
                let flags = self.st.get(apply).flags.with(ctor.apply_flags());
                self.st.get_mut(apply).flags = flags;
                self.st.get_mut(apply).private_within = ctor.private_within.clone();
            }
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

    pub(crate) fn namer_module(&mut self, tree: &mut Tree) {
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
                // `cls`'s own members, available immediately. A package
                // object can also *inherit* an exported name -- `package
                // object data extends ScalaVersionSpecificPackage` exports
                // `type NonEmptyLazyList`, declared on the parent, not in the
                // package object's own body -- but `cls`'s parents are not
                // reliable yet: `rough_parents`, run earlier in this same
                // call, cannot resolve a parent declared in a file namer has
                // not reached. `pending_pkg_folds` redoes this with
                // `members_including_inherited` once the header pass has
                // resolved every unit's parents for real.
                let mems = self.st.get(cls).members.clone();
                for mem in mems {
                    if !self.st.get(pkg).members.contains(&mem) {
                        self.st.get_mut(pkg).members.push(mem);
                    }
                }
                self.pending_pkg_folds.push((pkg, cls));
            }
        }
    }

    pub(crate) fn namer_member(&mut self, tree: &mut Tree) {
        match &tree.kind {
            TreeKind::ValDef {
                name, mods, rhs, ..
            } => {
                let annots = mods.annotations.clone();
                // `val v: Int` with no right-hand side is nsc's DEFERRED; see
                // `Symbol::deferred_val` for why the flag word cannot say so.
                let deferred = rhs.is_empty();
                let id = self
                    .st
                    .alloc(name, self.st.owner, SymKind::Term, mods.flags, "");
                self.st.get_mut(id).private_within = mods.private_within.clone();
                self.st.get_mut(id).annotations = annots;
                self.st.get_mut(id).deferred_val = deferred;
                self.st.enter_in_current(name, id);
                tree.sym = id;
            }
            TreeKind::DefDef {
                name, mods, rhs, ..
            } => {
                let annots = mods.annotations.clone();
                // Record `abstract override` *before* the body-less `def`
                // rule below adds `ABSTRACT`, which would make the two
                // indistinguishable.
                // A body-less `abstract override def m: T` is simply
                // deferred: nothing is stacked on top of `super`, and nsc
                // pickles it DEFERRED like any other declaration. Only the
                // concrete form is `ABSOVERRIDE`.
                let abs_over = !rhs.is_empty()
                    && mods.flags.contains(Flags::ABSTRACT)
                    && mods.flags.contains(Flags::OVERRIDE);
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
                self.st.get_mut(id).abstract_override = abs_over;
                // `@unspecialized def f` opts one member out of its owner's
                // specialization. Recorded, not yet acted on.
                let annots = self.st.get(id).annotations.clone();
                self.st.record_specialization(id, &annots);
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
                        let tp_ids = self.enter_tparams_provisional(tparams, id);
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
    pub(crate) fn parents_pass(&mut self, tree: &mut Tree, ctors: bool) -> bool {
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
        // Every import in the body first, then the aliases, then the nested
        // templates: resolving a nested template's parent clause can complete
        // one of this template's type aliases, and the alias's right-hand side
        // is written in this unit's vocabulary. The header pass exists only to
        // resolve parents and its diagnostics are dropped, so hoisting the
        // imports above the templates that follow them costs nothing.
        for stt in body.iter_mut() {
            if matches!(stt.kind, TreeKind::Import { .. }) {
                self.type_import(stt);
            }
        }
        self.refresh_alias_sigs(body);
        for stt in body.iter_mut() {
            if matches!(
                stt.kind,
                TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
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
}
