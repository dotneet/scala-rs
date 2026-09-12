#![allow(dead_code)]
//! Turning a type tree into a `Type`, and getting the members of a type from
//! wherever they live.
//!
//! `tree_to_type` and the projection machinery around it: type members seen
//! from a prefix, path-dependent and singleton types, compound and refinement
//! types, existentials, and qualified type lookup. The second half is supply:
//! completing a class, module or member out of a pickle or a Java class file,
//! and warming the implicit scopes that a search will need.

use crate::check::*;
use crate::implicits::ImplicitSearch;
use crate::symbol::{BindRank, SymKind, SymbolTable};
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

impl Typer {
    /// `ty` is what `resolve_type_name` made of `name`. Under
    /// [`Self::strict_type_names`], a `Type::Named` still carrying that very
    /// name means the lookup found nothing at all -- `expose_unqualified` has
    /// already tried every package, wildcard import and pickle -- so report it
    /// the way nsc does instead of handing a placeholder to the rest of the
    /// run.
    fn reject_unresolved_type(&mut self, ty: Type, name: &str, span: Span) -> Type {
        if !self.strict_type_names {
            return ty;
        }
        if self.exist_quantified.iter().any(|q| q == name) {
            return ty;
        }
        match &ty {
            Type::Named { name: n, args } if args.is_empty() && n == name => {
                self.not_found_error(span, "type", name);
                Type::Error
            }
            _ => ty,
        }
    }

    /// `q` in a type `q.T` denotes nothing at all: no term, no type, no
    /// package, nowhere this pass can reach.
    ///
    /// This is what keeps the bare-name fallback in `tree_to_type`'s
    /// `TreeKind::Select` arm from answering a missing import with whatever
    /// happens to share the simple name. The fallback is there for paths this
    /// pass cannot model -- a `type` alias a jar class declares leaves no
    /// trace in the bytecode, so `HtmlFormat.Appendable`, which Twirl writes
    /// in the parents clause of every generated template, has no symbol for
    /// `lookup_qualified_type` to find -- and in every one of those the
    /// *qualifier* still resolves: `HtmlFormat` is a module in a jar. When it
    /// resolves to nothing there is no path left to model, and nsc's answer
    /// is `not found: value q`. Falling back on `T` alone made
    /// `trait AllOps[A] extends Ops[A] with Missing.AllOps[A]` inherit from
    /// the very trait being defined.
    ///
    /// Only a plain `Ident` qualifier is judged. A longer prefix has more
    /// ways to be a path this pass cannot enumerate, and one whose own head
    /// is missing already reaches `missing_qualified_type`'s `Select` arm by
    /// the ordinary route.
    ///
    /// Restricted to [`Self::strict_type_names`] -- parents clauses, and
    /// signatures outside a file whose scope this compiler cannot enumerate
    /// -- for the same reason the sibling `missing_qualified_type` call in
    /// that arm is: those are the positions where every name a file writes
    /// has already been given every chance to resolve.
    fn qualifier_names_nothing(&mut self, qual: &Tree) -> bool {
        if !self.strict_type_names {
            return false;
        }
        let TreeKind::Ident { name } = &qual.kind else {
            return false;
        };
        let name = name.clone();
        if self.exist_quantified.iter().any(|q| *q == name) {
            return false;
        }
        // Both exposures run every open package, wildcard import and pickle
        // before answering; `qualified_type_owners` calls `expose_unqualified`
        // itself.
        if !self.qualified_type_owners(qual).is_empty() {
            return false;
        }
        self.expose_unqualified_type(&name, qual.span);
        self.st.lookup(&name).is_empty() && !self.st.has_real_type_entry(&name)
    }

    /// `qual.name` denotes no type. Report it the way nsc does: blame the
    /// leftmost segment that does not resolve, so `p2.sub.Foo` with no `sub`
    /// is `object sub is not a member of package p2` and not a complaint about
    /// `Foo`.
    fn missing_qualified_type(&mut self, qual: &Tree, name: &str, span: Span) -> Type {
        if let Some(owner) = self.qualified_type_owners(qual).first().copied() {
            let desc = self.owner_desc(owner);
            self.error(span, format!("type {name} is not a member of {desc}"));
            return Type::Error;
        }
        match &qual.kind {
            TreeKind::Select {
                qual: inner,
                name: seg,
            } => {
                let (inner, seg) = ((**inner).clone(), seg.clone());
                if let Some(owner) = self.qualified_type_owners(&inner).first().copied() {
                    // nsc names the owner of a missing *package segment* by
                    // its simple name (`package collection`), and the owner of
                    // a missing type by its full one (`package java.util`).
                    let s = self.st.get(owner);
                    let short = s.name.trim_end_matches('$').to_string();
                    let desc = match s.kind {
                        SymKind::Package => format!("package {short}"),
                        SymKind::Module | SymKind::ModuleClass => format!("object {short}"),
                        _ => short,
                    };
                    self.error(qual.span, format!("object {seg} is not a member of {desc}"));
                    return Type::Error;
                }
                self.missing_qualified_type(&inner, &seg, qual.span)
            }
            TreeKind::Ident { name: head } => {
                // SLS 3.2.3: the prefix of a type `p.T` is a *term*, so nsc
                // reports the value it could not find, not a type.
                let head = head.clone();
                self.not_found_error(qual.span, "value", &head);
                Type::Error
            }
            _ => {
                self.error(span, format!("not found: type {name}"));
                Type::Error
            }
        }
    }

