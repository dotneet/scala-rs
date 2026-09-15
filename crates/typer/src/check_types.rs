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
use crate::symbol::{BindRank, SymKind, SymbolTable};
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

mod binary;

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
                let ty = self.rebind_imported_type_member(ty);
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
                        SymKind::TypeMember => self.module_path_type_member(qual, id),
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
            TreeKind::Select { .. } => self
                .term_path_sym(t)
                .is_some_and(|s| matches!(self.st.get(s).kind, SymKind::Term | SymKind::Method)),
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

    /// `O.T` for an `object O` that *inherits* the deferred `type T`, with the
    /// object kept as the path: `NonEmptySetImpl.Type`, where `Type` is
    /// declared in the `Newtype` trait the object mixes in.
    ///
    /// nsc writes `TypeRef(NonEmptySetImpl.type, Newtype.Type, args)`. Reading
    /// the declaration alone spelled one type two ways in our own pickles --
    /// `NonEmptySetImpl.this.Type[A]` inside the object, `Newtype.this.Type[A]`
    /// in `cats.data`'s `type NonEmptySet[A] = NonEmptySetImpl.Type[A]` -- and
    /// real scalac reading them reported the one against the other. The prefix
    /// also decides the implicit scope, which is where
    /// `NonEmptySetImpl.catsNonEmptySetOps` lives, so with the object gone
    /// every operation on a `NonEmptySet` was missing.
    ///
    /// Only a *deferred* member, and only one the object does not declare
    /// itself: a concrete alias carries its right-hand side already, and a
    /// declaration of the object's own needs no prefix beyond `this`.
    fn module_path_type_member(&mut self, qual: &Tree, id: SymbolId) -> Type {
        let plain = Type::TypeMember(id);
        if !self.st.is_deferred_type_member(id) || self.st.path_member_decl(id).is_some() {
            return plain;
        }
        let owner = self.st.get(id).owner;
        if owner.is_none() || !matches!(self.st.get(owner).kind, SymKind::Class) {
            return plain;
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
            return Type::TypeMember(self.st.path_member(&[o], id, &prefix));
        }
        plain
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
                let ty = self.st.expand_in_type(prefix, &ty);
                return self.rebind_outer_type_member(cls, SymbolId::NONE, ty);
            }
        }
        for m in found {
            let ty = match self.st.get(m).kind {
                SymKind::TypeMember => self.st.type_member_as_seen(m),
                SymKind::Class | SymKind::ModuleClass => self.projected_class_type(prefix, cls, m),
                _ => continue,
            };
            self.note_cto_ref(m, span);
            let ty = self.st.expand_in_type(prefix, &ty);
            return self.rebind_outer_type_member(cls, m, ty);
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
                let ty = self.st.expand_in_type(prefix, &ty);
                return self.rebind_outer_type_member(cls, SymbolId::NONE, ty);
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

    /// [`Self::rebind_outer_type_member`] for a name an `import p._` brought
    /// into scope, where the prefix is the import's qualifier rather than a
    /// written one.
    ///
    /// `trait Profile { val profile: BlockingJdbcProfile; import
    /// profile.blockingApi._; … BaseColumnType[java.sql.Timestamp] … }` is
    /// gitbucket's own shape: the name binds to `RelationalProfile#API`'s
    /// alias, and only the import says that the `C.this` in its right-hand
    /// side is *this* profile.
    fn rebind_imported_type_member(&mut self, ty: Type) -> Type {
        let Some((alias, _)) = self.outer_this_alias_target(SymbolId::NONE, &ty) else {
            return ty;
        };
        let owner = self.st.get(alias).owner;
        let Some(cls) = self.import_prefix_class_for(owner) else {
            return ty;
        };
        self.rebind_outer_type_member(cls, SymbolId::NONE, ty)
    }

    /// nsc's `Types.rebind`, for the one shape a pickle cannot record.
    ///
    /// A nested trait's alias may be written in terms of its *enclosing*
    /// class's `this`: slick's `trait RelationalProfile { self =>` declares
    /// `trait API { type BaseColumnType[T] = self.BaseColumnType[T] }`, whose
    /// right-hand side is the abstract `type BaseColumnType[T] <:
    /// ColumnType[T] with BaseTypedType[T]` of `RelationalTypesComponent`.
    /// `SigType` has no room for a `THIStpe` prefix, so the pickle records it
    /// beside the member (`binary_alias_prefixes`) and the converted
    /// right-hand side arrives as a bare deferred member -- of a class that
    /// the *use site's* enclosing instance may well fix.
    ///
    /// nsc never leaves such a reference abstract: every `TypeRef(pre, sym,
    /// args)` it builds goes through `rebind`, which replaces an overridable
    /// `sym` by `pre.nonPrivateMember(sym.name)`. `asSeenFrom` maps the
    /// alias's `C.this` to the outer instance of the selection's own prefix,
    /// so the overriding member is looked for in the enclosing classes of the
    /// prefix's class: `profile.api.BaseColumnType[Timestamp]` on a `profile:
    /// JdbcProfile` is `JdbcTypesComponent`'s `JdbcType[T] with
    /// BaseTypedType[T]`, which is what lets the implicit
    /// `timestampColumnType` fit gitbucket's `MappedColumnType.base[Date,
    /// Timestamp]`.
    ///
    /// Only a *deferred* right-hand side of an alias that carries a `C.this`
    /// prefix—or whose enclosing class can be recovered from the class
    /// hierarchy when scalac omitted that prefix—is rebound, and only to a
    /// definition the enclosing class really has: with nothing more derived
    /// in sight the abstract member stands, exactly as before.
    fn rebind_outer_type_member(&mut self, cls: SymbolId, m: SymbolId, seen: Type) -> Type {
        if !self.library_abi || cls.is_none() {
            return seen;
        }
        let Some((alias, d)) = self.outer_this_alias_target(m, &seen) else {
            return seen;
        };
        // The class whose `this` the alias's right-hand side was written
        // against, and the instance of it the prefix carries: an enclosing
        // class of the prefix's own class.
        let Some(this_class) = self
            .binary_alias_this_class(alias)
            .or_else(|| self.inferred_alias_this_class(alias, d))
        else {
            return seen;
        };
        self.rebind_deferred_in_outer(cls, d, this_class)
            .unwrap_or(seen)
    }

    /// The definition of the deferred type member `d` that the first enclosing
    /// class of `cls` deriving from `want` gives it, when there is one.
    ///
    /// `cls` is the class the *prefix* names, so its enclosing classes are the
    /// outer instances the prefix carries; `want` is the class whose `this`
    /// the reference was written against. Nothing is returned when the outer
    /// class leaves the member abstract too, or when the definition does not
    /// take the same number of parameters -- then the declaration stands.
    fn rebind_deferred_in_outer(
        &mut self,
        cls: SymbolId,
        d: SymbolId,
        want: SymbolId,
    ) -> Option<Type> {
        let name = self.st.get(d).name.clone();
        let arity = self.st.get(d).tparams.len();
        for outer in self.st.enclosing_classes(cls) {
            if outer == cls || !(outer == want || self.st.is_ancestor_of(want, outer)) {
                continue;
            }
            let found =
                self.pickle
                    .complete_type_member(&mut self.st, &mut self.binary, outer, &name);
            let ty = found?;
            let same_or_abstract = match &ty {
                Type::TypeMember(x) => *x == d || self.st.is_deferred_type_member(*x),
                _ => false,
            };
            let fits = match &ty {
                Type::TypeMember(x) => self.st.get(*x).tparams.len() == arity,
                _ => arity == 0,
            };
            return (!same_or_abstract && fits).then_some(ty);
        }
        None
    }

    /// The deferred type members of a receiver's *outer* class that the
    /// `import p._` the receiver was named through fixes.
    ///
    /// A jar method's signature may name an abstract type member of the class
    /// that *encloses* its own: slick's `MappedColumnTypeFactory` is an inner
    /// trait of `RelationalTypesComponent`, and its
    /// `base[T: ClassTag, U: BaseColumnType]` means
    /// `RelationalTypesComponent.this.BaseColumnType`. nsc reads that through
    /// the receiver's prefix -- `profile.MappedColumnTypeFactory`, whose outer
    /// is `profile.type` -- and `rebind`s it to the definition the profile
    /// has. Here the receiver is a bare class type, so the outer instance
    /// comes from the import the receiver was named through:
    /// `import profile.blockingApi._` says that the enclosing profile is
    /// `profile`, a `BlockingJdbcProfile`, whose `JdbcTypesComponent` fixes
    /// `BaseColumnType[T] = JdbcType[T] with BaseTypedType[T]`. That is what
    /// makes gitbucket's `MappedColumnType.base[java.util.Date,
    /// java.sql.Timestamp]` find `timestampColumnType`.
    ///
    /// Only a member of a class the receiver's class is *nested in* is
    /// rebound: one of the receiver's own is read through the receiver, which
    /// [`Self::warm_receiver_type_members`] and the as-seen-from substitution
    /// already do.
    pub(crate) fn receiver_outer_rebinds(
        &mut self,
        qual: &Tree,
        recv_ty: &Type,
        found: &[SymbolId],
    ) -> Vec<(SymbolId, Type)> {
        let mut out = self.receiver_result_prefix_rebinds(qual, recv_ty, found);
        if !self.library_abi || qual.sym.is_none() {
            return out;
        }
        let Some(recv_cls) = self.st.class_sym_of(recv_ty) else {
            return out;
        };
        let owner = self.st.get(qual.sym).owner;
        let Some(import_cls) = self.import_prefix_class_for(owner) else {
            return out;
        };
        let outers: Vec<SymbolId> = self
            .st
            .enclosing_classes(recv_cls)
            .into_iter()
            .skip(1)
            .collect();
        if outers.is_empty() {
            return out;
        }
        for &s in found {
            let ty = self.st.get(s).ty.clone();
            for d in self.st.type_members_in(&ty) {
                if out.iter().any(|(x, _)| *x == d) || !self.st.is_deferred_type_member(d) {
                    continue;
                }
                let o = self.st.get(d).owner;
                if o.is_none() {
                    continue;
                }
                let encloses = outers
                    .iter()
                    .any(|&e| e == o || self.st.is_ancestor_of(o, e));
                if !encloses {
                    continue;
                }
                if let Some(t) = self.rebind_deferred_in_outer(import_cls, d, o) {
                    out.push((d, t));
                }
            }
        }
        out
    }

    /// Rebind a pickled member result whose outer path was recorded beside
    /// the signature rather than in the converted type. Scala's pickle uses
    /// this for inherited API members such as `BasicProfile.API#Database`:
    /// the result is `BasicBackend.DatabaseFactory`, while
    /// `result_prefix = BasicProfile.this.backend` says that the type member
    /// belongs to the concrete profile's backend. The receiver's as-seen-from
    /// view carries that concrete profile as its `<prefix>` marker.
    ///
    /// This is deliberately limited to `SigType::Single` paths whose prefix
    /// is a declaration `this`. A `This` prefix on an inner class is already
    /// handled by `with_pickled_this_prefix`, and arbitrary singleton paths
    /// do not have a declaration-side outer instance to rebind.
    fn receiver_result_prefix_rebinds(
        &mut self,
        qual: &Tree,
        recv_ty: &Type,
        found: &[SymbolId],
    ) -> Vec<(SymbolId, Type)> {
        let view = crate::prefix::view_prefix(recv_ty).cloned();
        let outer_prefix = view.or_else(|| match &qual.kind {
            TreeKind::Select { qual, .. } => self.singleton_prefix_of(qual),
            _ => None,
        });
        if !self.library_abi {
            return Vec::new();
        }
        let Some(outer_prefix) = outer_prefix else {
            return Vec::new();
        };
        let outer_cls = match &outer_prefix {
            Type::SingleType { sym, .. }
                if !sym.is_none()
                    && matches!(self.st.get(*sym).kind, SymKind::Method | SymKind::Term)
                    && matches!(self.st.get(*sym).ty, Type::TypeMember(_)) =>
            {
                self.concrete_class_of_prefix(&outer_prefix)
            }
            _ => self.st.class_sym_of(&outer_prefix),
        };
        let Some(outer_cls) = outer_cls else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for &member in found {
            let Some(result_prefix) = self.pickle.result_prefix_for(member).cloned() else {
                continue;
            };
            let selected_ty = self.st.get(member).ty.clone();
            if let scala_rs_pickle::sym::SigType::This(_) = result_prefix {
                let mut rebound = false;
                for decl in self.st.type_members_in(&selected_ty) {
                    let name = self.st.get(decl).name.clone();
                    let seen = self
                        .pickle
                        .complete_type_member(&mut self.st, &mut self.binary, outer_cls, &name)
                        .or_else(|| {
                            Some(
                                self.st
                                    .expand_in_type(&outer_prefix, &Type::TypeMember(decl)),
                            )
                        });
                    let Some(seen) = seen else { continue };
                    if matches!(seen, Type::TypeMember(id) if id == decl)
                        || seen.is_no_type()
                        || seen.is_error()
                    {
                        continue;
                    }
                    if !out.iter().any(|(id, _)| *id == decl) {
                        out.push((decl, seen));
                        rebound = true;
                    }
                }
                if rebound {
                    continue;
                }
            }
            let scala_rs_pickle::sym::SigType::Single { prefix, sym } = result_prefix else {
                continue;
            };
            let scala_rs_pickle::sym::SigType::This(_) = prefix.as_ref() else {
                continue;
            };
            let Some(backend_name) = sym.rsplit('.').next() else {
                continue;
            };
            // The outer class may expose an overriding accessor directly or
            // inherit the profile's one. Complete its pickle first: a
            // classfile-only hierarchy can otherwise contain just
            // BasicBackend's abstract accessor even though JdbcProfile's
            // concrete override is available in the pickle. Then apply the
            // normal most-derived-member reduction instead of depending on
            // lookup_member's traversal order.
            self.pickle
                .complete(&mut self.st, &mut self.binary, outer_cls, backend_name);
            let backend_candidates: Vec<SymbolId> = self
                .st
                .lookup_member(outer_cls, backend_name)
                .into_iter()
                .filter(|&s| matches!(self.st.get(s).kind, SymKind::Method | SymKind::Term))
                .collect();
            let Some(backend_member) = self
                .drop_overridden_at(outer_cls, backend_candidates)
                .into_iter()
                .next()
            else {
                continue;
            };
            let backend_ty = self.st.singleton_underlying(backend_member);
            if backend_ty.is_no_type() || backend_ty.is_error() {
                continue;
            }
            // The concrete backend can be a class whose aliases have not
            // been demanded yet. Complete each referenced type member before
            // asking `expand_in_type`; otherwise `lookup_member` sees only
            // the abstract declaration inherited from BasicBackend.
            let mut completed_members = std::collections::HashMap::new();
            if let Some(backend_cls) = self.st.class_sym_of(&backend_ty) {
                for decl in self.st.type_members_in(&selected_ty) {
                    let name = self.st.get(decl).name.clone();
                    let completed = self.pickle.complete_type_member(
                        &mut self.st,
                        &mut self.binary,
                        backend_cls,
                        &name,
                    );
                    if let Some(t) = completed.clone() {
                        completed_members.insert(name.clone(), t);
                    }
                }
            }
            for decl in self.st.type_members_in(&selected_ty) {
                let name = self.st.get(decl).name.clone();
                let seen = completed_members.get(&name).cloned().unwrap_or_else(|| {
                    self.st.expand_in_type(&backend_ty, &Type::TypeMember(decl))
                });
                if matches!(seen, Type::TypeMember(id) if id == decl)
                    || seen.is_no_type()
                    || seen.is_error()
                {
                    continue;
                }
                if !out.iter().any(|(id, _)| *id == decl) {
                    crate::pickle_supply::trace(format_args!(
                        "{}: rebinding result member {} through {}",
                        self.st.get(member).name,
                        self.st.get(decl).name,
                        backend_name
                    ));
                    out.push((decl, seen));
                }
            }
        }
        out
    }

    /// Resolve the class of a stable path after reading inherited members in
    /// the current class.  A path such as `tdb.profile` stores the accessor's
    /// declaration type (`BasicProfile`) in its final symbol, while the
    /// enclosing receiver may provide a concrete `Profile = JdbcProfile`
    /// alias.  Re-read that accessor as seen from `this` before falling back
    /// to the declaration class; this keeps path-dependent inner members
    /// tied to the concrete enclosing instance without naming a library type.
    pub(crate) fn concrete_class_of_prefix(&self, prefix: &Type) -> Option<SymbolId> {
        if let Type::SingleType { sym, .. } = prefix {
            if !sym.is_none() {
                let seen = self.ident_ty_as_seen_from_this(*sym, self.st.get(*sym).ty.clone());
                let seen = match prefix {
                    Type::SingleType { prefix: parent, .. } => self
                        .concrete_class_of_prefix(parent)
                        .map(|cls| self.st.expand_type_members(cls, &seen))
                        .unwrap_or_else(|| self.st.expand_type_members(self.st.this_class, &seen)),
                    _ => self.st.expand_type_members(self.st.this_class, &seen),
                };
                if let Some(cls) = self.st.class_sym_of(&seen) {
                    return Some(cls);
                }
            }
        }
        self.st.class_sym_of(prefix)
    }

    /// The alias symbol behind `seen` and the deferred member its right-hand
    /// side names, when that member is not one the prefix's own class has.
    ///
    /// `m` is the member the selection resolved to, which is the alias itself
    /// for a nullary alias (whose right-hand side replaces it outright); for a
    /// parameterised one `seen` is the alias symbol and the right-hand side
    /// sits in its `ty`.
    fn outer_this_alias_target(&self, m: SymbolId, seen: &Type) -> Option<(SymbolId, SymbolId)> {
        let alias = match seen {
            Type::TypeMember(a) if !self.st.get(*a).tparams.is_empty() => *a,
            _ if self.st.get(m).kind == SymKind::TypeMember => m,
            _ => return None,
        };
        let rhs = if alias == m {
            seen.clone()
        } else {
            self.st.get(alias).ty.clone()
        };
        let core = match &rhs {
            Type::Applied { ctor, .. } => (**ctor).clone(),
            other => other.clone(),
        };
        let Type::TypeMember(d) = core else {
            return None;
        };
        if d == alias || !self.st.is_deferred_type_member(d) {
            return None;
        }
        Some((alias, d))
    }

    /// The class `C` of a `C.this.X` right-hand side the pickle recorded for
    /// `alias`, when `C` encloses the alias's own owner (which is what makes
    /// it an *outer* instance rather than the alias's own class).
    fn binary_alias_this_class(&self, alias: SymbolId) -> Option<SymbolId> {
        let owner = self.st.get(alias).owner;
        let name = self.st.get(alias).name.clone();
        let prefix = self.st.binary_alias_prefixes.get(&(owner, name))?;
        let scala_rs_pickle::sym::SigType::This(full) = prefix else {
            return None;
        };
        let internal = full.replace('.', "/");
        let this_class =
            self.st.enclosing_classes(owner).into_iter().find(|&c| {
                c != owner && self.st.get(c).jvm_name.trim_end_matches('$') == internal
            })?;
        Some(this_class)
    }

    /// Some scalac pickles omit the `This` prefix of a self-referential alias
    /// after reducing it to an inherited member. The alias owner still tells
    /// us which enclosing class supplied that self type: choose the nearest
    /// enclosing class that inherits the deferred member's owner. This is the
    /// same `asSeenFrom` relationship as the recorded prefix, recovered from
    /// the class hierarchy rather than from a library-specific name.
    fn inferred_alias_this_class(&self, alias: SymbolId, deferred: SymbolId) -> Option<SymbolId> {
        let owner = self.st.get(alias).owner;
        let target = self.st.get(deferred).owner;
        let enclosing = self.st.enclosing_classes(owner);
        if let Some(c) = enclosing
            .into_iter()
            .skip(1)
            .find(|&c| c == target || self.st.is_ancestor_of(target, c))
        {
            return Some(c);
        }
        // A companion object may own the nested class symbol when the
        // enclosing trait also has a same-named companion. Recover the
        // trait/interface by its JVM nesting prefix; unlike a name-specific
        // exception this is valid for any companion pair.
        let outer_jvm = self.st.get(owner).jvm_name.rsplit_once('$')?.0;
        let candidate = self.st.find_class_by_jvm(outer_jvm)?;
        // The index is class-like (it also contains module classes), while
        // this recovery specifically needs the enclosing class/interface.
        (self.st.get(candidate).kind == SymKind::Class
            && (candidate == target || self.st.is_ancestor_of(target, candidate)))
        .then_some(candidate)
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
                            // An inherited stable member is read through the
                            // current class, just like an unqualified
                            // expression in `bind_found`. This matters for
                            // `val profile: Profile` where the subclass
                            // narrows the inherited abstract `type Profile`:
                            // without this as-seen-from step, `profile.type`
                            // keeps the parent's `BasicProfile` bound and
                            // `profile.api.Table` loses the relational API.
                            let mut ty = self.ident_ty_as_seen_from_this(s, sy.ty.clone());
                            if matches!(
                                self.st.get(sy.owner).kind,
                                SymKind::Class | SymKind::ModuleClass | SymKind::Module
                            ) && !self.st.this_class.is_none()
                            {
                                ty = self.st.expand_type_members(self.st.this_class, &ty);
                            }
                            Some(self.maybe_auto_apply(ty, &Type::NoType))
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
                    // A *polymorphic* declaration (`def stepper[S <:
                    // Stepper[_]](implicit shape: StepperShape[A, S]): S with
                    // EfficientSplit`, which is how `StreamExtensions` spells
                    // the three refinements it uses as the right-hand side of a
                    // `<:<` evidence parameter). Its parameters are entered as
                    // real symbols for the duration of the signature, so the
                    // parameter and result types mention them, and they are
                    // carried on the declaration for
                    // `conforms_to_refinement` to alpha-rename.
                    let tparams = tparams.clone();
                    let tp_ids = if tparams.is_empty() {
                        Vec::new()
                    } else {
                        self.st.push_scope();
                        let mut ts = tparams;
                        self.enter_tparams(&mut ts, SymbolId::NONE)
                    };
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
                    if !tp_ids.is_empty() {
                        self.st.pop_scope();
                    }
                    decls.push(scala_rs_parser::RefineDecl::Def {
                        name: name.clone(),
                        tparams: tp_ids,
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
                            tparams: Vec::new(),
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
            self.collect_type_alias_captures(&rhs_ty, &mut free);
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

    /// Collect parameters an alias body needs from its enclosing scopes.
    ///
    /// `collect_tparams` intentionally treats a `TypeMember` as opaque, which
    /// is correct for an abstract member but loses captures when an alias is a
    /// refinement containing another alias. `OptionMapperDSL.arg[B1, P1]` is
    /// the important shape: its nested `arg[B2, P2]` member stores a body that
    /// still mentions the outer `B1` and `P1`. If those captures are not made
    /// explicit on the nested member, applying the outer alias leaves the
    /// original symbols in the method signature and they erase to `Object`.
    /// Walk nested alias bodies, hiding each member's own parameters while
    /// retaining parameters that were not captured by that member. Explicit
    /// arguments on an applied member are visited normally.
    fn collect_type_alias_captures(&self, ty: &Type, out: &mut Vec<SymbolId>) {
        fn visit(
            st: &SymbolTable,
            ty: &Type,
            bound: &[SymbolId],
            out: &mut Vec<SymbolId>,
            stack: &mut Vec<SymbolId>,
        ) {
            match ty {
                Type::TypeParam(id) => {
                    if !bound.contains(id) && !out.contains(id) {
                        out.push(*id);
                    }
                }
                Type::TypeMember(id) => {
                    let info = st.get(*id);
                    // Named aliases already run this capture pass when their
                    // own body is entered. The extra walk is for the symbols
                    // allocated for nested refinement members (which have no
                    // owner); following every named alias here would turn a
                    // standard-library alias graph into a quadratic walk.
                    if !info.is_type_alias || !info.owner.is_none() || stack.contains(id) {
                        return;
                    }
                    let mut nested_bound = bound.to_vec();
                    nested_bound.extend(info.tparams.iter().copied());
                    stack.push(*id);
                    visit(st, &info.ty, &nested_bound, out, stack);
                    stack.pop();
                }
                Type::Class { args, .. }
                | Type::Tuple(args)
                | Type::Named { args, .. }
                | Type::Overload(args) => {
                    for arg in args {
                        visit(st, arg, bound, out, stack);
                    }
                }
                Type::Applied { ctor, args } => {
                    visit(st, ctor, bound, out, stack);
                    for arg in args {
                        visit(st, arg, bound, out, stack);
                    }
                }
                Type::Array(t)
                | Type::ByName(t)
                | Type::Repeated(t)
                | Type::Annotated { tpe: t, .. } => visit(st, t, bound, out, stack),
                Type::Function { params, ret } => {
                    for param in params {
                        visit(st, param, bound, out, stack);
                    }
                    visit(st, ret, bound, out, stack);
                }
                Type::Method { paramss, ret } => {
                    for params in paramss {
                        for param in params {
                            visit(st, param, bound, out, stack);
                        }
                    }
                    visit(st, ret, bound, out, stack);
                }
                Type::Refined { parents, decls } => {
                    for parent in parents {
                        visit(st, parent, bound, out, stack);
                    }
                    for decl in decls {
                        match decl {
                            RefineDecl::Type { rhs, lo, hi, .. } => {
                                if let Some(rhs) = rhs {
                                    visit(st, rhs, bound, out, stack);
                                }
                                if let Some(lo) = lo {
                                    visit(st, lo, bound, out, stack);
                                }
                                if let Some(hi) = hi {
                                    visit(st, hi, bound, out, stack);
                                }
                            }
                            RefineDecl::Def {
                                tparams,
                                paramss,
                                ret,
                                ..
                            } => {
                                let mut method_bound = bound.to_vec();
                                method_bound.extend(tparams.iter().copied());
                                for params in paramss {
                                    for param in params {
                                        visit(st, param, &method_bound, out, stack);
                                    }
                                }
                                visit(st, ret, &method_bound, out, stack);
                            }
                            RefineDecl::Val { ty, .. } => visit(st, ty, bound, out, stack),
                        }
                    }
                }
                Type::SingleType { prefix, .. } => visit(st, prefix, bound, out, stack),
                _ => {}
            }
        }

        visit(&self.st, ty, &[], out, &mut Vec::new());
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
        // A package object's inherited alias may share a name with the JVM
        // mirror class of an object in that package (`object
        // NonEmptyLazyList` plus `type NonEmptyLazyList[A] = ...`). The
        // mirror is only the term-side fallback; in a type path the alias
        // must answer before that class. Ordinary class/companion pairs keep
        // the historical class-first ordering.
        let package_alias = found.iter().copied().find(|&s| {
            self.st.get(s).kind == SymKind::TypeMember
                && self.st.get(s).owner == owner
                && found.iter().any(|&c| {
                    self.st.get(c).kind == SymKind::Class && self.st.companion_module(c).is_some()
                })
        });
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
        if let Some(alias) = package_alias {
            out.retain(|&s| s != alias);
            out.insert(0, alias);
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
}

impl Typer {
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
                // Prefer a package alias over the JVM mirror class of an
                // object with the same name. Cats' Newtype encoding has
                // `object NonEmptyLazyList` beside inherited `type
                // NonEmptyLazyList[A]`; the mirror is only a term fallback.
                // Ordinary class/companion pairs retain class-first lookup.
                let mirror_alias = found.iter().copied().find(|&alias| {
                    self.st.get(alias).kind == SymKind::TypeMember
                        && self.st.get(alias).owner != SymbolId::NONE
                        && self.st.get(self.st.get(alias).owner).kind == SymKind::Package
                        && found.iter().any(|&class| {
                            self.st.get(class).kind == SymKind::Class
                                && self.st.companion_module(class).is_some()
                        })
                });
                let id = mirror_alias
                    .or_else(|| {
                        found
                            .iter()
                            .copied()
                            .find(|s| matches!(self.st.get(*s).kind, SymKind::Class))
                    })
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