    /// [`Self::with_strict_type_names`] for a *signature* — a parameter, a
    /// field, a result type. Unlike a parents clause this covers every name a
    /// file writes, so it stands down in a file whose scope this compiler
    /// cannot enumerate; see [`Self::opaque_import_files`].
    pub(crate) fn with_strict_sig_names<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        if self.opaque_import_files.contains(&self.file_index) {
            return f(self);
        }
        self.with_strict_type_names(f)
    }

    /// Run `f` with [`Self::strict_type_names`] on.
    pub(crate) fn with_strict_type_names<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = std::mem::replace(&mut self.strict_type_names, true);
        let r = f(self);
        self.strict_type_names = saved;
        r
    }

    pub(crate) fn tree_to_type(&mut self, tpt: &Tree) -> Type {
        match &tpt.kind {
            TreeKind::Empty => Type::NoType,
            // nsc's `TypeTree(tp)`: a type the compiler already knows,
            // standing where the source would have written a path. Built by
            // `crate::materialize`, which has the `Type` and no name to
            // reach it by at the use site.
            TreeKind::Ident { name } if name == crate::materialize::RESOLVED_TYPE => tpt.ty.clone(),
            TreeKind::Ident { name } if name == "_" => Type::Wildcard,
            TreeKind::Ident { name } => {
                self.expose_unqualified(name, tpt.span);
                self.expose_unqualified_type(name, tpt.span);
                let name = name.clone();
                let ty = self.resolve_type_name_completing(&name, &[], tpt.span);
                // Only a *written* name: a tree the compiler built (the
                // conversion an implicit class desugars to) arrives resolved.
                if self.any_cto && tpt.sym.is_none() {
                    // The name is the reference: an alias `type Al = K` is
                    // not a reference to `K` (nsc checks the symbol the tree
                    // names), and by now it has been expanded.
                    let named = self.st.lookup_type(&name).first().copied();
                    let sym = match (named, &ty) {
                        (Some(n), _) if self.st.get(n).kind == SymKind::TypeMember => Some(n),
                        (
                            _,
                            Type::Class { sym, .. } | Type::TypeMember(sym) | Type::ModuleRef(sym),
                        ) => Some(*sym),
                        (n, _) => n,
                    };
                    if let Some(sym) = sym {
                        self.note_cto_ref(sym, tpt.span);
                    }
                }
                let ty = self.reject_unresolved_type(ty, &name, tpt.span);
                let ty = self.this_prefixed(ty);
                self.import_prefixed(&name, ty, tpt.span)
            }
            TreeKind::Select { name, qual } => {
                if let TreeKind::Ident { name: q } = &qual.kind {
                    let q = q.clone();
                    self.expose_unqualified(&q, tpt.span);
                }
                if self.type_select_is_term_prefix(qual) {
                    self.path_dependent_type(tpt.span, qual, name)
                } else if let Some(t) = scala_value_type(qual, name) {
                    t
                } else if let Some(id) = self.lookup_qualified_type(qual, name) {
                    self.complete_lazy_sig(id, tpt.span);
                    self.note_cto_ref(id, tpt.span);
                    match self.st.get(id).kind {
                        SymKind::Module | SymKind::ModuleClass => Type::ModuleRef(id),
                        SymKind::TypeParam => Type::TypeParam(id),
                        SymKind::TypeMember => Type::TypeMember(id),
                        _ if id == self.st.string_sym => Type::String,
                        _ => self.module_prefix_view(qual, id).unwrap_or(Type::Class {
                            sym: id,
                            args: vec![],
                        }),
                    }
                } else if self.qualifier_names_nothing(qual) {
                    // The qualifier itself denotes nothing -- see
                    // [`Self::qualifier_names_nothing`]. There is no path here
                    // for this pass to fail to model, so the bare-name
                    // fallback below would answer with an unrelated class that
                    // merely shares the simple name.
                    self.missing_qualified_type(qual, name, tpt.span)
                } else if self.qualifier_is_package(qual) {
                    // A resolved package cannot supply an unrelated bare type.
                    // Its package-object aliases still need lazy pickle lookup.
                    self.qualified_pickled_type_member(qual, name)
                        .unwrap_or_else(|| self.missing_qualified_type(qual, name, tpt.span))
                } else {
                    // The prefix knows nothing of that name. Falling back on
                    // the *bare* name is deliberate -- a path this pass cannot
                    // model still resolves that way -- but when that fails too
                    // the clause names nothing at all.
                    let ty = self.resolve_type_name(name, &[]);
                    match &ty {
                        Type::Named { name: n, args } if args.is_empty() && n == name => {
                            let name = name.clone();
                            let qual = (**qual).clone();
                            // A `type` alias a jar class declares leaves no
                            // trace in the bytecode, so `lookup_qualified_type`
                            // -- which can only see symbols -- finds nothing
                            // until something else happens to adopt the
                            // pickle. Twirl writes `HtmlFormat.Appendable` in
                            // the parents clause of every generated template,
                            // which is exactly where nothing has.
                            //
                            // The pickle is asked whether or not
                            // [`Self::strict_type_names`] is on. Only the
                            // *diagnostic* is strict: outside a parents clause
                            // an unresolved `p.T` still falls back to the
                            // placeholder `Type::Named`, because a path this
                            // pass cannot model resolves that way. But a
                            // placeholder is not an answer, and a template's
                            // `def apply(...): HtmlFormat.Appendable` is an
                            // ordinary signature, not a parent -- so the
                            // alias resolved in the parents clause and stayed
                            // a bare name everywhere else in the same file.
                            if let Some(t) = self.qualified_pickled_type_member(&qual, &name) {
                                t
                            } else if self.strict_type_names {
                                self.missing_qualified_type(&qual, &name, tpt.span)
                            } else {
                                ty
                            }
                        }
                        _ => ty,
                    }
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
                let t = self.project_from_prefix(tpt.span, &prefix, name);
                if *hash {
                    self.mark_type_projection(&prefix, t)
                } else {
                    t
                }
            }
            TreeKind::CompoundTypeTree {
                parents,
                refinements,
            } => self.compound_to_type(tpt.span, parents, refinements),
            TreeKind::SingletonTypeTree { ref_ } => self.singleton_to_type(tpt.span, ref_),
            TreeKind::AnnotatedTypeTree { tpt: inner, annot } => {
                let ty = self.tree_to_type(inner);
                self.note_cto_type_name(annot, annot.span);
                let path = annot.annotation_path();
                let mut simple = path.rsplit('.').next().unwrap_or(path.as_str()).to_string();
                // The variance checks look for `uncheckedVariance` by name;
                // slick writes it through a renaming import (`import
                // scala.annotation.unchecked.{uncheckedVariance => uv}`, then
                // `type Self = HCons[H @uv, T @uv]`), so resolve the written
                // name and keep the annotation's own.
                if simple != "uncheckedVariance"
                    && self.st.lookup_type(&simple).iter().any(|s| {
                        self.st.get(*s).jvm_name == "scala/annotation/unchecked/uncheckedVariance"
                    })
                {
                    simple = "uncheckedVariance".to_string();
                }
                Type::Annotated {
                    tpe: Box::new(ty),
                    annot: simple,
                }
            }
            TreeKind::AppliedTypeTree { tpt, args } => {
                let span = tpt.span;
                let arg_mark = self.diags.len();
                let mut as_ = Vec::new();
                for a in args {
                    as_.push(self.tree_to_type(a));
                }
                let head_mark = self.diags.len();
                let applied = match tpt.name() {
                    // These are parser markers, not source-level names.
                    Some("<repeated>") => {
                        Type::Repeated(Box::new(as_.first().cloned().unwrap_or(Type::Any)))
                    }
                    Some("<ByName>") if tpt.byname_type_marker => {
                        Type::ByName(Box::new(as_.first().cloned().unwrap_or(Type::Error)))
                    }
                    Some("<tuple>") => Type::Tuple(as_),
                    Some(name) if name.starts_with("<Function") && name.ends_with('>') => {
                        let ret = as_.pop().unwrap_or(Type::Error);
                        Type::Function {
                            params: as_,
                            ret: Box::new(ret),
                        }
                    }
                    Some(_) => {
                        let ctor = self.tree_to_type(tpt);
                        // The private runtime's prelude uses the canonical
                        // Function module as the Function[A,B] alias carrier.
                        // A user declaration with this name is not that symbol.
                        if as_.len() == 2 && self.is_scala_function_module(&ctor) {
                            Type::Function {
                                params: vec![as_[0].clone()],
                                ret: Box::new(as_[1].clone()),
                            }
                        } else {
                            let applied = self.apply_types(ctor.clone(), as_, span);
                            // A parameterised alias of an inner class (`type
                            // Table[T] = RelationalProfile.this.Table[T]`)
                            // expands only now, so its `C.this` is read
                            // through the import or the path here, the way
                            // `import_prefixed` / `project_from_prefix_at`
                            // read a nullary one (`prefix.rs`).
                            let applied = self.applied_alias_prefixed(tpt, applied, span);
                            self.with_prefix_if_type_member(tpt, &ctor, applied)
                        }
                    }
                    None => Type::Error,
                };
                // nsc's error type is absorbing: when the type constructor
                // names nothing, its arguments are not reported as well.
                // `-Ykind-projector` passes an unrecognised
                // `Functor[λ[α => Box[α], β]]` through untouched, and
                // `α`/`β` are then names nobody wrote a binder for -- one
                // diagnostic about `λ`, not three. Only the *arguments*'
                // diagnostics are dropped, and only when the head itself
                // produced one, so `def f(x: List[Zork])` still reports `Zork`.
                if self.diags[head_mark..]
                    .iter()
                    .any(|d| d.span == tpt.span && d.level == scala_rs_span::Level::Error)
                {
                    self.diags.drain(arg_mark..head_mark);
                }
                applied
            }
            TreeKind::TypeApply { fun, args } => {
                // `new C[T]` in term position may be TypeApply; treat it as a type.
                let mut as_ = Vec::new();
                for a in args {
                    as_.push(self.tree_to_type(a));
                }
                let ctor = self.tree_to_type(fun);
                self.apply_types(ctor, as_, fun.span)
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
                // The quantified names are bound by `subst_quantified` *after*
                // the body is resolved, so within the body they resolve to
                // nothing. Announce them first -- all of them, since a bound
                // may name a later clause -- so that a strict type position
                // does not report them as missing.
                let exist_depth = self.exist_quantified.len();
                for c in clauses {
                    if let TreeKind::TypeDef { name, .. } = &c.kind {
                        self.exist_quantified.push(name.clone());
                    }
                }
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
                            self.exist_quantified.truncate(exist_depth);
                            return packed;
                        }
                    }
                    for (_, _, sp) in &val_clauses {
                        self.error(
                            *sp,
                            "unimplemented type: value existential (`forSome { val … }`)",
                        );
                    }
                    self.exist_quantified.truncate(exist_depth);
                    return Type::Error;
                }
                let ty = self.tree_to_type(inner);
                self.exist_quantified.truncate(exist_depth);
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
    pub(crate) fn type_new_prefix(&mut self, tpt: &mut Tree) {
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
    ///
    /// SLS 3.2.3: the dot in a type `p.T` always evaluates `p` as a term (a
    /// stable path); `#` is the syntax for a genuine type projection. A
    /// package object that re-exports a jar's module with both a `type` alias
    /// and a `val` of the same name -- exactly what `cats.effect`'s package
    /// object does for `Resource` and `Outcome`, so `import
    /// cats.effect.Resource` brings in both -- used to make this return
    /// `false`: a name that resolved to *any* type-like symbol (the alias)
    /// vetoed the term reading even when a term of the same name also
    /// existed. `Resource.ExitCase` then went through `project_from_prefix`
    /// with `Resource` read as the *type* alias's dealiased class (the
    /// trait), not the module -- so it could never see `ExitCase`, a member
    /// only the module installs. A term/module denotation always wins the
    /// dot, regardless of what else shares the name.
    fn type_select_is_term_prefix(&self, t: &Tree) -> bool {
        if !t.sym.is_none() && matches!(self.st.get(t.sym).kind, SymKind::Term | SymKind::Method) {
            return true;
        }
        match &t.kind {
            TreeKind::This { .. } | TreeKind::Super { .. } => true,
            TreeKind::Ident { name } => {
                let found = self.st.lookup(name);
                // Deliberately *not* `SymKind::Module`: `new Outer.Inner()`
                // must still go through `qualified_type_owners`, whose
                // `type_owner_rank` already knows to prefer the module over
                // the class for `p.T` and, unlike this path, disambiguates a
                // class from its own companion of the same name --
                // `path_dependent_type` has no such preference and bound
                // `Outer.Inner` to nothing. Only an actual term (`val`,
                // parameter, or a `def`-shaped accessor) forces the term
                // reading.
                found
                    .iter()
                    .any(|s| matches!(self.st.get(*s).kind, SymKind::Term | SymKind::Method))
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

    /// `A#B` (and `a.B`) where `B` is nested in a class `O` that `A` extends.
    ///
    /// `B`'s members are written in `O`'s vocabulary, so an abstract type
    /// member of `O` that `A` makes concrete has to be read at `A`'s
    /// definition: slick's `HeapBackend#BasicActionContext` inherits
    /// `def session: Session` from `BasicBackend.BasicActionContext`, and
    /// `Session` is `BasicBackend`'s abstract member -- `HeapSessionDef` only
    /// through `HeapBackend`.
    ///
    /// `Type::Class` has no room for a prefix, so the projection would drop
    /// that fact and every later selection would read the abstract member
    /// (`value database is not a member of BasicBackend.Session`). Pin what
    /// the prefix settles onto the result as a type-only refinement instead;
    /// `expand_in_type` / `subst_as_seen_from` already read refinements, and
    /// erasure discards a refinement with no term members, so the projected
    /// type still erases to `B`.
    fn projected_class_type(&mut self, prefix: &Type, pcls: SymbolId, member: SymbolId) -> Type {
        let base = Type::Class {
            sym: member,
            args: vec![],
        };
        let decls = self.projection_refinements(prefix, pcls, member);
        let t = Self::as_seen_from(base, decls);
        // An inner class of a class is a different type per enclosing
        // instance, and the prefix is what tells them apart and what
        // instantiates the enclosing class's type parameters in its members
        // (`prefix.rs`). A type prefix is the projection `P#In`; a stable
        // path gets its singleton from `path_dependent_type`.
        if self.st.is_inner_class_of_class(member) {
            crate::prefix::with_prefix(t, prefix.clone())
        } else {
            t
        }
    }

    /// The singleton type of a stable path written as a type prefix
    /// (`p.In`, `a.b.In`, `this.In`, `O.In`), or `None` when the path is
    /// not one this compiler can name (a `super`, a package).
    ///
    /// Unlike `singleton_to_type`, which keeps only the last term, every
    /// term of the path is kept: `x.p.In` and `p.In` are two types.
    pub(crate) fn singleton_prefix_of(&self, path: &Tree) -> Option<Type> {
        match &path.kind {
            // `super.Builder` in type position: the parent's member, but the
            // instance is this one -- nsc's `Mid.super.type` is `Mid.this`.
            // Read as the parent's *projection* it was a different prefix
            // from the `Main.this.Builder` a member declared bare expects.
            TreeKind::Super { .. } => {
                (!self.st.this_class.is_none()).then_some(Type::ThisType(self.st.this_class))
            }
            TreeKind::This { qual } => {
                let id = match qual {
                    Some(name) => self
                        .st
                        .enclosing_class_named(self.st.this_class, name)
                        .unwrap_or(self.st.this_class),
                    None => self.st.this_class,
                };
                (!id.is_none()).then_some(Type::ThisType(id))
            }
            TreeKind::Ident { name } => {
                let sym = if !path.sym.is_none()
                    && matches!(
                        self.st.get(path.sym).kind,
                        SymKind::Term | SymKind::Method | SymKind::Module | SymKind::ModuleClass
                    ) {
                    path.sym
                } else {
                    self.st
                        .lookup_term(name)
                        .into_iter()
                        .find(|s| self.names_a_singleton(*s))?
                };
                Some(self.singleton_of_sym(sym, None))
            }
            TreeKind::Select { qual, name }
            | TreeKind::SelectFromTypeTree {
                qual,
                name,
                hash: false,
            } => {
                let qpre = self.singleton_prefix_of(qual);
                let sym = if !path.sym.is_none()
                    && matches!(
                        self.st.get(path.sym).kind,
                        SymKind::Term | SymKind::Method | SymKind::Module | SymKind::ModuleClass
                    ) {
                    path.sym
                } else {
                    let qty = self.term_path_type(qual)?;
                    let cls = self.st.class_sym_of(&qty)?;
                    self.st
                        .lookup_member(cls, name)
                        .into_iter()
                        .find(|s| self.names_a_singleton(*s))?
                };
                Some(self.singleton_of_sym(sym, qpre))
            }
            _ => None,
        }
    }

    /// `sym.type` under the prefix `qpre` (the enclosing path), or under the
    /// owner's `this` for a member reached bare, or under nothing for a local.
    fn singleton_of_sym(&self, sym: SymbolId, qpre: Option<Type>) -> Type {
        let s = self.st.get(sym);
        if matches!(s.kind, SymKind::Module | SymKind::ModuleClass) {
            // An object nested in a class is one value *per enclosing
            // instance*, so `p.O` keeps `p` (`rewrite_view_this` steps out
            // through it); a top-level or object-nested object is one value.
            let mcls = self.st.module_class_of(sym);
            let owner = s.owner;
            let per_instance = !owner.is_none()
                && self.st.get(owner).kind == SymKind::Class
                && !self.st.get(owner).flags.contains(Flags::MODULE);
            return match qpre {
                Some(p) if per_instance => Type::SingleType {
                    prefix: Box::new(p),
                    sym: mcls,
                },
                None if per_instance => Type::SingleType {
                    prefix: Box::new(Type::ThisType(owner)),
                    sym: mcls,
                },
                _ => Type::ModuleRef(mcls),
            };
        }
        // A class's self alias is another spelling of its `this`.
        if qpre.is_none() && !s.owner.is_none() && self.st.get(s.owner).self_alias == Some(sym) {
            return Type::ThisType(s.owner);
        }
        let prefix = match qpre {
            Some(p) => p,
            None => {
                let owner = s.owner;
                if owner.is_none() || !self.st.get(owner).is_class_like() {
                    Type::NoType
                } else {
                    Type::ThisType(owner)
                }
            }
        };
        Type::SingleType {
            prefix: Box::new(prefix),
            sym,
        }
    }

    /// The enclosing instance an inner class's constructor call needs, written
    /// into the head of the `new` / parent clause as a qualifier the backend
    /// evaluates (`gen_expr::new_prefix_instance`).
    ///
    /// `import prof.api._; class Mine(k: Int) extends Inner(k)` -- gitbucket's
    /// every table -- names `Inner` through an alias `type Inner = self.Inner`
    /// of `prof.api`'s trait, and the instance `Inner` belongs to is `prof`:
    /// the prefix the alias expanded to (`prefix.rs`), not the import's
    /// `prof.api` and not `Comp.this`, which is what the backend's fallback
    /// loaded (a `ClassCastException` at run time). The head keeps its type;
    /// only its qualifier is (re)written, from the prefix's path. A `this`
    /// prefix is left to the outer-instance chain, which already handles it.
    pub(crate) fn qualify_inner_ctor_head(&mut self, head: &mut Tree) {
        let ty = head.ty.clone();
        self.qualify_inner_ctor_head_in(head, &ty);
    }

    fn qualify_inner_ctor_head_in(&mut self, head: &mut Tree, ty: &Type) {
        match &mut head.kind {
            TreeKind::AppliedTypeTree { tpt, .. }
            | TreeKind::TypeApply { fun: tpt, .. }
            | TreeKind::AnnotatedTypeTree { tpt, .. } => {
                return self.qualify_inner_ctor_head_in(tpt, ty);
            }
            TreeKind::Ident { .. } | TreeKind::Select { .. } => {}
            _ => return,
        }
        let Some(pre) = crate::prefix::view_prefix(ty).cloned() else {
            return;
        };
        if !matches!(pre, Type::SingleType { .. } | Type::ModuleRef(_)) {
            return;
        }
        let Type::Class { sym, .. } = crate::prefix::strip_view(ty).clone() else {
            return;
        };
        let owner = self.st.get(sym).owner;
        if owner.is_none() || !self.st.get(owner).is_class_like() {
            return;
        }
        let reaches = |st: &SymbolTable, t: &Type| {
            st.class_sym_of(t)
                .is_some_and(|c| c == owner || st.is_ancestor_of(owner, c))
        };
        // Already written through the enclosing instance (`new prof.Inner`).
        if let TreeKind::Select { qual, .. } = &head.kind {
            if reaches(&self.st, &qual.ty) {
                return;
            }
        }
        let Some(mut q) = self.path_tree_of(&pre, head.span) else {
            return;
        };
        let mark = self.diags.len();
        self.type_expr(&mut q, &Type::NoType);
        if self.error_count_since(mark) > 0
            || q.ty.is_no_type()
            || q.ty.is_error()
            || !reaches(&self.st, &q.ty)
        {
            self.diags.truncate(mark);
            return;
        }
        let name = match &head.kind {
            TreeKind::Ident { name } | TreeKind::Select { name, .. } => name.clone(),
            _ => return,
        };
        // The alias's name may differ from the class's (`type Table = ...`);
        // the qualifier's *class* has the class itself under its own name.
        let name = if self.st.get(sym).name == name {
            name
        } else {
            self.st.get(sym).name.clone()
        };
        head.kind = TreeKind::Select {
            qual: Box::new(q),
            name,
        };
    }

    /// A tree that evaluates to the value of the singleton `pre`, for
    /// `qualify_inner_ctor_head`; typed by the caller. `None` when `pre` is
    /// not a path this compiler can spell here.
    fn path_tree_of(&self, pre: &Type, span: Span) -> Option<Tree> {
        let mut t = match pre {
            Type::ThisType(c) if !c.is_none() => {
                let qual = if *c == self.st.this_class {
                    None
                } else {
                    Some(self.st.get(*c).name.clone())
                };
                Tree::dummy(TreeKind::This { qual })
            }
            Type::ModuleRef(m) if !m.is_none() => Tree::dummy(TreeKind::Ident {
                name: self.st.get(*m).name.trim_end_matches('$').to_string(),
            }),
            Type::SingleType { prefix, sym } if !sym.is_none() => {
                let name = self.st.get(*sym).name.clone();
                match prefix.as_ref() {
                    // A local, or a member reached bare: the name resolves.
                    Type::NoType | Type::ThisType(_) => Tree::dummy(TreeKind::Ident { name }),
                    p => Tree::dummy(TreeKind::Select {
                        qual: Box::new(self.path_tree_of(p, span)?),
                        name,
                    }),
                }
            }
            _ => return None,
        };
        t.span = span;
        Some(t)
    }

    /// A type named through `import p._` for a value `p`: what it names is
    /// read through `p` (`prefix.rs`). `import prof.api._` brings in `type
    /// Inner = self.Inner` of `prof.api`'s trait, and that alias means
    /// `prof.Inner` here -- the instance `prof`, not `Api.this` -- which is
    /// what an inner class's constructor needs to know. Left alone when the
    /// name is not an import's or the enclosing class has it itself.
    pub(crate) fn import_prefixed(&mut self, name: &str, ty: Type, span: Span) -> Type {
        if !self.st.mentions_inner_class(&ty) {
            return ty;
        }
        // The binding that produced `ty`: the class itself, or an alias
        // whose right-hand side names it. Several symbols may answer to the
        // name (a class and a same-named alias of it, say).
        let core = self.st.class_sym_of(crate::prefix::strip_view(&ty));
        let Some(sym) = self.st.lookup_type(name).into_iter().find(|&s| {
            let info = self.st.get(s);
            match info.kind {
                SymKind::Class => Some(s) == core,
                SymKind::TypeMember => self.st.class_sym_of(&info.ty) == core,
                _ => false,
            }
        }) else {
            return ty;
        };
        let owner = self.st.get(sym).owner;
        if owner.is_none() || !self.st.get(owner).is_class_like() {
            return ty;
        }
        let Some(mut q) = self.term_import_prefix_for(owner) else {
            return ty;
        };
        let mark = self.diags.len();
        q.span = span;
        self.type_expr(&mut q, &Type::NoType);
        if self.error_count_since(mark) > 0 || q.ty.is_no_type() || q.ty.is_error() {
            self.diags.truncate(mark);
            return ty;
        }
        self.warm_enclosing_parents(&q.ty);
        let qpre = self.singleton_prefix_of(&q);
        self.st.subst_as_seen_from_at(&q.ty, qpre.as_ref(), &ty)
    }

    /// `applied` is `tpt[args]` for a parameterised alias: read it through
    /// the import (`Ident`) or the term path (`p.T[args]`) the alias was
    /// named by. Anything else is handed back unchanged.
    fn applied_alias_prefixed(&mut self, tpt: &Tree, applied: Type, span: Span) -> Type {
        if !self.st.mentions_inner_class(&applied) {
            return applied;
        }
        match &tpt.kind {
            TreeKind::Ident { name } => {
                let name = name.clone();
                self.import_prefixed(&name, applied, span)
            }
            TreeKind::Select { qual, .. } if self.type_select_is_term_prefix(qual) => {
                let Some(pty) = self.term_path_type(qual) else {
                    return applied;
                };
                let Some(at) = self.singleton_prefix_of(qual) else {
                    return applied;
                };
                self.warm_enclosing_parents(&pty);
                self.st.rewrite_view_this(&pty, Some(&at), &applied)
            }
            _ => applied,
        }
    }

    /// Attach the pickled parents of the class `ty` names and of every class
    /// enclosing it, so that `rewrite_view_this` can tell which enclosing
    /// class a `C.this` prefix belongs to (`is_ancestor_of` reads parents).
    /// slick's `JdbcProfile#API` inherits `type Table[T] =
    /// RelationalProfile.this.Table[T]`, and `RelationalProfile` is only
    /// known to be an ancestor of `JdbcProfile` once its parents are in.
    pub(crate) fn warm_enclosing_parents(&mut self, ty: &Type) {
        if !self.library_abi {
            return;
        }
        let Some(cls) = self.st.class_sym_of(crate::prefix::strip_view(ty)) else {
            return;
        };
        for c in self.st.enclosing_classes(cls) {
            if self.st.get(c).is_class_like() {
                self.pickle
                    .ensure_parents(&mut self.st, &mut self.binary, c);
            }
        }
    }

    /// `In` written bare inside a class that has it as a member (declared or
    /// inherited): nsc's `C.this.In` for the innermost such enclosing class
    /// `C`. A class reached some other way (an import, a local) keeps its
    /// bare, unknown prefix.
    pub(crate) fn this_prefixed(&self, ty: Type) -> Type {
        let Type::Class { sym, .. } = &ty else {
            return ty;
        };
        if !self.st.is_inner_class_of_class(*sym) || self.st.this_class.is_none() {
            return ty;
        }
        let owner = self.st.get(*sym).owner;
        let c = self.ident_prefix_class(owner);
        if c.is_none() || (c != owner && !self.st.is_ancestor_of(owner, c)) {
            return ty;
        }
        crate::prefix::with_prefix(ty, Type::ThisType(c))
    }

    /// `M.C` where `M` is an object and `C` a class it *inherits* from the
    /// class or trait that declares it: the same as-seen-from view `M.type#C`
    /// gets ([`Self::projected_class_type`]), so what `M` settles reaches
    /// `C`'s members. `object IntBase extends Base { type T = Int }` makes
    /// `(new IntBase.Inner).set(1)` take an `Int`; read as the bare class it
    /// took `Base`'s abstract `T`. `None` when `M` declares `C` itself or
    /// the qualifier is not an object.
    fn module_prefix_view(&mut self, qual: &Tree, id: SymbolId) -> Option<Type> {
        let owner = self.st.get(id).owner;
        if owner.is_none() || !matches!(self.st.get(owner).kind, SymKind::Class) {
            return None;
        }
        for o in self.qualified_type_owners(qual) {
            let mcls = match self.st.get(o).kind {
                SymKind::Module => self.st.module_class_of(o),
                SymKind::ModuleClass => o,
                _ => continue,
            };
            if mcls == owner || !self.st.is_ancestor_of(owner, mcls) {
                continue;
            }
            let prefix = Type::ModuleRef(mcls);
            let t = self.projected_class_type(&prefix, mcls, id);
            return (!matches!(t, Type::Class { .. })).then_some(t);
        }
        None
    }

    /// `A#B` written with a type prefix, where `B`'s enclosing class leaves an
    /// abstract type member that `A` does not settle: record that on the view
    /// ([`crate::symbol::PROJECTION_MARK`]), so that a member selected
    /// through it reads that type member as a fresh abstract type in
    /// parameter positions (`Typer::opaque_projection_params`).
    ///
    /// nsc: for `a: Base#Inner`, `a.set(y)` with `set(y: T)` expects
    /// `_1.T forSome { val _1: Base }` -- one particular, unknown `Base`'s
    /// `T` -- and nothing but `Nothing` conforms (`neg/sabin2`). Anything
    /// else (a stable path `x.Inner`, a prefix that settles every member)
    /// is left exactly as it was.
    fn mark_type_projection(&self, prefix: &Type, t: Type) -> Type {
        let (base, mut decls) = match &t {
            Type::Class { .. } => (t.clone(), Vec::new()),
            Type::Refined { parents, decls } if SymbolTable::as_seen_from_view(&t).is_some() => (
                parents[0].clone(),
                decls
                    .iter()
                    .filter(|d| {
                        !matches!(d, RefineDecl::Type { name, .. }
                            if name == crate::symbol::AS_SEEN_FROM_MARK)
                    })
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            _ => return t,
        };
        let Type::Class { sym: member, .. } = &base else {
            return t;
        };
        // A generic inner class is applied to its arguments after this
        // (`BasicBackend#BasicDatabaseDef[F]` in slick), and an application
        // does not see through a view; wrapping it made that "does not take
        // type parameters". Such a projection keeps its old, unchecked
        // reading.
        if !self.st.get(*member).tparams.is_empty() {
            return t;
        }
        let Some(pcls) = self.st.class_sym_of(prefix) else {
            return t;
        };
        let settled: Vec<String> = decls
            .iter()
            .filter_map(|d| match d {
                RefineDecl::Type { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        let unsettled = self
            .st
            .enclosing_classes(*member)
            .into_iter()
            .skip(1)
            .filter(|&o| o == pcls || self.st.is_ancestor_of(o, pcls))
            .flat_map(|o| self.st.abstract_type_member_names(o))
            .any(|n| !settled.contains(&n));
        if !unsettled {
            return t;
        }
        decls.push(RefineDecl::Type {
            name: crate::symbol::PROJECTION_MARK.to_string(),
            rhs: None,
            tparams: 0,
            lo: None,
            hi: None,
        });
        Self::as_seen_from(base, decls)
    }

    /// Wrap `base` in the as-seen-from view carrying `decls`, or hand it back
    /// unchanged when the prefix settles nothing.
    fn as_seen_from(base: Type, mut decls: Vec<RefineDecl>) -> Type {
        if decls.is_empty() {
            return base;
        }
        decls.insert(
            0,
            RefineDecl::Type {
                name: crate::symbol::AS_SEEN_FROM_MARK.to_string(),
                rhs: None,
                tparams: 0,
                lo: None,
                hi: None,
            },
        );
        Type::Refined {
            parents: vec![base],
            decls,
        }
    }

    /// `p.T[args]` where `T` stays abstract in `p`'s own class (SLS 7.2's
    /// "enclosing prefixes" of implicit scope). `ctor` is the un-applied
    /// type `tree_to_type(tpt)` produced for the constructor position. When
    /// `ctor` is a still-abstract `Type::TypeMember` reached through a
    /// qualified *module* prefix (`tpt` a `p.T`-shaped `Select` whose
    /// qualifier is not itself a term), record that module in
    /// [`Typer::type_member_prefixes`] against `T`'s own defining symbol, so
    /// the implicit search's `collect_type_parts` (in `implicits.rs`) can add
    /// it as an extra implicit-scope part wherever `T` turns up, dealiased or
    /// not. `applied` -- `ctor` combined with its arguments -- is returned
    /// unchanged; this only ever adds an entry to the side table.
    ///
    /// cats' `Newtype` encoding is exactly this shape: `object
    /// NonEmptySetImpl extends Newtype { type Type[A] <: Base with Tag;
    /// implicit def catsNonEmptySetOps[A](value: NonEmptySet[A]):
    /// NonEmptySetOps[A] = ... }` never overrides `Newtype`'s abstract
    /// `Type`, so `NonEmptySetImpl.Type[A]`'s only class-side answer is
    /// `Base`'s, and `Base`'s companion (there is none) is what implicit
    /// search used to see -- reporting `value toSortedSet is not a member
    /// of Newtype.Type[A]` for every method `NonEmptySetOps` adds. The
    /// conversion is declared on `NonEmptySetImpl` itself, reachable only
    /// through the prefix the source actually selected `Type` through.
    ///
    /// A side table, not a prefix carried on the `Type` itself (a
    /// `Type::Refined` "as-seen-from view", the way
    /// `Checker::projected_class_type` records a `Type::Class` prefix) --
    /// that view is exact-equality-visible everywhere a bare
    /// `Type::TypeMember` used to compare equal to itself (generic method
    /// type-argument inference in particular does not consult
    /// `SymbolTable::as_seen_from_view` the way `is_sub_type` and
    /// `display_type` do), and wrapping it that way regressed
    /// `WidgetImpl.unwrap(value)`: inferring `A` from a wrapped `value` no
    /// longer unified against `unwrap`'s bare `Type[A]` parameter, which is
    /// the very same symbol. A side table only implicit search reads cannot
    /// cause that, at the cost of being coarser than a real prefix: every
    /// object that ever selects `T` becomes a candidate source for every
    /// occurrence of `T`, not just the one it was written through. Harmless
    /// in practice -- an inapplicable candidate's signature simply fails to
    /// unify, the same way an unrelated implicit already in scope does.
    fn with_prefix_if_type_member(&mut self, tpt: &Tree, ctor: &Type, applied: Type) -> Type {
        let Type::TypeMember(id) = ctor else {
            return applied;
        };
        let TreeKind::Select { qual, .. } = &tpt.kind else {
            return applied;
        };
        if self.type_select_is_term_prefix(qual) {
            // A genuine value prefix (`self.Representation`) goes through
            // `path_dependent_type` / `project_from_prefix` instead, and is
            // not handled here -- see `docs/cats.md`'s "`Type::TypeMember`
            // has no prefix" note for that harder, still-open case.
            return applied;
        }
        let Some(owner) = self.qualified_type_owners(qual).into_iter().next() else {
            return applied;
        };
        if !matches!(
            self.st.get(owner).kind,
            SymKind::Module | SymKind::ModuleClass
        ) {
            return applied;
        }
        {
            let mut map = self.type_member_prefixes.borrow_mut();
            let owners = map.entry(id.0).or_default();
            if !owners.contains(&owner) {
                owners.push(owner);
            }
        }
        // `Jdbc.api.ColumnType[Int]` reduces to whatever the *profile that
        // owns `api`* makes of `ColumnType`, because `API`'s alias names its
        // enclosing profile through a self alias. The prefix is the only
        // thing that says which profile that is, so the reduction happens
        // here and not in the ordinary `this_class` walk: written inside
        // `object Jdbc`, `Mem.api.ColumnType[Int]` is still `MemType[Int]`.
        let from = self.as_type_owner(owner);
        self.st.expand_type_members(from, &applied)
    }

    /// The aliases `pcls` supplies for the abstract type members declared
    /// beside `member` (in its lexically enclosing classes and their
    /// ancestors). Empty when the prefix adds nothing.
    fn projection_refinements(
        &mut self,
        prefix: &Type,
        pcls: SymbolId,
        member: SymbolId,
    ) -> Vec<RefineDecl> {
        let mut decls: Vec<RefineDecl> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for owner in self.st.enclosing_classes(member).into_iter().skip(1) {
            // Only an enclosing class the prefix actually is can settle
            // anything; an unrelated lexical owner leaves `member` alone.
            if owner != pcls && !self.st.is_ancestor_of(owner, pcls) {
                continue;
            }
            for name in self.st.abstract_type_member_names(owner) {
                if !seen.insert(name.clone()) {
                    continue;
                }
                let Some(rhs) = self.concrete_type_member_of(prefix, pcls, &name) else {
                    continue;
                };
                decls.push(RefineDecl::Type {
                    name,
                    rhs: Some(rhs.0),
                    tparams: rhs.1,
                    lo: None,
                    hi: None,
                });
            }
        }
        decls
    }

    /// `pcls`'s answer for the type member `name`, read through `prefix`, or
    /// `None` when `pcls` leaves it abstract too. The `usize` is the kind
    /// arity (`type Database[F[_]] = HeapDatabaseDef[F]` → 1).
    fn concrete_type_member_of(
        &mut self,
        prefix: &Type,
        pcls: SymbolId,
        name: &str,
    ) -> Option<(Type, usize)> {
        for m in self.st.lookup_member(pcls, name) {
            if self.st.get(m).kind != SymKind::TypeMember {
                continue;
            }
            let info = self.st.get(m);
            let arity = info.tparams.len();
            if matches!(&info.ty, Type::NoType | Type::Error) {
                continue;
            }
            if let Type::TypeMember(inner) = &info.ty {
                // Still abstract (a member stands for itself): nothing to pin.
                if *inner == m {
                    continue;
                }
            }
            if arity > 0 {
                // A higher-kinded alias stays a constructor until applied;
                // `expand_applied_hk_alias` expands it at the use site.
                return Some((Type::TypeMember(m), arity));
            }
            let rhs = info.ty.clone();
            return Some((self.st.expand_in_type(prefix, &rhs), 0));
        }
        None
    }

    /// `P#T` where `P` is a type parameter or a type member the program has
    /// left deferred, so `P`'s *bound* is the only place `T` can be looked up.
    ///
    /// nsc resolves the member there and then reduces only what the bound
    /// really fixes: an alias (`type Backend <: JdbcBackend` where
    /// `JdbcBackend` writes `type Session = SessionDef`) dealiases, and a
    /// declaration the bound also leaves deferred does **not** -- `E#Elem` is
    /// not `AbstractRow`'s `Elem`, and reading it as one is what made
    /// `slick.lifted.TableQuery[E]`'s element type `Any` for every `E`. The
    /// unreduced case becomes an [abstract
    /// projection](SymbolTable::abstract_projection), which
    /// `SymbolTable::subst_projections` reduces at the argument.
    fn project_through_bound(
        &mut self,
        span: Span,
        prefix: SymbolId,
        bound: &Type,
        name: &str,
    ) -> Type {
        let through = self.project_from_prefix(span, bound, name);
        let Type::TypeMember(m) = through else {
            return through;
        };
        if !self.st.is_deferred_type_member(m) || self.st.abs_projection(m).is_some() {
            return through;
        }
        Type::TypeMember(self.st.abstract_projection(prefix, m))
    }

    fn project_from_prefix(&mut self, span: Span, prefix: &Type, name: &str) -> Type {
        self.project_from_prefix_at(span, prefix, name, prefix)
    }

    /// `project_from_prefix`, reading the member as seen from `at`: the
    /// prefix itself for `P#T`, the path's singleton for `p.T`. A member
    /// written in terms of the enclosing class's `this` -- `type Session =
    /// JdbcSessionDef`, an inner class -- means that class's instance *at
    /// the prefix* once projected (`prefix.rs`): `JdbcBackend#Session` is
    /// `JdbcBackend#JdbcSessionDef`, and `p.Session` is `p.JdbcSessionDef`.
    fn project_from_prefix_at(&mut self, span: Span, prefix: &Type, name: &str, at: &Type) -> Type {
        let t = self.project_from_prefix_in(span, prefix, name);
        if t.is_error() || !self.st.mentions_inner_class(&t) {
            return t;
        }
        self.warm_enclosing_parents(prefix);
        self.st.rewrite_view_this(prefix, Some(at), &t)
    }

    fn project_from_prefix_in(&mut self, span: Span, prefix: &Type, name: &str) -> Type {
        // A projection out of a prefix that already failed reports nothing new.
        if prefix.is_error() {
            return Type::Error;
        }
        // `o#arg[…]`: the prefix may be an alias whose right-hand side lives in
        // a unit that has not been walked yet. Resolve it before projecting.
        let prefix = &self.complete_prefix_aliases(span, prefix);
        // `A#B#C`: the view wrapped around `A#B` is not a structural type, so
        // project through its parent and keep carrying what `A` settled.
        if let Some(parent) = SymbolTable::as_seen_from_view(prefix) {
            if let Some(t) = self.st.lookup_type_member_on(prefix, name) {
                return t;
            }
            let parent = parent.clone();
            let Type::Refined { decls, .. } = prefix.clone() else {
                unreachable!()
            };
            let t = self.project_from_prefix(span, &parent, name);
            let carried: Vec<RefineDecl> = decls
                .into_iter()
                .filter(|d| {
                    !matches!(d, RefineDecl::Type { name, .. }
                        if name == crate::symbol::AS_SEEN_FROM_MARK)
                })
                .collect();
            return match t {
                Type::Class { .. } => Self::as_seen_from(t, carried),
                other => other,
            };
        }
        if let Type::Refined { parents, .. } = prefix {
            if let Some(t) = self.st.lookup_type_member_on(prefix, name) {
                return t;
            }
            // A refined jar class: the refinement declares one member and the
            // rest come from the parent, whose *type* members are still
            // unread. slick's `mapToImpl` is written exactly this way --
            // `c: blackbox.Context { type PrefixType = ShapedValue[_, U] }`,
            // and then `c.Expr[...]` / `c.Tree` for its own signature.
            if self.library_abi {
                for p in parents.clone() {
                    let Some(cls) = self.st.class_sym_of(&p) else {
                        continue;
                    };
                    if let Some(ty) =
                        self.pickle
                            .complete_type_member(&mut self.st, &mut self.binary, cls, name)
                    {
                        return self.st.expand_in_type(prefix, &ty);
                    }
                }
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
                    return self.project_through_bound(span, *id, &hi, name);
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
            // `E#T` for a type parameter `E`: the same question as for a
            // deferred type member, and the shape slick's `TableQuery[E <:
            // AbstractTable[_]] extends Query[E, E#TableElementType, Seq]`
            // is written in.
            Type::TypeParam(id) if !self.st.get(*id).tparams.is_empty() => {
                let n = self.st.get(*id).name.clone();
                self.error(span, format!("type {n} takes type parameters"));
                return Type::Error;
            }
            Type::TypeParam(id) => {
                let Some(hi) = self.st.get(*id).bound_hi.clone() else {
                    self.error(
                        span,
                        format!(
                            "type {name} is not a member of {}",
                            self.st.display_type(prefix)
                        ),
                    );
                    return Type::Error;
                };
                return self.project_through_bound(span, *id, &hi, name);
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
        if found.is_empty() {
            // A jar's nested class or companion is loaded on demand, not
            // eagerly: `cats.effect.Resource.ExitCase` reaches here as soon
            // as `type_select_is_term_prefix` reads `Resource` as the term it
            // is, and nothing has asked the classpath for `ExitCase` yet.
            // `lookup_qualified_type` (the package/Java-static sibling of
            // this path) already does this before giving up; a `p.T` through
            // a *value* prefix needs the same on-demand load.
            self.complete_binary_member(cls, name, span);
            found = self.st.lookup_member(cls, name);
        }
        found.sort_by_key(|s| if self.st.get(*s).owner == cls { 0 } else { 1 });
        // Every candidate is a *deferred* type member inherited from an
        // ancestor, and `cls` may well fix it: `slick.jdbc.JdbcBackend`
        // declares `type Session = SessionDef` over the `type Session` its
        // `slick.basic.BasicBackend` parent leaves abstract. Whether the
        // abstract declaration is already in the table depends only on which
        // file was compiled first, so ask the pickle -- which reads the
        // linearisation most-derived-first -- before answering with it.
        if self.library_abi
            && !found.is_empty()
            && found
                .iter()
                .all(|&m| self.st.get(m).owner != cls && self.st.is_deferred_type_member(m))
        {
            if let Some(ty) =
                self.pickle
                    .complete_type_member(&mut self.st, &mut self.binary, cls, name)
            {
                return self.st.expand_in_type(prefix, &ty);
            }
        }
        for m in found {
            let ty = match self.st.get(m).kind {
                SymKind::TypeMember => self.st.type_member_as_seen(m),
                SymKind::Class | SymKind::ModuleClass => self.projected_class_type(prefix, cls, m),
                _ => continue,
            };
            self.note_cto_ref(m, span);
            return self.st.expand_in_type(prefix, &ty);
        }
        // Nothing under that name yet. A class read from a jar has its members
        // completed one at a time, and its *type* members were never completed
        // at all: `c.Expr[T]` / `c.Tree` on a macro `Context` name aliases
        // declared far up `scala.reflect.macros.Aliases`, which no `def`
        // completion ever reaches. See `docs/macros.md` §7.6.
        if self.library_abi {
            if let Some(ty) =
                self.pickle
                    .complete_type_member(&mut self.st, &mut self.binary, cls, name)
            {
                return self.st.expand_in_type(prefix, &ty);
            }
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
        // The path's head may be a member whose type is still inferred from its
        // right-hand side. A template types its type aliases before its other
        // signatures, so cats' `new Representable[…] { val FG =
        // F0.compose(G0); type Representation = FG.Representation }` read
        // `FG` as `<notype>` (`type Representation is not a member of
        // <notype>`). nsc's lazy completer types `FG` right there.
        self.complete_path_head(prefix, span);
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
        // `p.In` for an inner class: the type prefix `projected_class_type`
        // recorded is the projection; the path names one instance.
        let spre = self.singleton_prefix_of(prefix);
        let t = match &spre {
            Some(sp) => self.project_from_prefix_at(span, &pty, name, sp),
            None => self.project_from_prefix(span, &pty, name),
        };
        let direct = |st: &SymbolTable, sym: SymbolId| {
            st.class_sym_of(&pty).is_some_and(|c| {
                st.lookup_member(c, name)
                    .into_iter()
                    .any(|m| m == sym && st.get(m).kind == SymKind::Class)
            })
        };
        let t = match (crate::prefix::strip_view(&t), spre) {
            (Type::Class { sym, .. }, Some(pre))
                if self.st.is_inner_class_of_class(*sym) && direct(&self.st, *sym) =>
            {
                crate::prefix::with_prefix(t, pre)
            }
            _ => t,
        };
        self.at_term_path(prefix, &pty, t)
    }

    /// Complete the signature of the term a type path starts from, when it is
    /// still pending (an unannotated `val` of the template being typed).
    fn complete_path_head(&mut self, prefix: &Tree, span: Span) {
        let mut head = prefix;
        while let TreeKind::Select { qual, .. } = &head.kind {
            head = qual;
        }
        let TreeKind::Ident { name } = &head.kind else {
            return;
        };
        let pending = self.st.lookup_term(name).into_iter().find(|s| {
            let sy = self.st.get(*s);
            matches!(sy.kind, SymKind::Term | SymKind::Method) && sy.ty.is_no_type()
        });
        if let Some(s) = pending {
            self.complete_lazy_sig(s, span);
        }
    }

    /// The chain of term symbols a stable path names (`a.b.c` -> `[a, b, c]`),
    /// or `None` when the path is not spelled out of terms -- `this`, `super`,
    /// a package, or an `object`, none of which can denote two instances and
    /// so none of which need their members told apart by prefix.
    ///
    /// The chain must *start* at a local: a parameter, a local `val`, or a
    /// pattern binding. A path that starts at a member of some class carries
    /// an implied outer prefix (`backend.Session` inside `BasicProfile` is
    /// `this.backend.Session`), and reading it through another receiver has to
    /// compose the two -- slick writes `profile.runSynchronousQuery(...)(s: profile.backend.Session)`
    /// and gets `backend.Session` for the parameter. Composing prefixes is a
    /// step beyond this slice; a local path never needs it, because there is
    /// no outer instance for it to be relative to.
    pub(crate) fn stable_term_path(&self, t: &Tree) -> Option<Vec<SymbolId>> {
        let path = self.stable_term_path_in(t)?;
        let head = *path.first()?;
        let owner = self.st.get(head).owner;
        if owner.is_none() {
            return None;
        }
        // A class's self alias (`trait Rep { self => … }`) is allowed: it is
        // another spelling of `this`, so it has no outer prefix either, and
        // it is the one that keeps `type Representation = (self.Representation,
        // G.Representation)` in an anonymous subclass from resolving its own
        // right-hand side back to itself by name (cats' `Representable#compose`).
        let is_self_alias = self.st.get(owner).self_alias == Some(head);
        (is_self_alias || !self.st.get(owner).is_class_like()).then_some(path)
    }

    fn stable_term_path_in(&self, t: &Tree) -> Option<Vec<SymbolId>> {
        match &t.kind {
            TreeKind::Ident { name } => {
                // A tree that has already been typed says which symbol it
                // resolved to; only a type tree, which is read before any of
                // that, has to go back to the scope.
                if !t.sym.is_none()
                    && matches!(self.st.get(t.sym).kind, SymKind::Term | SymKind::Method)
                {
                    return Some(vec![t.sym]);
                }
                let s = self.st.lookup_term(name).into_iter().find(|s| {
                    matches!(self.st.get(*s).kind, SymKind::Term | SymKind::Method)
                        && !self.st.get(*s).ty.is_no_type()
                })?;
                Some(vec![s])
            }
            TreeKind::Select { qual, name }
            | TreeKind::SelectFromTypeTree {
                qual,
                name,
                hash: false,
            } => {
                let mut chain = self.stable_term_path_in(qual)?;
                let qty = self.term_path_type(qual)?;
                let cls = self.st.class_sym_of(&qty)?;
                let s =
                    self.st.lookup_member(cls, name).into_iter().find(|s| {
                        matches!(self.st.get(*s).kind, SymKind::Term | SymKind::Method)
                    })?;
                chain.push(s);
                Some(chain)
            }
            _ => None,
        }
    }

    /// `T` seen through the term path `path_tree`, whose type is `pty`.
    ///
    /// Only a *deferred* member needs this: an alias carries its right-hand
    /// side, which `project_from_prefix` has already read through the prefix.
    pub(crate) fn at_term_path(&mut self, path_tree: &Tree, pty: &Type, t: Type) -> Type {
        let Type::TypeMember(m) = t else {
            return t;
        };
        // `p.T` on a `p: P` whose type is abstract: `project_from_prefix`
        // answered with the abstract projection `P#T`, because the *type* `P`
        // settles nothing. The *term* `p` does -- it is one instance, and
        // `path_member` is the representation for that (`agent/projection`).
        // Slick's `def get[P <: Phase](p: P): Option[p.State]` is exactly
        // this, and leaving the projection in place lost fourteen calls.
        let m = match self.st.abs_projection(m) {
            Some((_, decl)) => decl,
            None => m,
        };
        if !self.can_be_path_member(m, pty) {
            return t;
        }
        let Some(path) = self.stable_term_path(path_tree) else {
            return t;
        };
        Type::TypeMember(self.st.path_member(&path, m, pty))
    }

    /// Load the concrete definitions of type members used by an API member.
    /// A nested API's method can name its outer profile's abstract family;
    /// the classfile has no entry for the alias that a derived profile fixes.
    pub(crate) fn warm_receiver_type_members(&mut self, recv: &Type, ty: &Type) {
        let Some(cls) = self.st.class_sym_of(recv) else {
            return;
        };
        let members = self.st.type_members_in(ty);
        if members.is_empty() {
            return;
        }
        for outer in self.st.enclosing_classes(cls) {
            for &member in &members {
                let owner = self.st.get(member).owner;
                if owner.is_none() || !(outer == owner || self.st.is_ancestor_of(owner, outer)) {
                    continue;
                }
                let name = self.st.get(member).name.clone();
                self.pickle
                    .complete_type_member(&mut self.st, &mut self.binary, outer, &name);
            }
        }
    }

    /// The rewrite that puts a selected member's type behind the path its
    /// receiver was written as: `c.start` on `val start: () => Eval[Start]` is
    /// `() => Eval[c.Start]`, not `() => Eval[Start]`.
    ///
    /// Returned as a (declaration, path member) list rather than applied,
    /// because it has to run on the *declaration* -- before the receiver's
    /// type arguments are substituted in. Afterwards an occurrence that came
    /// from a type argument (`case cc: FlatMap[c.Start]` makes `cc.run`'s
    /// result `Eval[c.Start]`) is indistinguishable from one the declaration
    /// wrote, and re-pathing both to `cc` is wrong.
    ///
    /// Only members the receiver's class declares are rewritten -- a type
    /// member of some other class that the signature mentions is not the
    /// receiver's to reinterpret.
    pub(crate) fn receiver_path_members(
        &mut self,
        qual: &Tree,
        recv_ty: &Type,
        found: &[SymbolId],
    ) -> Vec<(SymbolId, SymbolId)> {
        for &id in found {
            let ty = self.st.get(id).ty.clone();
            self.warm_receiver_type_members(recv_ty, &ty);
        }
        let Some(cls) = self.st.class_sym_of(recv_ty) else {
            return Vec::new();
        };
        let mut candidates: Vec<SymbolId> = Vec::new();
        for s in found {
            let ty = self.st.get(*s).ty.clone();
            for m in self.st.type_members_in(&ty) {
                if candidates.contains(&m) || !self.can_be_path_member(m, recv_ty) {
                    continue;
                }
                let name = self.st.get(m).name.clone();
                if self.st.lookup_member(cls, &name).contains(&m) {
                    candidates.push(m);
                }
            }
        }
        if candidates.is_empty() {
            return Vec::new();
        }
        let Some(path) = self.stable_term_path(qual) else {
            return Vec::new();
        };
        candidates
            .into_iter()
            .map(|m| (m, self.st.path_member(&path, m, recv_ty)))
            .collect()
    }

    /// nsc's dependent method types, for the paths this compiler tracks.
    ///
    /// `def nanos[C](c: C)(implicit ev: Classifier[C]): ev.R` names one of its
    /// own parameters in its result, and at a call site that means the
    /// *argument's* path. Without the substitution the callee's `ev.R` and the
    /// caller's `ev.R` are two different paths, and every one of
    /// `DurationConversions`' fourteen forwarders fails against its own
    /// signature with `found: ev.R  required: ev.R`.
    pub(crate) fn subst_dependent_paths(
        &mut self,
        params: &[SymbolId],
        args: &[Tree],
        ret: Type,
    ) -> Type {
        // The arguments have to line up one for one with the parameters, or
        // the index a path names is not the index this call filled.
        if params.len() != args.len() {
            return ret;
        }
        // A result may name the parameter itself (b.type), not just b.Member.
        // Substitute at the call boundary so an instantiated Builder's result
        // does not get read through the formal Builder[A, F] again. Stable
        // actuals retain their singleton identity; fresh expressions widen to
        // their already inferred type without being evaluated a second time.
        let ret = if crate::symbol::any_type(
            &ret,
            &mut |t| matches!(t, Type::SingleType { sym, .. } if params.contains(sym)),
        ) {
            let actuals: Vec<Type> = args
                .iter()
                .map(|arg| {
                    if matches!(
                        arg.ty,
                        Type::SingleType { .. } | Type::ThisType(_) | Type::ModuleRef(_)
                    ) {
                        return arg.ty.clone();
                    }
                    if let TreeKind::This { .. } = &arg.kind {
                        if let Some(c) = self.st.class_sym_of(&arg.ty) {
                            return Type::ThisType(c);
                        }
                    }
                    if self.is_stable_path(arg) {
                        if let Some(sym) = self.term_path_sym(arg) {
                            let prefix = match &arg.kind {
                                TreeKind::Select { qual, .. } => qual.ty.clone(),
                                _ => Type::NoType,
                            };
                            return Type::SingleType {
                                prefix: Box::new(prefix),
                                sym,
                            };
                        }
                    }
                    arg.ty.clone()
                })
                .collect();
            crate::symbol::map_type(&ret, &mut |t| {
                if let Type::SingleType { sym, .. } = t {
                    if let Some(i) = params.iter().position(|p| p == sym) {
                        return actuals[i].clone();
                    }
                }
                t.clone()
            })
        } else {
            ret
        };
        if !self.st.mentions_path_member_deep(&ret) {
            return ret;
        }
        let skolems: Vec<SymbolId> = self.st.path_members_in(&ret);
        let mut out = ret;
        for sk in skolems {
            let Some(path) = self.st.path_member_path(sk).map(|p| p.to_vec()) else {
                continue;
            };
            let Some(i) = params.iter().position(|p| *p == path[0]) else {
                continue;
            };
            let decl = self.st.path_member_decl(sk).unwrap_or(sk);
            let prefix = args[i].ty.clone();
            // The argument's own class may *fix* the member -- slick's
            // `state.get(Phase.assignUniqueSymbols)` is `Option[p.State]` on a
            // `p` whose actual class defines `State`. Then the answer is that
            // type, not a path at all.
            if !self.can_be_path_member(decl, &prefix) {
                if self.st.class_sym_of(&prefix).is_some() {
                    let seen = self.st.expand_in_type(&prefix, &Type::TypeMember(decl));
                    if !matches!(&seen, Type::TypeMember(x) if *x == decl) {
                        out = self.st.subst_path_member_deep(&out, sk, &seen);
                    }
                }
                continue;
            }
            let Some(mut actual) = self.stable_term_path(&args[i]) else {
                continue;
            };
            actual.extend_from_slice(&path[1..]);
            if actual == path {
                continue;
            }
            let re = self.st.path_member(&actual, decl, &prefix);
            out = self
                .st
                .subst_path_member_deep(&out, sk, &Type::TypeMember(re));
        }
        out
    }

    /// Is `m` a declaration a path can be attached to?
    ///
    /// A *deferred* member only: an alias carries its right-hand side, which
    /// has already been read through the prefix. And a first-order one only: a
    /// higher-kinded member is a type constructor whose application drives
    /// inference and reduction, and carrying a prefix through that is a step
    /// this slice does not take.
    fn can_be_path_member(&self, m: SymbolId, prefix: &Type) -> bool {
        if !self.st.is_deferred_type_member(m) || self.st.path_member_decl(m).is_some() {
            return false;
        }
        // ...and only when the *prefix's own class* still leaves it deferred.
        // `trait Node { type Self >: this.type <: Node }` is abstract at its
        // declaration, but `this2: ParameterSwitch` fixes it, so `this2.Self`
        // is `ParameterSwitch` -- not an abstract member behind a prefix.
        // Freezing it as one made six of slick's `:@` calls fail against
        // their own declared result.
        if self.st.class_sym_of(prefix).is_none() {
            return false;
        }
        matches!(
            self.st.expand_in_type(prefix, &Type::TypeMember(m)),
            Type::TypeMember(x) if x == m
        )
    }

    pub(crate) fn is_stable_path(&self, t: &Tree) -> bool {
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

    pub(crate) fn singleton_to_type(&mut self, span: Span, ref_: &Tree) -> Type {
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
            TreeKind::Ident { name } => self
                .st
                .lookup_term(name)
                .into_iter()
                .find(|s| self.names_a_singleton(*s)),
            TreeKind::Select { qual, name } | TreeKind::SelectFromTypeTree { qual, name, .. } => {
                let Some(qt) = self.term_path_type(qual) else {
                    // A package is not a value, so it has no type -- but it is
                    // still a legal path prefix: `p.q.HNil.type`.
                    let owner = self.path_owner_sym(qual)?;
                    return self
                        .st
                        .lookup_member(owner, name)
                        .into_iter()
                        .find(|s| self.names_a_singleton(*s));
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
                self.st
                    .lookup_member(cls, name)
                    .into_iter()
                    .find(|s| self.names_a_singleton(*s))
            }
            _ => None,
        }
    }

    /// Whether `s` is a term a singleton type can be written over.
    ///
    /// The same three kinds `ident_is_stable` / `member_is_stable` accept,
    /// **plus a `val` accessor read from a pickle**: a class file cannot tell
    /// a `val`'s accessor from an ordinary `def`, so such a member is a
    /// `SymKind::Method` carrying `Flags::ACCESSOR`. Leaving it out is what
    /// made `Mirror[c.universe.type]` -- `c.universe` is `val universe: Universe`
    /// on `blackbox.Context` -- report `stable identifier required, but
    /// c.universe found` while `c.universe.Tree`, which goes through
    /// `path_dependent_type` and only asks `member_is_stable`, compiled fine.
    /// `docs/macros.md` §7.8 residual 6.
    fn names_a_singleton(&self, s: SymbolId) -> bool {
        let sy = self.st.get(s);
        match sy.kind {
            SymKind::Term | SymKind::Module | SymKind::ModuleClass => true,
            SymKind::Method => sy.flags.contains(Flags::ACCESSOR),
            _ => false,
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
                // A JVM classfile cannot distinguish a `val`'s accessor from
                // an ordinary `def` -- both are a bare zero-arg method. A val
                // read from a pickle (`complete_named`, `pickle_supply.rs`)
                // is marked `Flags::ACCESSOR` for exactly this check; a
                // *real* `def` never carries it.
                SymKind::Method => sy.flags.contains(Flags::ACCESSOR),
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
                        SymKind::Method => sy.flags.contains(Flags::ACCESSOR),
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
                // A JVM classfile cannot distinguish a `val`'s accessor from
                // an ordinary `def` -- both are a bare zero-arg method. A val
                // read from a pickle (`complete_named`, `pickle_supply.rs`)
                // is marked `Flags::ACCESSOR` for exactly this check; a
                // *real* `def` never carries it.
                SymKind::Method => sy.flags.contains(Flags::ACCESSOR),
                _ => false,
            }
        })
    }

    fn term_path_type(&self, t: &Tree) -> Option<Type> {
        match &t.kind {
            // `Outer.this` names the *enclosing* class, not the innermost
            // one. Reading it as `this` made `trait Outer { type T; trait
            // Inner { type T <: Outer.this.T } }` bound `Inner`'s own `T` by
            // itself -- an invented cycle that made `class_sym_of` recurse
            // until the stack ran out (`pos/t690`), and that
            // `cyclic::bound_cycles` would now reject outright. The qualifier
            // is resolved the same way `singleton_to_type` resolves it for
            // `Outer.this.type`.
            TreeKind::This { qual } => {
                let id = match qual {
                    Some(name) => self
                        .st
                        .enclosing_class_named(self.st.this_class, name)
                        .unwrap_or(self.st.this_class),
                    None => self.st.this_class,
                };
                if id.is_none() {
                    None
                } else {
                    Some(self.st.self_type_of_class(id))
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
                        // A package object's `val Resource = cats.effect.
                        // kernel.Resource` compiles to a nullary method (see
                        // `Flags::ACCESSOR` on `pflags::STABLE` above); its
                        // stored type is `Type::Method { paramss: [], ret:
                        // ModuleRef(..) }`, not the `ModuleRef` itself.
                        // Ordinary expression typing widens that through
                        // `maybe_auto_apply` on every other path; a bare
                        // `sy.ty.clone()` here skipped it, so
                        // `class_sym_of` saw a `Method` it does not handle
                        // and `Resource.ExitCase` failed with "type ExitCase
                        // is not a member of Resource$" even once `Resource`
                        // itself resolved as a stable path.
                        SymKind::Term | SymKind::Method => {
                            Some(self.maybe_auto_apply(sy.ty.clone(), &Type::NoType))
                        }
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
                                SymKind::Term | SymKind::Method => {
                                    Some(self.maybe_auto_apply(sy.ty.clone(), &Type::NoType))
                                }
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
                // Read through the path: `prof.api` for `val api: Aliases`
                // declared in `Api` is a `prof.Aliases`, whose own prefix is
                // what `prof.api.Inner` then resolves against (`prefix.rs`).
                let qpre = self.singleton_prefix_of(qual);
                self.st.lookup_member(cls, name).into_iter().find_map(|s| {
                    let sy = self.st.get(s);
                    match sy.kind {
                        SymKind::Term | SymKind::Method => {
                            let t = self.st.expand_in_type(&qt, &sy.ty);
                            let t = if self.st.mentions_inner_class(&t) {
                                self.st.subst_as_seen_from_at(&qt, qpre.as_ref(), &t)
                            } else {
                                t
                            };
                            Some(self.maybe_auto_apply(t, &Type::NoType))
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
                self.st.get_mut(id).is_type_alias = true;
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
            self.st.get_mut(id).is_type_alias = true;
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
        // `[-a]List[a]` puts its own parameter in the wrong position. nsc
        // validates the refinement member wherever the refinement is written
        // (`neg/t7872b`), naming it `type l` inside a `type` alias and `value
        // <local l>` in a term's type (`kind_bounds.rs`).
        {
            let own = self.st.get(id).tparams.clone();
            let desc = if self.alias_rhs_depth > 0 {
                format!("type {name}")
            } else {
                format!("value <local {name}>")
            };
            let body = (!rhs.is_empty()).then_some(&rhs_ty);
            self.check_hk_member_own_variance(
                &own,
                body,
                lo_ty.as_ref(),
                hi_ty.as_ref(),
                r.span,
                &desc,
            );
        }
        // A type lambda may mention type parameters of whatever encloses it:
        // `implicit def readerMonad[R]: Monad[({ type L[X] = Reader[R, X] })#L]`
        // captures `R`. A `Type::TypeMember` is only a symbol, so a later
        // substitution of `R` cannot reach inside the stored body -- the
        // instance for `R = Int` would still read `Reader[R, X]`. Add every
        // captured parameter as a *leading* parameter of the member and hand
        // out the member already applied to them, so the projection is a
        // partial application. Substitution then works on the arguments, which
        // are ordinary types, and the arity the world sees is unchanged
        // (`kind_arity` of a partial application subtracts what is applied).
        let mut captured = Vec::new();
        if !rhs.is_empty() {
            let own = self.st.get(id).tparams.clone();
            let mut free = Vec::new();
            collect_tparams(&rhs_ty, &mut free);
            captured = free.into_iter().filter(|t| !own.contains(t)).collect();
            if !captured.is_empty() {
                let all = captured.iter().copied().chain(own).collect();
                self.st.get_mut(id).tparams = all;
            }
        }
        self.st.get_mut(id).ty = rhs_ty;
        self.st.pop_scope();
        let member = if captured.is_empty() {
            Type::TypeMember(id)
        } else {
            Type::Applied {
                ctor: Box::new(Type::TypeMember(id)),
                args: captured.into_iter().map(Type::TypeParam).collect(),
            }
        };
        Some(scala_rs_parser::RefineDecl::Type {
            name: name.clone(),
            rhs: Some(member),
            tparams: tparams.len(),
            lo: lo_ty,
            hi: hi_ty,
        })
    }

    /// `p.T` where `T` is a type alias declared by a class read from a jar.
    /// Only the `ScalaSignature` pickle records it, so this is the answer when
    /// [`Self::lookup_qualified_type`] -- a symbol-table lookup -- has none.
    fn qualified_pickled_type_member(&mut self, qual: &Tree, name: &str) -> Option<Type> {
        if !self.library_abi {
            return None;
        }
        for owner in self.qualified_type_owners(qual) {
            if let Some(t) =
                self.pickle
                    .complete_type_member(&mut self.st, &mut self.binary, owner, name)
            {
                return Some(t);
            }
        }
        None
    }

    fn lookup_qualified_type(&mut self, prefix: &Tree, name: &str) -> Option<SymbolId> {
        // A class beats an object of the same name *wherever* it was found,
        // not only within one owner. `object Ref { trait Make[F[_]] }` read
        // from a class file splits in two: the pickle installs `Make`'s module
        // accessor on `Ref$`, while the trait -- the only one of the pair that
        // has type parameters -- was stubbed under the *trait* `Ref`, because
        // `Ref$Make` alone does not say which of the two `Ref`s owns it.
        // Stopping at the first owner therefore answered `Ref.Make[F]` with
        // the object and reported "Make does not take type parameters".
        let mut fallback: Option<SymbolId> = None;
        for owner in self.qualified_type_owners(prefix) {
            self.complete_binary_member(owner, name, prefix.span);
            let Some(id) = self.prefer_class_member(owner, name) else {
                continue;
            };
            if self.st.get(id).kind == SymKind::Class {
                return Some(id);
            }
            if fallback.is_none() {
                fallback = Some(id);
            }
        }
        fallback
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
    pub(crate) fn as_type_owner(&self, id: SymbolId) -> SymbolId {
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
    ///
    /// A class beats a `type` member beats a module: `new Outer.Inner()`
    /// wants the class over a companion object of the same name, but a `type`
    /// alias must still beat that same module when *it* is what shares the
    /// name -- cats' `Newtype` encoding declares `object Widget` directly in
    /// a package and, elsewhere, `type Widget[A] = Widget.Type[A]` on the
    /// package object folded into the same package (`members_including_
    /// inherited`), so `lookup_member` hands back both under one name, direct
    /// members first. Without this tier, `p.Widget[Int]` picked the *module*
    /// -- kind arity 0 -- whenever `lookup_member` happened to return it
    /// before the alias, which it always does here (the module is a direct
    /// member; the alias reaches the package only through the deferred
    /// fold).
    pub(crate) fn type_owner_members(&self, owner: SymbolId, name: &str) -> Vec<SymbolId> {
        let found = self.st.lookup_member(owner, name);
        let mut out: Vec<SymbolId> = found
            .iter()
            .copied()
            .filter(|&s| self.st.get(s).kind == SymKind::Class)
            .collect();
        for s in &found {
            if matches!(
                self.st.get(*s).kind,
                SymKind::TypeMember | SymKind::TypeParam
            ) && !out.contains(s)
            {
                out.push(*s);
            }
        }
        for s in found {
            let ok = matches!(
                self.st.get(s).kind,
                SymKind::Package | SymKind::Module | SymKind::ModuleClass
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
    fn qualifier_is_package(&mut self, qual: &Tree) -> bool {
        let owners = self.qualified_type_owners(qual);
        !owners.is_empty()
            && owners
                .iter()
                .all(|&id| self.st.get(id).kind == SymKind::Package)
    }

    fn qualified_type_owners(&mut self, t: &Tree) -> Vec<SymbolId> {
        let mut out: Vec<SymbolId> = Vec::new();
        match &t.kind {
            // `_root_` names the root package here too. Without this,
            // `_root_.p.q.C[A]` -- what Twirl writes at the head of every
            // generated template -- found no owner for the prefix and was
            // reported as `not found: type C`, while the same path without the
            // `_root_` resolved.
            TreeKind::Ident { name } if name == "_root_" && self.st.lookup(name).is_empty() => {
                let o = self.as_type_owner(self.st.root);
                out.push(o);
            }
            TreeKind::Ident { name } => {
                self.expose_unqualified(name, t.span);
                let is_owner_kind = |st: &SymbolTable, id: SymbolId| {
                    matches!(
                        st.get(id).kind,
                        SymKind::Package | SymKind::Class | SymKind::Module | SymKind::ModuleClass
                    )
                };
                let mut found: Vec<SymbolId> = self
                    .st
                    .lookup(name)
                    .into_iter()
                    .filter(|&id| is_owner_kind(&self.st, id))
                    .collect();
                if found.is_empty() {
                    // `expose_unqualified` bails out as soon as *any* symbol --
                    // of any namespace -- already answers `name` locally. That
                    // is right for its usual callers, but a package-level
                    // definition can forward-reference its own name before its
                    // own type is known (the namer enters it early so
                    // recursive definitions resolve), and that self-entry then
                    // shadows a *different* symbol of the same name declared
                    // elsewhere in a different namespace. cats' `type
                    // NonEmptyLazyList[+A] = NonEmptyLazyList.Type[A]` needs
                    // the *object* `NonEmptyLazyList` while typing the alias
                    // of the same name, and the alias's own forward-entered
                    // stub was all `lookup` could see. Fall back to a member
                    // search of the packages this file opened, restricted to
                    // the kinds an owner can be.
                    let name = name.clone();
                    let span = t.span;
                    let from = if !self.st.this_class.is_none() {
                        self.st.this_class
                    } else {
                        self.st.owner
                    };
                    for pkg in self.open_packages(from) {
                        self.complete_binary_member(pkg, &name, span);
                        for id in self.st.lookup_member(pkg, &name) {
                            if is_owner_kind(&self.st, id) && !found.contains(&id) {
                                found.push(id);
                            }
                        }
                        if !found.is_empty() {
                            break;
                        }
                    }
                }
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

    pub(crate) fn complete_binary_member(&mut self, owner: SymbolId, name: &str, span: Span) {
        if owner.is_none() || name.is_empty() {
            return;
        }
        let owner = self.as_type_owner(owner);
        if self.st.get(owner).kind == SymKind::Class {
            self.ensure_java_loaded(owner, span);
            let found = self.st.lookup_member(owner, name);
            if !found.is_empty() {
                // A nested Java class usually reaches the table as a *stub*
                // long before anyone writes its name: `Map.entrySet()`'s
                // generic signature mentions `java/util/Map$Entry`, so reading
                // `Map` alone enters an `Entry` with no parents and no type
                // parameters. Returning here on the strength of that stub
                // meant `java/util/Map$Entry.class` was never read, and
                // `java.util.Map.Entry[String, Int]` drew "Entry does not take
                // type parameters". The nested class file is what carries the
                // nested `Signature` (`<K:…;V:…>`), so complete it too.
                for id in found {
                    if self.st.get(id).kind == SymKind::Class {
                        self.ensure_java_loaded(id, span);
                    }
                }
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
        // Try every candidate, not just the first hit: a case class with a
        // companion (`Const` / `Const$`, or cats-effect's `Errored` /
        // `Errored$`) is two class files under the same simple name, and
        // stopping at the class alone left the module -- the one term
        // position actually wants, and the only one with `apply`/`unapply`
        // -- never installed. `Const(5)` then read as "value apply is not a
        // member of Const" instead of finding the companion's constructor
        // sugar.
        let mut any = false;
        for internal in self.binary_member_candidates(owner, name) {
            if self.load_binary_into(&internal, owner, span, true) {
                any = true;
            }
        }
        if any {
            return;
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
            self.complete_package_object_member(owner, name, span);
        }
    }

    /// A package object's members reach the symbol table through its
    /// *classfile*, and a JVM descriptor cannot say that a parameter clause is
    /// implicit: `scala.reflect.classTag[T](implicit ct: ClassTag[T])` arrived
    /// as an ordinary one-parameter method, so `classTag[Short]` kept a method
    /// type and conformed to nothing. The pickle is the only place the real
    /// signature is written down.
    fn complete_package_object_member(&mut self, pkg: SymbolId, name: &str, span: Span) {
        if !self.library_abi || !self.st.get(pkg).jvm_name.starts_with("scala/") {
            return;
        }
        let Some(po) = self.package_object_of(pkg, span) else {
            return;
        };
        let added = self
            .pickle
            .complete(&mut self.st, &mut self.binary, po, name);
        if added.is_empty() {
            return;
        }
        // The descriptor-derived symbol for the same JVM method is the same
        // member seen through a poorer lens; keeping both would make every
        // call an overload set. Matched on the erased descriptor, so a real
        // overload the pickle did not supply is left in place.
        let replaced: Vec<String> = added
            .iter()
            .map(|&m| self.st.get(m).jvm_name.clone())
            .filter(|d| !d.is_empty())
            .collect();
        let stale: Vec<SymbolId> = self
            .st
            .get(po)
            .members
            .iter()
            .copied()
            .filter(|&m| {
                !added.contains(&m)
                    && self.st.get(m).name == name
                    && replaced.contains(&self.st.get(m).jvm_name)
            })
            .collect();
        for owner in [po, pkg] {
            self.st
                .get_mut(owner)
                .members
                .retain(|m| !stale.contains(m));
        }
        for m in added {
            if !self.st.get(pkg).members.contains(&m) {
                self.st.get_mut(pkg).members.push(m);
            }
        }
    }

    fn binary_member_candidates(&self, owner: SymbolId, name: &str) -> Vec<String> {
        let name = scala_rs_pickle::names::encode_method_name(name);
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

    /// Give the prelude's `TupleN` classes the `Product` / `Serializable`
    /// parents nsc gives them.
    ///
    /// Both interfaces live in the library jar, which is read on demand, so
    /// they are forced here -- once, before any unit is named, so that every
    /// later `is_sub_type` sees the same hierarchy. Without the jar
    /// (`--no-scala-library`) neither class is found and nothing is linked:
    /// the private runtime's `scala/Tuple2` implements neither, and a parent
    /// the backend cannot back up would be a lie.
    pub(crate) fn link_tuple_products(&mut self) {
        if !self.library_abi {
            return;
        }
        // `ProductN` too: `TupleN extends ProductN[T1, …]`, so an
        // `Option[Product2[Int, String]]` extractor may return `Some((1, "a"))`
        // (`run/unapply`, `pos/unapplySeq`).
        let products: Vec<String> = (1..=crate::check::MAX_TUPLE_ARITY)
            .map(|n| format!("scala/Product{n}"))
            .collect();
        for jvm in ["scala/Product", "java/io/Serializable"]
            .into_iter()
            .chain(products.iter().map(String::as_str))
        {
            let pkg = jvm.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            let owner = crate::classpath::ensure_package(&mut self.st, pkg);
            self.load_binary_into(jvm, owner, Span::new(0, 0), false);
        }
        crate::prelude_genrep::link_tuple_products(&mut self.st);
    }

    /// `object Vector extends IterableFactory[Vector]` — the edge that lets
    /// `IterableFactory.toFactory` see a collection companion as a `Factory`
    /// source, so `xs.to(Vector)` types. `IterableFactory` lives in the jar,
    /// and `install_prelude` runs before the classpath is installed, so this
    /// has to be a separate pass.
    pub(crate) fn link_collection_factories(&mut self) {
        if !self.library_abi {
            return;
        }
        for jvm in crate::prelude_buildfrom::FACTORY_CLASSES {
            let pkg = jvm.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            let owner = crate::classpath::ensure_package(&mut self.st, pkg);
            self.load_binary_into(jvm, owner, Span::new(0, 0), false);
        }
        for jvm in [
            "scala/collection/EvidenceIterableFactory$",
            "scala/collection/SortedMapFactory$",
        ] {
            if let Some(cls) = crate::classpath::find_by_jvm(&self.st, jvm) {
                self.pickle
                    .adopt_binary_class(&mut self.st, &mut self.binary, cls);
                self.pickle
                    .complete_on_class(&mut self.st, &mut self.binary, cls, "toFactory");
            }
        }
        crate::prelude_buildfrom::install(&mut self.st, true);
    }

    /// `java.lang.String` implements `CharSequence`, `Comparable<String>` and
    /// `Serializable`; the prelude declares it with `AnyRef` alone. Unlike the
    /// tuples above these are JDK classes, so this runs in both library modes.
    pub(crate) fn link_string_parents(&mut self) {
        for jvm in crate::prelude_strhier::STRING_PARENTS {
            let pkg = jvm.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            let owner = crate::classpath::ensure_package(&mut self.st, pkg);
            self.load_binary_into(jvm, owner, Span::new(0, 0), false);
        }
        crate::prelude_strhier::link_string_parents(&mut self.st);
    }

    /// Load `<internal>$`, a class's companion object, if it has one on the
    /// classpath and is not already there.
    ///
    /// Only for Scala class files: a *Java* class's `Foo$` is a nested class,
    /// not a companion, and installing it would enter a class called `Foo$` in
    /// the package.
    ///
    /// `scala.*` is *not* excluded, though the prelude is what describes the
    /// standard library. The prelude describes what programs *name*, and
    /// nothing ever names an implicit -- it is found by searching a scope. So
    /// a library companion the prelude never declared held no witnesses at
    /// all. `scala.collection.BuildFrom` is one, and `buildFromIterableOps` is
    /// the only witness `LazyZip2.map` can use: unless the program happened to
    /// *import* `BuildFrom` by name, it was in no scope, which is what made a
    /// fully applied `implicitly[BuildFrom[…]]` work while
    /// `xs.lazyZip(ys).map(f)` did not.
    ///
    /// Nothing a hand-written declaration owns is replaced: a class that
    /// already has a companion returns at the check above, a companion already
    /// entered under that JVM name is left alone, and for `scala.*` only the
    /// implicits are installed -- everything else keeps coming from the pickle
    /// on demand, exactly as it did when the companion was an empty stub.
    pub(crate) fn load_companion_module(&mut self, class_id: SymbolId) {
        if class_id.is_none() || self.st.get(class_id).kind != SymKind::Class {
            return;
        }
        if self.st.companion_module(class_id).is_some() {
            return;
        }
        // Not gated on the `JAVA` flag: `find_or_stub_java_class` sets it on
        // every placeholder it enters, including Scala classes reached through
        // a parent list, and nothing clears it afterwards. The classfile's own
        // `is_scala` below is the honest test.
        let internal = self.st.get(class_id).jvm_name.clone();
        if internal.is_empty()
            || internal.ends_with('$')
            || internal.starts_with('[')
            || internal.starts_with("java/")
            || internal.starts_with("javax/")
        {
            return;
        }
        // A *nested* `scala.*` object is not this pass's business. Its class
        // file has no `ScalaSignature` of its own (a trait's nested object is
        // pickled inside the enclosing trait), and `materialize::ensure_tag_module`
        // hand-builds `TypeTags#TypeTag$` -- taking the presence of a symbol
        // under that JVM name as the record that it already did. Entering the
        // bare class file first made it stand down, and `typeOf[T]`'s
        // `TypeTag.apply(mirror, creator)` had no `apply` to resolve to
        // (slick's `ShapedValue` / `TableQuery` macros).
        let simple = internal.rsplit('/').next().unwrap_or("");
        if internal.starts_with("scala/") && simple.contains('$') {
            return;
        }
        let module = format!("{internal}$");
        // Already there under that JVM name; never enter a second copy.
        if crate::classpath::find_by_jvm(&self.st, &module).is_some() {
            return;
        }
        if !self.completed_java.insert(module.clone()) {
            return;
        }
        let Ok(Some(bytes)) = self.binary.find_class(&module) else {
            return;
        };
        let Ok(jc) = crate::javaclass::parse_java_classfile(&bytes) else {
            return;
        };
        if !jc.is_scala {
            return;
        }
        // A *nested* class's companion belongs to whatever encloses the class,
        // not to the package. `companion_module` looks for a module of the
        // same name among the class's own owner's members, and
        // `cats/effect/kernel/Ref$Make$` installed in the package
        // `cats.effect.kernel` under the name `Make` was invisible from the
        // trait `Ref.Make`, whose owner is `Ref`. `Ref.of`'s
        // `implicit mk: Make[F]` then searched an empty implicit scope --
        // unless the program happened to *write* `Ref.Make` somewhere, which
        // builds the companion by another route and made the failure look
        // order-dependent (slick's `ConcurrencyControl.scala`).
        let owner = {
            let o = self.st.get(class_id).owner;
            if !o.is_none() && self.st.get(o).is_class_like() {
                o
            } else {
                let pkg = internal.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                crate::classpath::ensure_package(&mut self.st, pkg)
            }
        };
        let mid = crate::classpath::install_java_class_in(&mut self.st, &jc, owner);
        // SLS 7.2 names the companion *object*, whose members include the ones
        // it inherits, and 2.13 puts the low-priority half of an implicit set
        // in traits the object mixes in:
        // `object BuildFrom extends BuildFromLowPriority1 extends
        // BuildFromLowPriority2`, and `buildFromIterableOps` -- the only
        // witness a plain `List` receiver has -- is declared in the *last* of
        // those. Loading only the object left it invisible.
        self.complete_java_parents(mid, Span::new(0, 0));
        // Deliberately *not* `adopt_binary_class`: only the implicits are
        // wanted here, and adopting the whole companion costs minutes.
        // For a standard-library companion the *pickle* is the authority, and
        // the ordinary on-demand path reads it a name at a time. The members
        // the classfile reader just entered carry erased signatures and would
        // sit next to the pickled ones as bogus overloads -- `Option$` gained
        // a second `apply` and `Option(2)` became `ambiguous overload`. Before
        // this pass existed, `scala.*` companions reached the typer as an
        // empty stub and got everything from the pickle; keep it that way and
        // only add what a stub cannot have: the implicits, below.
        if internal.starts_with("scala/") {
            self.st.get_mut(mid).members.clear();
        }
        let mut work = vec![mid];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = work.pop() {
            if id.is_none() || !seen.insert(id.0) {
                continue;
            }
            self.pickle
                .supply_implicit_members(&mut self.st, &mut self.binary, id);
            for p in self.st.get(id).parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    work.push(ps);
                }
            }
        }
    }

    /// Apply the implicit clauses of an implicit conversion that has any.
    ///
    /// `tree` is the conversion already applied to the receiver; a conversion
    /// like cats' `toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])`
    /// needs a second application for its implicit clause, or codegen emits a
    /// call one argument short of the descriptor.
    pub(crate) fn fill_conv_implicits(
        &mut self,
        conv: SymbolId,
        from: &Type,
        mut tree: Tree,
        span: Span,
    ) -> Tree {
        for clause in self.conv_implicit_params(conv, from, &tree.ty) {
            let mut args = Vec::with_capacity(clause.len());
            for want in &clause {
                if let Type::Class { sym, args } = want {
                    if self.st.get(*sym).jvm_name == "scala/reflect/ClassTag" {
                        if let Some(Type::TypeParam(tp)) = args.first() {
                            if self.st.get(conv).tparams.contains(tp) {
                                self.error(
                                    span,
                                    format!(
                                        "type {} is an unresolved spliceable type",
                                        self.st.get(*tp).name
                                    ),
                                );
                                return tree;
                            }
                        }
                    }
                }
                self.warm_implicit_scope(want);
                let mut search = self.search_implicit(want);
                // The same completion `fill_implicit_params_in` does for an
                // ordinary implicit clause (`agent/tail6`): a candidate whose
                // class came from a jar answers for its supertypes only once
                // something has read its parents, and the search itself runs
                // under an immutable borrow. `implicit val asyncF: Async[F]`
                // in a trait could not answer `toFlatMapOps`'s `FlatMap[F]`
                // until some earlier line in the same file happened to warm
                // it (slick's `BasicBackend.scala`, `run`).
                if matches!(search, ImplicitSearch::None)
                    && self.warm_implicit_candidates(std::slice::from_ref(want))
                {
                    search = self.search_implicit(want);
                }
                match search {
                    ImplicitSearch::Found(id) => {
                        let mut a = self.implicit_tree(id, want, span, 0);
                        self.adapt(&mut a, want);
                        args.push(a);
                    }
                    ImplicitSearch::None if self.classtag_apply_fallback(want, span).is_some() => {
                        args.push(self.classtag_apply_fallback(want, span).unwrap());
                    }
                    _ => {
                        let diverged = self.diverged_implicit.borrow().clone();
                        self.error(span, self.missing_implicit_message(want, diverged));
                        return tree;
                    }
                }
            }
            let ty = tree.ty.clone();
            tree = Tree {
                id: NodeId(0),
                span,
                kind: TreeKind::Apply {
                    fun: Box::new(tree),
                    args,
                },
                ty,
                sym: conv,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
                byname_thunk: false,
                byname_type_marker: false,
            };
        }
        // This application was synthesized rather than passed through
        // type_apply. Its escaping parameters still belong to the caller's
        // inference problem, just like a polymorphic receiver written out.
        let open = self.undetermined_of(&tree);
        self.undet_tvars.extend(open);
        tree
    }

    /// Make sure every companion object in `pt`'s implicit scope is loaded.
    ///
    /// A companion that came from a jar is a class file of its own that nothing
    /// else asks for, so without this `Async[IO]` searches an implicit scope
    /// that does not yet contain `cats.effect.IO.asyncForIO` — the only place
    /// that witness exists. The search runs under an immutable borrow and
    /// cannot load anything itself, so it has to happen here, on demand: doing
    /// it for every jar class as it is adopted pulls in the whole transitive
    /// closure of cats-effect and takes minutes.
    pub(crate) fn warm_implicit_scope(&mut self, pt: &Type) {
        self.warm_implicit_scope_once(pt);
    }

    /// The same, for the *candidates* rather than the wanted type.
    ///
    /// A candidate fits a supertype of its own declared type, and for a class
    /// that came from a jar those parents are only read when something warms
    /// it. `class C[F[_]](implicit F: Async[F])` asking for `Sync[F]` -- or,
    /// through `cats.effect.syntax`, `GenTemporal[F, E]` -- searched with
    /// `Async`'s parent list still empty and found nothing; asking for
    /// `Async[F]` first anywhere in the same file made the later searches
    /// work, which is the shape of a missing completion, not of a scoping
    /// rule. Reported by `slick/basic/ConcurrencyControl.scala`.
    ///
    /// Run only after a search has already come up empty: it reads a pickle
    /// per candidate class, and `warmed_scopes` makes each one a one-off, but
    /// the walk itself is not free. Answers whether anything was new, so the
    /// caller only retries the search when a retry could say something else.
    ///
    /// `wanted` are the types the search came up empty on. Their classes need
    /// their parents just as much: `candidate_bounds_hold` asks whether the
    /// solution for a candidate's `Level <: ShapeLevel` is one, and slick's
    /// `Query.map` wants a `Shape[_ <: FlatShapeLevel, …]`, so the answer is
    /// read off `FlatShapeLevel`'s parents -- a jar class the program never
    /// names. Empty, that said no, and `q.map(_.title)` was "could not find
    /// implicit value of type Shape[_ <: FlatShapeLevel, Rep[String], T, G]"
    /// while naming `FlatShapeLevel` anywhere in the same file fixed it.
    pub(crate) fn warm_implicit_candidates(&mut self, wanted: &[Type]) -> bool {
        let mut cands = self.implicits_in_scope();
        // The companion candidates too. `search_implicit_uncached` falls back
        // to `companion_implicits(pt)` when nothing lexical fits, so those are
        // real candidates and their result types need parents just as much --
        // and they are the ones the program is *least* likely to have named,
        // since the whole point of SLS 7.2 is that they need no import.
        // `TypedType.typedTypeToOptionTypedType[T]: OptionTypedType[T]` is the
        // case: `OptionTypedType extends TypedType[Option[T]]` is the only
        // thing that makes it fit `TypedType[Option[String]]`, and with its
        // parent list still empty `plausibly_inhabits` rejected it before a
        // single unification ran. Every `column[Option[T]]` in slick was
        // "could not find implicit value of type TypedType[Option[String]]",
        // and writing `OptionTypedType` anywhere in the same file fixed it.
        for w in wanted {
            cands.extend(self.companion_implicits(w));
        }
        cands.sort_unstable_by_key(|id| id.0);
        cands.dedup_by_key(|id| id.0);
        let mut completed = false;
        for &id in &cands {
            let before = self.st.get(id).ty.clone();
            let unknown = match &before {
                Type::Method { ret, .. } => ret.is_no_type(),
                ty => ty.is_no_type(),
            };
            if unknown && !self.lazy_completing.contains(&id) {
                self.complete_lazy_sig(id, Span::DUMMY);
                completed |= self.st.get(id).ty != before;
            }
        }
        let instance_depth = wanted
            .iter()
            .map(crate::implicits::complexity)
            .max()
            .unwrap_or(0)
            .max(crate::implicits::MAX_IMPLICIT_DEPTH);
        for &id in &cands {
            completed |= self.prepare_implicit_instances(id, instance_depth);
        }
        let tys: Vec<Type> = cands
            .into_iter()
            .map(|id| self.implicit_candidate_ty(id).into_owned())
            .chain(wanted.iter().cloned())
            .collect();
        let mut fresh = completed;
        for t in tys {
            // Parents only. Warming a candidate's *implicit scope* the way
            // `warm_implicit_scope` warms the wanted type's pulls pickled
            // parents onto standard-library companions -- the hazard
            // `warm_own_scope_once` documents -- and cost slick two new
            // `containsSymbol(Set[A])` overload errors when it was tried.
            fresh |= self.ensure_pickled_parents(&t);
        }
        fresh
    }

    /// Attach the pickled parents of every class `ty` names, and of the
    /// parents that appear as that goes on.
    ///
    /// `PickleSupply::attach_parents` runs one level at a time and only for a
    /// class something has completed a member of. A candidate the program
    /// merely *named* -- `(implicit F: Async[F])` -- has an empty parent list
    /// until then, so it fits nothing but its own type. Answers whether any
    /// class gained parents.
    fn ensure_pickled_parents(&mut self, ty: &Type) -> bool {
        if !self.library_abi {
            return false;
        }
        let mut work: Vec<SymbolId> = self.implicit_scope_classes(ty);
        let mut seen: std::collections::HashSet<u32> = work.iter().map(|c| c.0).collect();
        let mut fresh = false;
        while let Some(c) = work.pop() {
            if c.is_none() {
                continue;
            }
            // Never the standard library. Its hierarchy is the prelude's,
            // hand-written and reasoned about, and topping it up from the
            // class files rewrote `mutable.HashSet`'s parents well enough to
            // turn `HashSet[A]`'s `+`/`contains` into `Set`'s -- two new
            // errors in slick for a hierarchy nobody had asked to change.
            // What this is for is a *jar* class the program only named.
            let jvm = self.st.get(c).jvm_name.clone();
            if c.0 < self.st.prelude_end || jvm.starts_with("scala/") || jvm.starts_with("java/") {
                continue;
            }
            let before = self.st.get(c).parents.len();
            // Only parents are needed for a module. Whole-module adoption
            // can replace the implicit declarations just published by its
            // companion loader, including nested implicit object identities.
            if !matches!(self.st.get(c).kind, SymKind::Module | SymKind::ModuleClass) {
                self.ensure_java_loaded(c, Span::DUMMY);
            }
            self.pickle
                .ensure_parents(&mut self.st, &mut self.binary, c);
            fresh |= self.st.get(c).parents.len() != before;
            for p in self.st.get(c).parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    if seen.insert(ps.0) {
                        work.push(ps);
                    }
                }
            }
        }
        fresh
    }

    /// [`Self::warm_implicit_scope`], reporting whether any class in `pt`'s
    /// implicit scope had not been warmed before. Callers that only want to
    /// *retry* something after new implicits appeared can skip the retry when
    /// this says nothing is new.
    pub(crate) fn warm_implicit_scope_once(&mut self, pt: &Type) -> bool {
        let mut fresh = false;
        for c in self.implicit_scope_classes(pt) {
            fresh |= self.warm_one_scope(c);
        }
        fresh
    }

    /// Warm only the class `ty` *names*, not the base classes SLS 7.2 adds to
    /// its implicit scope.
    ///
    /// Reading a companion's pickle attaches that companion's own pickled
    /// parents, and for a collection those are the factory traits the prelude
    /// models by hand: warming `mutable.Set[T]`'s full scope reached
    /// `collection.Iterable` and gave `Iterable$` (and `Seq$`, `Set$`, …) a
    /// pickled `IterableFactory.Delegate` parent, whose `apply[A](A*): CC[A]`
    /// then stood next to the prelude's own — and `mutable.Set[TypeSymbol]()`
    /// came back as `Set[A]`. The conversions this is here to find
    /// (`Option.option2Iterable`) live on the companion of the type itself, so
    /// nothing is lost by stopping there.
    /// Read the classfile behind each argument's class, so `is_sub_type` can
    /// see its parents. Answers whether any of them had not been read yet.
    ///
    /// `find_or_stub_java_class` enters a class named by a descriptor with
    /// `parents = [AnyRef]` and nothing else; until the classfile itself is
    /// read, that stub conforms to nothing. Overload scoring runs on `&self`
    /// and cannot read one, so the callers that fail ask for this and score
    /// again.
    pub(crate) fn warm_java_args(&mut self, arg_tys: &[Type]) -> bool {
        let classes: Vec<SymbolId> = arg_tys
            .iter()
            .filter_map(|t| self.st.class_sym_of(t))
            .collect();
        let mut fresh = false;
        for c in classes {
            let jvm = self.st.get(c).jvm_name.clone();
            if jvm.is_empty() || self.completed_java.contains(&jvm) {
                continue;
            }
            self.ensure_java_loaded(c, Span::DUMMY);
            fresh = true;
        }
        fresh
    }

    /// Force the class files behind an argument's type before the call is
    /// resolved against it.
    ///
    /// A `-cp` class the source never *names* has nothing to complete it:
    /// `ensure_class` leaves a `JAVA`-flagged placeholder for the ordinary
    /// loader, and the loader only runs where the program mentions the class.
    /// scalatra-forms' `mapping(...)` returns a `MappingValueType[T]`, which
    /// gitbucket only ever infers, so the class kept the empty `AnyRef` parent
    /// list and nothing knew it `implements ValueType[T]` -- the parameter type
    /// of the `post[T](path, form: ValueType[T])(action: T => Any)` it was
    /// being passed to. nsc has no such gap: a symbol's info is completed the
    /// first time anything asks for it, and `isApplicable` asks.
    ///
    /// Once per class, and only in library mode, so a call in a loop pays for
    /// nothing.
    pub(crate) fn complete_arg_classes(&mut self, tys: &[Type]) -> bool {
        if !self.library_abi {
            return false;
        }
        let mut fresh = false;
        for t in tys {
            let Some(c) = self.st.class_sym_of(t) else {
                continue;
            };
            if c.0 < self.st.prelude_end || !self.completed_arg_classes.insert(c.0) {
                continue;
            }
            fresh |= self.ensure_pickled_parents(t);
        }
        fresh
    }

    pub(crate) fn warm_own_scope_once(&mut self, ty: &Type) -> bool {
        match self.st.class_sym_of(ty) {
            Some(c) => self.warm_one_scope(c),
            None => false,
        }
    }

    fn warm_one_scope(&mut self, c: SymbolId) -> bool {
        if c.is_none() || !self.warmed_scopes.insert(c.0) {
            return false;
        }
        self.load_companion_module(c);
        self.warm_pickled_implicits(c);
        if self.st.get(c).kind == SymKind::ModuleClass
            && self.st.get(c).jvm_name.starts_with("scala/collection/")
        {
            crate::prelude_buildfrom::link_evidence_factories(&mut self.st);
        }
        true
    }

    /// The implicit members a *standard library* companion declares.
    ///
    /// [`Self::load_companion_module`] deliberately stops at `scala.*`: the
    /// prelude is what describes the library. But the prelude describes what
    /// programs *name*, and nothing ever names an implicit -- it is found by
    /// searching a scope. `Option.option2Iterable` was therefore in no
    /// member list at all, and `where.reduceLeft(f)` / `c.where.toSeq` on an
    /// `Option[Node]` (slick's `JdbcStatementBuilderComponent`) were
    /// `value reduceLeft is not a member of Option[Node]`.
    ///
    /// Only a name the companion has no member for is asked, so a
    /// hand-written prelude declaration still wins and no second copy of one
    /// is installed next to it.
    pub(crate) fn warm_binary_implicit_result_parents(&mut self, owner: SymbolId) {
        if owner.0 < self.st.prelude_end
            || self.st.source_classes.contains(&owner)
            || (!self.st.pending_classpath_signatures.contains(&owner)
                && !self.pickle.pickle_readable(&self.st, owner))
        {
            return;
        }
        // A lazy companion can return a class eagerly found in a directory.
        // Restore that result hierarchy before an erased parent makes an
        // inapplicable candidate look like an ambiguous or successful match.
        let results: Vec<Type> = self
            .st
            .get(owner)
            .members
            .iter()
            .filter_map(|&id| {
                let s = self.st.get(id);
                if !s.flags.contains(Flags::IMPLICIT) {
                    return None;
                }
                let mut result = &s.ty;
                while let Type::Method { ret, .. } = result {
                    result = ret;
                }
                Some(result.clone())
            })
            .collect();
        for result in results {
            self.ensure_pickled_parents(&result);
        }
    }

    fn warm_pickled_implicits(&mut self, class_id: SymbolId) {
        if !self.library_abi || class_id.is_none() {
            return;
        }
        // `object Int`'s implicits are `int2long` / `int2float` / `int2double`
        // -- the numeric widenings, which `weak_conforms` already implements
        // directly. As *views* they would only compete: `n + ":"` has no
        // `Int#+(String)` in the prelude, so it is `any2stringadd`, and with
        // three more conversions that also offer a `+` the search became
        // ambiguous and the selection failed outright.
        if self.st.is_primitive_value_class(class_id) {
            return;
        }
        let mcls = match self.st.get(class_id).kind {
            SymKind::Module => self.st.module_class_of(class_id),
            SymKind::ModuleClass => class_id,
            _ => match self.st.companion_module(class_id) {
                Some(m) => self.st.module_class_of(m),
                None => return,
            },
        };
        if mcls.is_none() {
            return;
        }
        // The companion object's *own* declarations, and the ones it inherits.
        // SLS 7.2 names the object, and an object's members include inherited
        // ones: `object Ordering extends LowPriorityOrderingImplicits`, and
        // `ordered[A](implicit asComparable: A => Comparable[A])` -- the only
        // way to an `Ordering[Null]` (slick's `ScalaBaseType.nullType`) -- is
        // declared by that parent trait, whose member list stays empty until
        // something completes it. The parent is only asked for the members it
        // does not have; nothing about the hierarchy itself is touched.
        let mut work = vec![mcls];
        let mut walked = std::collections::HashSet::new();
        while let Some(c) = work.pop() {
            if c.is_none() || !walked.insert(c.0) {
                continue;
            }
            // Directory discovery can have installed the companion already.
            // Read only its implicit declarations, just as lazy jar discovery
            // does, without adopting an entire module beside existing members.
            if self.st.pending_classpath_signatures.contains(&c)
                && !self.st.source_classes.contains(&c)
                && !self.pickle.pickle_readable(&self.st, c)
            {
                self.pickle
                    .supply_implicit_members(&mut self.st, &mut self.binary, c);
            }
            for n in self
                .pickle
                .implicit_member_names(&self.st, &mut self.binary, c)
            {
                if !self
                    .st
                    .lookup_member(c, &n)
                    .iter()
                    .any(|&id| self.st.get(id).flags.contains(Flags::IMPLICIT))
                {
                    self.supply_from_pickle_class(c, &n);
                }
            }
            self.warm_binary_implicit_result_parents(c);
            for p in self.st.get(c).parents.clone() {
                if let Some(ps) = self.st.class_sym_of(&p) {
                    work.push(ps);
                }
            }
        }
    }

    pub(crate) fn load_binary_into(
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
            let Some(id) = crate::classpath::find_by_jvm(&self.st, internal) else {
                return false;
            };
            // Read once, but not necessarily *for this owner*: see
            // `classpath::enter_loaded_in_owner`.
            crate::classpath::enter_loaded_in_owner(&mut self.st, id, owner);
            return true;
        }
        match self.binary.find_class(internal) {
            Ok(Some(bytes)) => match crate::javaclass::parse_java_classfile(&bytes) {
                Ok(jc) => {
                    let id = crate::classpath::install_java_class_in(&mut self.st, &jc, owner);
                    // A Scala classfile on `-cp` carries a `ScalaSignature`,
                    // and that is the only place its higher kinds are written
                    // down. Read them off it before anything looks at the
                    // symbol; the classfile reader's view stays underneath for
                    // whatever the pickle cannot express.
                    if jc.is_scala {
                        self.pickle
                            .adopt_binary_class(&mut self.st, &mut self.binary, id);
                    }
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
        // `InnerClasses` records every nested class the file *mentions*, not
        // only the ones it declares: `cats/effect/kernel/MonadCancel.class`
        // lists `cats/syntax/package$all$`. Adopting those installed
        // `cats.syntax.all` as a member of `MonadCancel`, and since
        // `load_binary_into` completes a class file once, the later
        // `import cats.syntax.all._` found nothing to load and nothing in the
        // package object -- but only when something under `cats.effect` was
        // imported first, which is why it looked like an ordering quirk.
        let nest_prefix = format!("{}$", jvm.trim_end_matches('$'));
        for inner in &jc.inner_classes {
            if !inner.inner_jvm.ends_with('$') || inner.inner_jvm.contains("$anon") {
                continue;
            }
            if !inner.inner_jvm.starts_with(&nest_prefix) {
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

    pub(crate) fn complete_java_type(&mut self, ty: &Type, span: Span) {
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

    /// `name` on any parent of a receiver whose upper bound is a **compound**.
    ///
    /// `SymbolTable::class_sym_of` answers with one symbol, and for a
    /// `Type::Refined` it takes the first parent that is a class -- so a
    /// member declared by the *second* half of a bound is unreachable.
    /// `scala.reflect.api.Names` declares
    ///
    /// ```text
    /// type TypeName >: Null <: TypeNameApi with Name
    /// ```
    ///
    /// where `trait TypeNameApi` is empty (it exists only to give `TypeName`
    /// an erased identity) and everything a name can do -- `toTermName`,
    /// `decodedName`, `isTermName` -- comes from `Name`, through `NameApi`.
    /// `symbolOf[R].name.toTermName`, which is how slick's `mapToImpl` gets
    /// at a case class's companion, was "value toTermName is not a member of
    /// Names.TypeName".
    ///
    /// Runs only after the ordinary search and the pickle have both found
    /// nothing, so it can add members and never replace one.
    pub(crate) fn members_through_compound_bound(
        &mut self,
        recv_ty: &Type,
        name: &str,
    ) -> Vec<SymbolId> {
        let id = match recv_ty {
            Type::TypeMember(id) | Type::TypeParam(id) => *id,
            _ => return Vec::new(),
        };
        let Some(Type::Refined { parents, .. }) = self.st.get(id).bound_hi.clone() else {
            return Vec::new();
        };
        let mut out: Vec<SymbolId> = Vec::new();
        for p in &parents {
            if let Some(o) = self.st.class_sym_of(p) {
                for m in self.st.lookup_member(o, name) {
                    if !out.contains(&m) {
                        out.push(m);
                    }
                }
            }
            if out.is_empty() {
                for m in self.supply_from_pickle(p, name) {
                    if !out.contains(&m) {
                        out.push(m);
                    }
                }
            }
        }
        out
    }

    /// Install `name` on the receiver's class from the library `ScalaSignature`
    /// and return whatever that made visible. Empty unless the receiver is a
    /// standard-library class *and* the member could be expressed faithfully.
    pub(crate) fn supply_from_pickle(&mut self, recv_ty: &Type, name: &str) -> Vec<SymbolId> {
        if !self.library_abi {
            return Vec::new();
        }
        let Some(cls) = self.st.class_sym_of(recv_ty) else {
            // Worth tracing: a receiver that never resolved to a class symbol
            // (a `Type::Named` left behind by a pickle that records member
            // types by simple name) can never be completed, and the user only
            // sees "is not a member".
            crate::pickle_supply::trace(format_args!(
                "#{name}: receiver {} has no class symbol",
                self.st.display_type(recv_ty)
            ));
            return Vec::new();
        };
        crate::pickle_supply::trace(format_args!(
            "#{name}: asking {} ({})",
            self.st.get(cls).name,
            self.st.get(cls).jvm_name
        ));
        // Members found on a companion object land on that module class, not
        // on `cls`, so take what completion reports rather than re-looking-up.
        self.pickle
            .complete(&mut self.st, &mut self.binary, cls, name)
    }

    /// The receiver's *own* declaration of a member it also inherits.
    ///
    /// Library members are read from the pickle on demand and installed on the
    /// class that declares them, so what the typer already has depends on what
    /// earlier code happened to ask for. `Map#collect` is
    /// `MapOps.collect[K2, V2](pf): Map[K2, V2]`, and once some `aMap.collect`
    /// has installed it, `aTreeMap.collect` finds it by inheritance and never
    /// asks `TreeMap` -- whose own `collect(pf)(implicit Ordering[K2]):
    /// TreeMap[K2, V2]` is the one nsc picks. The call then went out as
    /// `IterableOps.collect`, whose default implementation builds through
    /// `iterableFactory`: `TreeMap(1 -> "a").collect(pf)` *returned a `List`*,
    /// with no diagnostic anywhere. Which of the two you got depended on
    /// whether a plain `Map.collect` appeared earlier in the file.
    ///
    /// So when every candidate is inherited, ask the receiver's class too and
    /// union the answers -- `drop_overridden` and the specificity rules then
    /// choose, as they do for a class the typer read in full. Completion is
    /// memoised per `(class, name)`, so this costs one pickle walk per pair.
    pub(crate) fn supply_receiver_override(
        &mut self,
        recv_ty: &Type,
        name: &str,
        found: &mut Vec<SymbolId>,
    ) {
        if !self.library_abi || found.is_empty() {
            return;
        }
        let Some(cls) = self.st.class_sym_of(recv_ty) else {
            return;
        };
        // Nothing to add when the receiver's class already declares one.
        if found.iter().any(|&m| self.st.get(m).owner == cls) {
            return;
        }
        // A real extra JVM signature is an overload, not merely a receiver
        // substitution. Preserve the established overload completion first:
        // TreeMap.map(f)(Ordering) must not disappear when Map.map was loaded
        // by a preceding unit. The result-only path below must not intercept it.
        let have: Vec<Vec<Option<String>>> = found
            .iter()
            .map(|&m| crate::pickle_supply::flat_erased_params(&self.st, &self.st.get(m).ty))
            .collect();
        if self.ancestor_declares_other_signature(cls, name, &have, found) {
            if !self.supply_from_pickle_class(cls, name).is_empty() {
                let now = self.st.lookup_member(cls, name);
                if now.iter().any(|&m| self.st.get(m).owner == cls) {
                    *found = now;
                }
            }
            return;
        }
        let library_collection = self.st.get(cls).jvm_name.starts_with("scala/collection/");
        // And the hierarchy `drop_overridden` is about to order the candidates
        // by is the pickled one for the receiver and for every *library*
        // owner that is not the prelude's. A prelude class is left alone here
        // on purpose, measured: attaching `immutable.Set`'s pickled parents
        // put the prelude's crude `Set.map: ((A) => Any)Set[Any]` below
        // `IterableOps.map[B]` in the same hierarchy, `drop_overridden` then
        // kept only the prelude's, and `aHashSet.map[Int](f)` had no `B` for
        // its type argument (`crates/cli/tests/arraygen.rs`). Duplicated nullary
        // alternatives are resolved in value position by maybe_auto_apply.
        if library_collection {
            let owners: Vec<SymbolId> = found.iter().map(|&m| self.st.get(m).owner).collect();
            for owner in owners.into_iter().chain(std::iter::once(cls)) {
                if !owner.is_none()
                    && owner.0 >= self.st.prelude_end
                    && self.st.get(owner).jvm_name.starts_with("scala/")
                {
                    self.pickle
                        .ensure_parents(&mut self.st, &mut self.binary, owner);
                }
            }
        }
        let all_from_library_binaries = found.iter().all(|&m| {
            let s = self.st.get(m);
            let owner = s.owner;
            !owner.is_none()
                && self.st.get(owner).jvm_name.starts_with("scala/")
                && m.0 >= self.st.prelude_end
                && (!s.pickled_origin.is_empty() || s.jvm_name.starts_with('('))
        });
        // The receiver's copies have to be the *inherited* declarations
        // re-read at the receiver, shape for shape. `PickleSupply::install`
        // keeps one member per erased parameter list, the first the walk
        // offers, so a receiver whose pickle overloads a name on the same
        // erasure -- `HashMap.++` is `MapOps.++[V2 >: V]: CC[K, V2]` over
        // `IterableOps.++[B >: A]: CC[B]` -- can come back with the wrong
        // one (`Iterable[(K, V)]`, and `.get` is not a member of it:
        // `crates/cli/tests/asttype.rs`). The origin says which declaration
        // a copy stands for; a class-file forwarder names none and agrees
        // on its erased parameters instead. A copy of anything else is
        // taken off the class again, because a member left on the receiver
        // is found by every later selection whether or not this one used it.
        if library_collection && all_from_library_binaries {
            let own: Vec<SymbolId> = self
                .pickle
                .complete_on_class(&mut self.st, &mut self.binary, cls, name)
                .into_iter()
                .filter(|&m| self.st.get(m).owner == cls)
                .collect();
            if own.is_empty() {
                return;
            }
            let inherited: Vec<(String, Vec<Option<String>>)> = found
                .iter()
                .map(|&m| {
                    let s = self.st.get(m);
                    (
                        s.pickled_origin.clone(),
                        crate::pickle_supply::flat_erased_params(&self.st, &s.ty),
                    )
                })
                .collect();
            // A copy standing for the receiver's *own* declaration is the
            // override nsc would pick -- `LazyList` redeclares `grouped` and
            // `sliding(size, step)` itself -- and so is one standing for a
            // declaration *below* the inherited candidate's: `SortedMap.map`
            // is `SortedMapOps.map[K2, V2](f)(implicit Ordering[K2])` over
            // `MapOps.map`, and `SortedMapOps` extends `MapOps`. What is
            // refused is a copy of a declaration the inherited one does not
            // reach: `IterableOps.++` next to `MapOps.++` is the sibling the
            // walk should have passed over.
            let own_prefix = format!("{}#", self.st.get(cls).jvm_name.replace('/', "."));
            let declaring = |this: &Self, origin: &str| -> Option<SymbolId> {
                let dotted = origin.split('#').next()?;
                crate::classpath::find_by_jvm(&this.st, &dotted.replace('.', "/"))
            };
            let mut rejected = Vec::new();
            for &m in &own {
                let s = self.st.get(m);
                let params = crate::pickle_supply::flat_erased_params(&self.st, &s.ty);
                let origin = s.pickled_origin.clone();
                let of_shape: Vec<&String> = inherited
                    .iter()
                    .filter(|(_, p)| *p == params)
                    .map(|(o, _)| o)
                    .collect();
                // Judge each alternative independently. An unrelated
                // IterableOps.map copy must not discard SortedMapOps.map's
                // genuine additional Ordering clause alongside it.
                let valid = if of_shape.is_empty() || origin.starts_with(&own_prefix) {
                    true
                } else if let Some(below) = declaring(self, &origin) {
                    if below.0 >= self.st.prelude_end {
                        self.pickle
                            .ensure_parents(&mut self.st, &mut self.binary, below);
                    }
                    of_shape.iter().all(|other| {
                        other.is_empty()
                            || **other == origin
                            || declaring(self, other).is_some_and(|above| {
                                crate::pickle_supply::inherits_from(&self.st, below, above)
                            })
                    })
                } else {
                    false
                };
                if !valid {
                    rejected.push(m);
                }
            }
            self.st
                .get_mut(cls)
                .members
                .retain(|m| !rejected.contains(m));
            if rejected.len() < own.len() {
                *found = self.st.lookup_member(cls, name);
            }
        }
    }

    /// [`Self::declares_other_signature`], asked of the receiver's class *and*
    /// of every ancestor more derived than the answer already in hand.
    ///
    /// A trait declares nothing in its own class file that a `Ops` trait
    /// declares for it: `scala/collection/immutable/SortedMap.class` has no
    /// `map`, no `collect` and no `keySet` -- they are declared by
    /// `collection.SortedMapOps`, which `immutable.SortedMap` inherits. So
    /// asking `cls` alone answers "no" for exactly the family this guard
    /// exists to admit, and `aSortedMap.map(f)` kept whatever a plain
    /// `aMap.map(f)` earlier in the run had installed on `collection.MapOps`.
    ///
    /// The walk stops at the first class that already owns a candidate: past
    /// that point a declaration is not *newer* than the answer in hand, it
    /// *is* the answer in hand or something it overrides, and going further
    /// would re-complete every receiver in the library.
    fn ancestor_declares_other_signature(
        &mut self,
        cls: SymbolId,
        name: &str,
        have: &[Vec<Option<String>>],
        found: &[SymbolId],
    ) -> bool {
        let owners: Vec<SymbolId> = found.iter().map(|&m| self.st.get(m).owner).collect();
        for c in crate::lin::linearize(&self.st, cls) {
            if owners.contains(&c) {
                break;
            }
            if self.declares_other_signature(c, name, have) {
                return true;
            }
        }
        self.pickle_declares_other_arity(cls, name, found)
    }

    /// The receiver's pickle declares `name` somewhere the candidates in hand
    /// do not stand for. The prelude leaves the `…Ops` traits out of its
    /// hierarchy, so the class-file walk above never meets
    /// `SortedSetOps.map[B](f)(implicit ord: Ordering[B]): SortedSet[B]`, and
    /// `aSortedSet.map(f)` resolved to the prelude's plain `immutable.Set.map`:
    /// the call built a `HashSet` (nsc: a `TreeSet`), and a result the typer
    /// had narrowed back to `SortedSet` failed its `checkcast`.
    ///
    /// For each parameter count, the most derived pickled declaration must
    /// be represented. A prelude member stands for any declaration of its
    /// count -- the prelude's own types are what existing programs are
    /// checked against, and `Set.map` does stand for `IterableOps.map` on a
    /// `HashSet`. Once pickled copies are among the candidates, though, they
    /// have to be copies of that very declaration: after some
    /// `aSortedSet.map` has installed `SortedSetOps.map` / `IterableOps.map`
    /// on `SortedSet`, a `BitSet` inherits them, and its own
    /// `BitSetOps.map(f: Int => Int): BitSet` (what nsc picks) would never be
    /// read.
    fn pickle_declares_other_arity(
        &mut self,
        cls: SymbolId,
        name: &str,
        found: &[SymbolId],
    ) -> bool {
        let internal = self.st.get(cls).jvm_name.clone();
        if !internal.starts_with("scala/collection/") {
            return false;
        }
        let decls = self
            .pickle
            .pickled_arities(&mut self.binary, &internal, name);
        let mut firsts: Vec<(usize, String)> = Vec::new();
        for (n, owner) in decls {
            if !firsts.iter().any(|(k, _)| *k == n) {
                firsts.push((n, owner));
            }
        }
        let pickled_in_hand = found
            .iter()
            .any(|&m| !self.st.get(m).pickled_origin.is_empty());
        firsts.iter().any(|(n, owner)| {
            !found.iter().any(|&m| {
                if self.value_param_count(m) != *n {
                    return false;
                }
                let s = self.st.get(m);
                match s.pickled_origin.split_once('#') {
                    Some((origin, _)) => origin == owner,
                    None => !pickled_in_hand || m.0 >= self.st.prelude_end,
                }
            })
        })
    }

    /// Whether `cls`'s own classfile declares an instance method `name` whose
    /// erased parameter list matches none of the candidates in `have`.
    ///
    /// A candidate parameter whose erasure this cannot name is a wildcard, so
    /// an unreadable candidate never *adds* a reason to go to the pickle.
    fn declares_other_signature(
        &mut self,
        cls: SymbolId,
        name: &str,
        have: &[Vec<Option<String>>],
    ) -> bool {
        let internal = self.st.get(cls).jvm_name.clone();
        if internal.is_empty() || !internal.starts_with("scala/") {
            return false;
        }
        let enc = scala_rs_pickle::names::encode_method_name(name);
        let Ok(Some(bytes)) = self.binary.find_class(&internal) else {
            return false;
        };
        let Ok(jc) = crate::javaclass::parse_java_classfile(&bytes) else {
            return false;
        };
        jc.methods.iter().any(|m| {
            if m.name != enc || crate::javaclass::is_java_static(m.access) {
                return false;
            }
            // A bridge is the compiler's own copy of an inherited signature,
            // never a new alternative.
            if crate::javaclass::is_java_bridge(m.access) {
                return false;
            }
            let Some(got) = crate::pickle_supply::desc_params(&m.desc) else {
                return false;
            };
            !have.iter().any(|want| {
                want.len() == got.len()
                    && want
                        .iter()
                        .zip(&got)
                        .all(|(w, g)| w.as_ref().is_none_or(|w| w == g))
            })
        })
    }

    /// The number of value parameters a method takes, across all its clauses --
    /// the arity the JVM sees.
    fn value_param_count(&self, m: SymbolId) -> usize {
        match &self.st.get(m).ty {
            Type::Method { paramss, .. } => paramss.iter().map(|c| c.len()).sum(),
            _ => 0,
        }
    }

    /// [`Self::supply_from_pickle`] for a class symbol that is already known.
    ///
    /// The receiver-typed form reaches this through `class_sym_of`; a wildcard
    /// import (`import <a value>._`) has the class in hand and no receiver type
    /// to hand back.
    /// Read a `-cp` companion object's shape off its `ScalaSignature` pickle
    /// before the `Module[T]` redirect asks it for `apply`.
    ///
    /// A Scala class file on `-cp` gets two readings: the class file itself
    /// (`install_java_class`: erased descriptors plus the JVM generic
    /// signature) and, for the classes `PickleSupply` has *adopted*, the
    /// pickle. Only the pickle records which parameter clause is implicit --
    /// the JVM has no such notion -- and only the pickle can write a higher
    /// kind. `load_binary_into` adopts the class it loads, but a companion
    /// *module* class reached through a package object's re-export
    /// (`val Async = cats.effect.kernel.Async`, which is how `import
    /// cats.effect.Async` arrives) is only ever stubbed by
    /// `find_or_stub_java_class`, which adopts nothing. `Async$` then kept
    /// the class file's `apply(x$0: Async[F]): Async[F]` with an *explicit*
    /// parameter, `complete_named` refused to serve the module class at all,
    /// and `Async[F].flatMap(…)` was "value flatMap is not a member of
    /// `Async$`".
    ///
    /// Only module classes, and only where the redirect is about to ask for
    /// `apply` anyway: adopting a companion installs every member it
    /// declares, which is not something to do speculatively.
    pub(crate) fn adopt_cp_module_class(&mut self, cls: SymbolId) {
        // `adopt_binary_class` declines `java.*` and the prelude's own
        // `scala.*` classes itself; a name that is not a companion's cannot
        // have a companion pickle to read.
        if !self.library_abi
            || cls.is_none()
            || self.st.get(cls).kind != SymKind::ModuleClass
            || !self.st.get(cls).jvm_name.ends_with('$')
        {
            return;
        }
        self.pickle
            .adopt_binary_class(&mut self.st, &mut self.binary, cls);
    }

    /// Repair a `-cp` class that has no constructor at all from its pickle.
    ///
    /// See [`scala_rs_typer::pickle_supply::PickleSupply::supply_ctors`]: a
    /// nested Scala class's own class file carries no `ScalaSignature`, so a
    /// class reached through a type alias (`type Table[T] = …`, which is how
    /// slick exports every one of its abstract classes) is completed from the
    /// enclosing class's pickle -- where constructors are skipped by name.
    pub(crate) fn supply_binary_ctors(&mut self, cls: SymbolId) {
        if !self.library_abi || cls.is_none() {
            return;
        }
        self.pickle
            .supply_ctors(&mut self.st, &mut self.binary, cls);
    }

    pub(crate) fn supply_from_pickle_class(&mut self, cls: SymbolId, name: &str) -> Vec<SymbolId> {
        if !self.library_abi || cls.is_none() {
            return Vec::new();
        }
        self.pickle
            .complete(&mut self.st, &mut self.binary, cls, name)
    }

    /// The module class a *value* reference stands for, if it stands for one.
    ///
    /// A module reference carries `Type::ModuleRef`; the `scala` package
    /// object's aliases (`val Equiv = math.Equiv`) reach us as nullary
    /// accessors, so the same thing also arrives wrapped in a method type with
    /// no parameters. Used by `Module[T]` → `Module.apply[T]`: without it
    /// `Equiv[Int]` kept the module class as its type and `.equiv` was "not a
    /// member of Equiv$".
    pub(crate) fn module_class_of_value(&self, sym: SymbolId, ty: &Type) -> Option<SymbolId> {
        let s = self.st.get(sym);
        if !matches!(s.kind, SymKind::Method | SymKind::Term) || !s.params.is_empty() {
            return None;
        }
        let peeled = match ty {
            Type::Method { paramss, ret } if paramss.iter().all(|c| c.is_empty()) => {
                (**ret).clone()
            }
            other => other.clone(),
        };
        match peeled {
            Type::ModuleRef(c) => Some(c),
            Type::Class { sym: c, .. } if self.st.get(c).kind == SymKind::ModuleClass => Some(c),
            _ => None,
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
        // Classpath discovery records only a shallow Scala signature. Adopt
        // the complete one lazily, without re-reading source/prelude classes.
        // Remove first because completion can recursively request this class.
        if self.st.pending_classpath_signatures.remove(&class_id)
            && !self.st.source_classes.contains(&class_id)
            && self
                .pickle
                .adopt_binary_class(&mut self.st, &mut self.binary, class_id)
        {
            // The directory header has only erased parent names. Restore
            // type arguments before inherited members are viewed through
            // this receiver, and complete those parents' declarations too.
            self.pickle
                .ensure_parents(&mut self.st, &mut self.binary, class_id);
            self.complete_java_parents(class_id, span);
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
                    let id = crate::classpath::install_java_class(&mut self.st, &jc);
                    // A `-cp` stub reached through a parent list arrives here
                    // as "javaish" even when it is a Scala trait: see
                    // `adopt_binary_class`.
                    if jc.is_scala {
                        self.pickle
                            .adopt_binary_class(&mut self.st, &mut self.binary, id);
                    }
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

    /// Read raw members from a Scala classfile even when its pickle has
    /// already been adopted.  Most Scala members are supplied from pickles on
    /// demand; constructor default getters are the one JVM-only exception:
    /// the pickle spells `<init>$default$n`, while the classfile exposes the
    /// static `$lessinit$greater$default$n` forwarder.
    pub(crate) fn ensure_classfile_members_loaded(
        &mut self,
        class_id: SymbolId,
        member_name: &str,
        span: Span,
    ) {
        if class_id.is_none() {
            return;
        }
        // The pickle already supplies the source spelling (`<init>$default$n`)
        // but not the JVM forwarder.  Once that alias is installed, avoid
        // reparsing and reinstalling the whole class for every later default.
        // `install_java_class_in` merges members into the existing symbol and
        // preserves pickle-only flags, but this guard keeps that merge a
        // one-time classfile completion rather than making symbol state depend
        // on the order in which defaults are visited.
        if !member_name.is_empty()
            && self
                .st
                .lookup_member(class_id, member_name)
                .iter()
                .any(|&id| self.st.get(id).kind == SymKind::Method)
        {
            return;
        }
        let jvm = self.st.get(class_id).jvm_name.clone();
        if jvm.is_empty() || jvm.starts_with('[') {
            return;
        }
        let Ok(Some(bytes)) = self.binary.find_class(&jvm) else {
            return;
        };
        let Ok(jc) = crate::javaclass::parse_java_classfile(&bytes) else {
            return;
        };
        let owner = self.st.get(class_id).owner;
        // Reloading JVM members must not erase an already-read Scala parent
        // list: a value class's header says Object, while its pickle says
        // AnyVal. Pickle completion is memoized and would not restore it.
        let scala_parents = (jc.is_scala && self.pickle.parents_loaded(class_id))
            .then(|| self.st.get(class_id).parents.clone());
        let id = crate::classpath::install_java_class_in(&mut self.st, &jc, owner);
        if let Some(parents) = scala_parents {
            self.st.get_mut(id).parents = parents;
        }
        if jc.is_scala {
            self.pickle
                .adopt_binary_class(&mut self.st, &mut self.binary, id);
        }
        self.complete_java_parents(class_id, span);
    }

    /// Mark constructor parameters whose JVM getter exists in a separately
    /// compiled class.  The constructor pickle records the parameter types,
    /// but `supply_ctors` intentionally does not infer `DEFAULTPARAM` from a
    /// classfile: the default body lives on the class's companion and the
    /// classfile has no parameter flag for it.  The forwarder is still a
    /// precise witness (`$lessinit$greater$default$n`), including for
    /// `-Xno-forwarders` and nested classes where only `C$` has the method.
    fn link_existing_nested_companion(&mut self, class_id: SymbolId) {
        if self.st.companion_module(class_id).is_some() {
            return;
        }
        let class_jvm = self.st.get(class_id).jvm_name.clone();
        if class_jvm.is_empty() {
            return;
        }
        let module_jvm = format!("{class_jvm}$");
        let Some(mcls) = crate::classpath::find_by_jvm(&self.st, &module_jvm)
            .filter(|&id| self.st.get(id).kind == SymKind::ModuleClass)
        else {
            return;
        };
        let owner = self.st.get(class_id).owner;
        let module_owner = self.st.get(mcls).owner;
        let outer_module_owner = if !owner.is_none() {
            self.st
                .companion_module(owner)
                .map(|m| self.st.module_class_of(m))
        } else {
            None
        };
        if module_owner != owner && Some(module_owner) != outer_module_owner {
            return;
        }
        let name = self.st.get(class_id).name.clone();
        let Some(module) = self
            .st
            .get(module_owner)
            .members
            .iter()
            .copied()
            .find(|&id| self.st.get(id).kind == SymKind::Module && self.st.get(id).name == name)
        else {
            return;
        };
        if !self.st.get(owner).members.contains(&module) {
            self.st.get_mut(owner).members.push(module);
        }
    }

    pub(crate) fn ensure_external_ctor_defaults(&mut self, class_id: SymbolId, span: Span) {
        if class_id.is_none() {
            return;
        }
        self.ensure_classfile_members_loaded(class_id, "", span);
        self.load_companion_module(class_id);
        self.link_existing_nested_companion(class_id);
        if let Some(module) = self.st.companion_module(class_id) {
            let mcls = self.st.module_class_of(module);
            self.ensure_classfile_members_loaded(mcls, "", span);
        }
        // Constructor default ownership is source metadata, not a property of
        // the JVM getter name. Read and merge each pickled constructor against
        // its real descriptor; this also covers an auxiliary constructor whose
        // default getter is forwarded from the class.
        self.supply_binary_ctors(class_id);
    }

    /// `resolve_type_name`, after completing any alias the name binds to. A
    /// reference from an earlier unit must not see the alias's `<notype>`.
    fn resolve_type_name_completing(&mut self, name: &str, args: &[Type], span: Span) -> Type {
        for id in self.st.lookup_type(name) {
            if self.st.get(id).kind == SymKind::TypeMember {
                self.complete_lazy_sig(id, span);
            } else if id.0 >= self.st.prelude_end
                && self.st.get(id).is_class_like()
                && self.st.get(id).flags.contains(Flags::JAVA)
            {
                // A preceding unit can introduce a Java class through another
                // class's member descriptor. A wildcard then binds that shallow
                // declaration directly, bypassing binary name discovery.
                // JAVA also marks Scala declarations discovered through binary
                // signatures. Complete those from their pickle, preserving
                // source constructor clauses and enclosing-instance metadata.
                if self
                    .pickle
                    .adopt_binary_class(&mut self.st, &mut self.binary, id)
                {
                    self.pickle
                        .ensure_parents(&mut self.st, &mut self.binary, id);
                } else {
                    self.ensure_java_loaded(id, span);
                }
            }
        }
        self.resolve_type_name(name, args)
    }

    /// An alias type member's right-hand side is written in its owner's
    /// vocabulary. Seen from the class being checked it takes that class's own
    /// arguments for the owner: inside `new SimpleFeatureNode[T] with …`,
    /// `type Self = SimpleFeatureNode[T]` means *this* `T`, not the one
    /// `SimpleFeatureNode` declares.
    fn type_member_here(&self, id: SymbolId) -> Type {
        let base = self.st.type_member_as_seen(id);
        // A deferred member stands for itself and has nothing to substitute.
        // An alias whose right-hand side is *another* member does: `type
        // Reader = M#Reader` (slick's `ResultConverter`) is an abstract
        // projection through the owner's type parameter `M`, and seen from a
        // subclass that fixes `M` it reduces -- `subst_tparams` does exactly
        // that through `subst_projections`. Returning it unsubstituted left
        // `r: Reader` in `class L extends Conv[IntDomain, String]` as `M#Reader`
        // while `next[Int].read` wanted the reduced `Int`.
        if matches!(base, Type::TypeMember(m) if m == id) {
            return base;
        }
        let owner = self.st.get(id).owner;
        let this = self.st.this_class;
        if owner.is_none() || this.is_none() || owner == this {
            return base;
        }
        if self.st.get(owner).tparams.is_empty() {
            return base;
        }
        // The class the alias is read *through*: `this` itself, or -- for a
        // nested class naming an alias its enclosing class inherits -- that
        // enclosing class. `class C extends Base[String] { class D { def
        // foo[B1 <: B](b: B1) } }` reads `Base`'s `type B = A` as `C.this.B`,
        // which is `String`; answering with `Base`'s own `A` erased `foo` to
        // `(Object)I` where scalac emits `(String)I` (run/t7120b).
        for site in self.st.enclosing_classes(this) {
            if site == owner {
                return base;
            }
            if site != this && !self.st.is_ancestor_of(owner, site) {
                continue;
            }
            let site_ty = Type::Class {
                sym: site,
                args: self
                    .st
                    .get(site)
                    .tparams
                    .iter()
                    .map(|&t| Type::TypeParam(t))
                    .collect(),
            };
            if let Some(Type::Class { args, .. }) = self.base_type_instance(&site_ty, owner, 0) {
                if !args.is_empty() {
                    return self.st.subst_tparams(owner, &args, &base);
                }
            }
        }
        base
    }

    /// The `scala._` / `java.lang._` wildcard imports every source carries
    /// supply `Int`, `String`, `Object` and the rest. That is SLS 2's level 3,
    /// so a definition (level 1, including an inherited one) or an explicit
    /// import (level 2) of the same simple name *hides* them.
    ///
    /// This used to be decided by the name alone, ahead of any scope lookup,
    /// and the standard library's own
    /// `object BitOperations { trait Int … ; object Int extends Int }` is what
    /// the miss costs: `object Int extends Int` took its parent to be
    /// `scala.Int` and compiled, silently, to `extends java.lang.Integer`, so
    /// `import BitOperations.Int._` offered nothing and `TreeSeqMap` reported
    /// `not found: value zero` five times over. There was no diagnostic
    /// anywhere -- only `javap` showed it.
    ///
    /// scala/scala's `src/library` declares `scala.Int` itself, at level 1 in
    /// its own compilation unit; that symbol *is* the builtin and a reference
    /// to it must stay `Type::Int`, which is what
    /// [`SymbolTable::is_prelude_scope_type`] filters out here.
    fn builtin_type_shadowed(&self, name: &str) -> bool {
        if !matches!(
            self.st.type_bind_rank(name),
            Some(BindRank::Definition | BindRank::Explicit | BindRank::Wildcard)
        ) {
            return false;
        }
        let found = self.st.lookup_type(name);
        !found.is_empty() && found.iter().all(|&id| !self.st.is_prelude_scope_type(id))
    }

    fn resolve_type_name(&self, name: &str, args: &[Type]) -> Type {
        let builtin = match name {
            "Int" => Some(Type::Int),
            "Long" => Some(Type::Long),
            "Double" => Some(Type::Double),
            "Float" => Some(Type::Float),
            "Boolean" => Some(Type::Boolean),
            "Byte" => Some(Type::Byte),
            "Short" => Some(Type::Short),
            "Unit" => Some(Type::Unit),
            "Char" => Some(Type::Char),
            "String" => Some(Type::String),
            "Any" => Some(Type::Any),
            "AnyRef" => Some(Type::AnyRef),
            "AnyVal" => Some(Type::AnyVal),
            "Nothing" => Some(Type::Nothing),
            "Null" => Some(Type::Null),
            "Object" => Some(Type::AnyRef),
            _ => None,
        };
        match builtin {
            Some(t) if !self.builtin_type_shadowed(name) => t,
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
                        SymKind::TypeParam => {
                            let this = self.st.this_class;
                            let owner = self.st.get(id).owner;
                            let mut lexical = self.st.owner;
                            let mut lexical_owner = false;
                            while !lexical.is_none() {
                                if lexical == owner {
                                    lexical_owner = true;
                                    break;
                                }
                                lexical = self.st.get(lexical).owner;
                            }
                            if !lexical_owner
                                && !this.is_none()
                                && owner != this
                                && self.st.is_ancestor_of(owner, this)
                            {
                                let recv = Type::Class {
                                    sym: this,
                                    args: self
                                        .st
                                        .get(this)
                                        .tparams
                                        .iter()
                                        .map(|p| Type::TypeParam(*p))
                                        .collect(),
                                };
                                self.st.subst_as_seen_from(&recv, &Type::TypeParam(id))
                            } else {
                                Type::TypeParam(id)
                            }
                        }
                        SymKind::TypeMember => self.type_member_here(id),
                        _ => Type::Class {
                            sym: id,
                            args: args.to_vec(),
                        },
                    }
                } else if let Some(t) = builtin {
                    // The shadowing binding was of a kind this arm does not
                    // answer with. A placeholder `Type::Named { name: "Int" }`
                    // would be far worse than the builtin, so keep the builtin.
                    t
                } else {
                    Type::Named {
                        name: name.into(),
                        args: args.to_vec(),
                    }
                }
            }
        }
    }
}
