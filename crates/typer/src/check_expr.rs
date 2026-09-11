#![allow(dead_code)]
//! The expression typer's entry point and its main dispatcher, plus the
//! `reify` / quasiquote expansion that runs ahead of it.
//!
//! `type_expr` is where every expression enters; it first offers the tree to
//! the quasiquote and `reify` machinery (which builds `scala.reflect` trees
//! out of the source text) and otherwise falls through to `type_expr_inner`,
//! the large match over tree kinds that types literals, blocks, ifs, tries,
//! assignments, `new`, lambdas and the rest.

use crate::check::*;
use crate::symbol::{SymKind, SymbolTable};
use crate::uncurry::is_eta_marker;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;
use std::collections::{HashMap, HashSet};

impl Typer {
    pub(crate) fn type_qualifier(&mut self, tree: &mut Tree, pt: &Type) {
        let saved = std::mem::replace(&mut self.typing_qualifier, true);
        self.type_expr(tree, pt);
        self.typing_qualifier = saved;
    }

    pub(crate) fn type_expr_arg_prototype(
        &mut self,
        tree: &mut Tree,
        pt: &Type,
        provisional: bool,
    ) {
        if !provisional {
            self.type_expr(tree, pt);
            return;
        }
        fn value_sites(
            tree: &Tree,
            file: usize,
            out: &mut std::collections::HashSet<(usize, NodeId)>,
        ) {
            out.insert((file, tree.id));
            match &tree.kind {
                TreeKind::Block { expr, .. } | TreeKind::Return { expr } => {
                    value_sites(expr, file, out)
                }
                TreeKind::If { thenp, elsep, .. } => {
                    value_sites(thenp, file, out);
                    value_sites(elsep, file, out);
                }
                TreeKind::Match { cases, .. } => {
                    for c in cases {
                        value_sites(&c.body, file, out);
                    }
                }
                TreeKind::Try { block, catches, .. } => {
                    value_sites(block, file, out);
                    for c in catches {
                        value_sites(&c.body, file, out);
                    }
                }
                TreeKind::Function { body, .. } => value_sites(body, file, out),
                // A written ascription is an actual constraint. Likewise the
                // arguments of a nested call obey that callee's declaration.
                _ => {}
            }
        }
        let saved = self.provisional_arg_sites.clone();
        value_sites(tree, self.file_index, &mut self.provisional_arg_sites);
        self.type_expr(tree, pt);
        self.provisional_arg_sites = saved;
    }

    pub(crate) fn type_expr(&mut self, tree: &mut Tree, pt: &Type) {
        // Taken, not read: everything typed below this point is no longer the
        // callee of the application that set it. See `typing_callee`.
        let callee = std::mem::take(&mut self.typing_callee);
        let type_callee = std::mem::take(&mut self.typing_type_callee);
        let qualifier = std::mem::take(&mut self.typing_qualifier);
        // Generated thunk bodies were typed in the argument's lexical scope.
        // Reusing one for defaults must not reinterpret it as a source lambda.
        if tree.byname_thunk {
            return;
        }
        if tree.id.is_pretyped_default() {
            // A default argument's body, already typed in the scope it was
            // written in (`type_default_rhs_here`). Typing it again here would
            // resolve its names in the caller's scope -- which is the bug that
            // typing it there fixes -- so only fit it to the expectation.
            if !pt.is_no_type() && !tree.ty.is_no_type() && !tree.ty.is_error() {
                self.adapt(tree, pt);
            }
            return;
        }
        if matches!(&tree.kind, TreeKind::Ident { .. }) {
            let name = match &tree.kind {
                TreeKind::Ident { name } => name.clone(),
                _ => unreachable!(),
            };
            self.type_ident(tree, name, pt);
            self.spell_out_inferred_class_of(tree);
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
            self.type_eta(tree, pt);
        } else {
            self.type_expr_inner(tree, pt);
        }
        // nsc expands a macro application in the typer, at the outermost
        // `Apply`/`TypeApply`, and typechecks what comes back at the call
        // site. Before `adapt`, so what is adapted to `pt` is the expansion.
        // `has_macro_defs` is set by a macro def this run *compiles*;
        // `supplied_macro_def` by one read from a jar's pickle, which is how
        // slick's `TableQuery.apply[E]` reaches a program that only calls it.
        if !callee && (self.has_macro_defs || self.pickle.supplied_macro_def) {
            // A `Type::Method` expectation here means the *method value* is
            // wanted, not its result: `Macros.foo _`, which `type_eta` types
            // with exactly that expectation. nsc rejects it -- "macros cannot
            // be eta-expanded" -- because there is nothing to take a reference
            // to: a macro def has no bytecode. The other places that pass a
            // method expectation set `callee` or throw their diagnostics away.
            if matches!(pt, Type::Method { .. }) {
                self.reject_macro_eta(tree);
            } else {
                self.expand_macro_application(tree);
            }
        }
        if !type_callee {
            self.adapt_implicit_apply_in(tree, pt, callee);
        }
        // A parameterless method in value position has no later argument
        // clause to solve a real lower bound (`xs.toSet[B >: A]`). Keep
        // callee references open for explicit type arguments or an inserted
        // apply on the returned value.
        if !callee
            && !qualifier
            && !tree.sym.is_none()
            && self.is_nullary_method_sym(tree.sym)
            && !matches!(tree.kind, TreeKind::TypeApply { .. })
            && !matches!(tree.ty, Type::Method { .. } | Type::Overload(_))
        {
            let tps = self.st.get(tree.sym).tparams.clone();
            self.pin_lower_bounded_implicit_tparams(tree, &tps);
        }
        if !pt.is_no_type() && !tree.ty.is_no_type() && !tree.ty.is_error() {
            self.adapt(tree, pt);
        }
    }

    /// Reify a quasiquote in place: rewrite it into the universe calls that
    /// build the reflect `Tree`, and type the result.
    ///
    /// Returns false when this quasiquote is not one this compiler builds --
    /// no universe in scope, a body that did not parse, or a form
    /// `crates/typer/src/reify.rs` does not cover. The caller then reports it;
    /// nothing is ever silently accepted.
    fn reify_quasiquote(
        &mut self,
        tree: &mut Tree,
        span: Span,
        kind: crate::quasiquote::QuasiKind,
        parts: &[String],
        args: &[Tree],
        pt: &Type,
    ) -> bool {
        let Some(universe) = self.universe_in_scope() else {
            return false;
        };
        let Ok((body, src)) = crate::quasiquote::parse_body(kind, parts, args.len()) else {
            return false;
        };
        let ranks = crate::quasiquote::hole_ranks(parts, args.len());
        let lifts = self.hole_lifts(args, &ranks);
        let built = {
            let r = crate::reify::Reifier::new(universe, args, &ranks, &lifts, span, &src);
            match r.reify(kind, &body) {
                Ok(t) => t,
                Err(why) => {
                    self.error(
                        span,
                        format!(
                            "unimplemented syntax: quasiquote {}\"...\" ({why}). \
                             See docs/macros.md \u{a7}7.",
                            kind.prefix()
                        ),
                    );
                    tree.ty = Type::Error;
                    return true;
                }
            }
        };
        // Typed like any other expression. A failure here is a real one --
        // a hole whose argument is not a `Tree`, say -- so its diagnostics are
        // the ones to keep.
        *tree = built;
        self.type_expr(tree, pt);
        true
    }

    /// The universe a `reify { … }` application belongs to, if `tree` is one.
    ///
    /// `reify` is declared on `scala.reflect.api.Universe` and has no
    /// implementation to call (`Self::report_internal_universe_macro`), so an
    /// application of it is recognised the same way that diagnostic
    /// recognises the name: written on a universe, or written bare with a
    /// universe in scope and no other `reify` bound. A program with its own
    /// `reify` in scope keeps it.
    fn reify_universe(&mut self, tree: &Tree) -> Option<Tree> {
        if !self.library_abi {
            return None;
        }
        let TreeKind::Apply { fun, args } = &tree.kind else {
            return None;
        };
        if args.len() != 1 {
            return None;
        }
        match &fun.kind {
            TreeKind::Ident { name } if name == "reify" => {
                if !self.st.lookup("reify").is_empty() {
                    return None;
                }
                self.universe_in_scope()
            }
            TreeKind::Select { qual, name } if name == "reify" => {
                let mark = self.diags.len();
                let mut probe = (**qual).clone();
                self.type_expr(&mut probe, &Type::NoType);
                self.diags.truncate(mark);
                let owner = self.st.class_sym_of(&probe.ty).unwrap_or(SymbolId::NONE);
                // `scala.reflect.macros.Universe extends
                // scala.reflect.api.Universe` only in the pickle, and until
                // that parent is attached `c.universe` is not recognisable as
                // a universe at all -- the same reading
                // `remember_term_import_prefix` has to force for `import
                // c.universe._`.
                if !owner.is_none() {
                    self.pickle
                        .ensure_parents(&mut self.st, &mut self.binary, owner);
                }
                self.is_reflect_universe(owner).then_some(probe)
            }
            _ => None,
        }
    }

    /// Expand `reify { … }` in place (`docs/macros.md` §7.14,
    /// `crate::reify_expand`, `crate::reify::tree`).
    ///
    /// Returns false only when this is not a `reify` application at all. A
    /// body reify cannot build is an *error* here, never a silent pass: an
    /// expansion that reified a local as the bare name it was written with
    /// would compile, run, and mean whatever stood at the call site.
    ///
    /// The body is typed once, on a clone, and it is that **typed** clone the
    /// reifier walks: every reference is rebuilt from the symbol it resolved
    /// to, which is nsc's own rule (`docs/notes/reify-design.md`).
    pub(crate) fn try_expand_reify(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let Some(universe) = self.reify_universe(tree) else {
            return false;
        };
        let span = tree.span;
        // A `reify` nested in the body of another: it is typed here, so the
        // outer reifier can read its body's symbols, but *not* expanded --
        // the outer expansion reifies it as the call it is, and the toolbox
        // that compiles that tree expands it in turn.
        if self.reify_depth > 0 {
            let Some(expr_cls) = self.reflect_class(
                "scala.reflect.api.Exprs.Expr",
                "scala/reflect/api/Exprs$Expr",
            ) else {
                return false;
            };
            let TreeKind::Apply { args, .. } = &mut tree.kind else {
                return false;
            };
            if args.len() != 1 {
                return false;
            }
            self.reify_depth += 1;
            self.type_expr(&mut args[0], &Type::NoType);
            self.reify_depth -= 1;
            let arg = match &args[0].ty {
                Type::Constant(l) => Type::lit_underlying(l),
                t => t.clone(),
            };
            tree.ty = Type::Class {
                sym: expr_cls,
                args: vec![arg],
            };
            self.nested_reifies.insert(tree.id, universe);
            return true;
        }
        let TreeKind::Apply { args, .. } = &tree.kind else {
            return false;
        };
        let body = args[0].clone();

        // `T` of the resulting `Expr[T]`: what the body means in the macro
        // implementation's own scope. Typed on a clone -- the shape
        // `Self::hole_lifts` uses -- so the tree the call site keeps is typed
        // once, as part of the expansion. A body that does not typecheck is
        // reported from here, with the probe's own diagnostics.
        let mark = self.diags.len();
        let mut probe = body.clone();
        self.reify_depth += 1;
        self.type_expr(&mut probe, &Type::NoType);
        self.reify_depth -= 1;
        let nested = std::mem::take(&mut self.nested_reifies);
        if self.diags[mark..]
            .iter()
            .any(|d| d.level == scala_rs_span::Level::Error)
        {
            tree.ty = Type::Error;
            return true;
        }
        self.diags.truncate(mark);
        let arg = match &probe.ty {
            Type::Constant(l) => Type::lit_underlying(l),
            t => t.clone(),
        };
        if arg.is_no_type() || arg.is_error() {
            self.error(
                span,
                "cannot expand reify { ... }: the type of the expression is not known here"
                    .to_string(),
            );
            tree.ty = Type::Error;
            return true;
        }

        let Some(expr_cls) = self.reflect_class(
            "scala.reflect.api.Exprs.Expr",
            "scala/reflect/api/Exprs$Expr",
        ) else {
            return false;
        };
        self.gensym += 1;
        let n = self.gensym;
        let (universe_local, mirror_local) = (format!("$u${n}"), format!("$m${n}"));
        let src = self
            .sources
            .get(self.file_index)
            .cloned()
            .unwrap_or_else(|| std::rc::Rc::from(""));
        let mut facts = self.reify_facts(&body, &probe, &arg, span);
        let strong_tags = self.strong_reify_tags(&facts.tags);
        let tag_bindings = self.bind_reify_tags(&mut facts.tags, span);
        let built = {
            let env = crate::reify::ReifyEnv {
                st: &self.st,
                local_syms: facts.local_syms,
                local_type_names: facts.local_type_names,
                this_classes: facts.this_classes,
                tags: facts.tags,
                strong_tags,
                types: facts.types,
                nested,
                splices: facts.splices,
                def_spans: self.def_spans.clone(),
                file_name: facts.file_name,
                expr_class: expr_cls,
            };
            let universe_ident = Tree::new(
                NodeId(0),
                span,
                TreeKind::Ident {
                    name: universe_local.clone(),
                },
            );
            let r = crate::reify::Reifier::new(universe_ident, &[], &[], &[], span, &src).in_reify(
                crate::reify::ReifyCtx {
                    env,
                    mirror_local: mirror_local.clone(),
                    universe_local: universe_local.clone(),
                },
            );
            match r.reify(crate::quasiquote::QuasiKind::Term, &probe) {
                Ok(t) => {
                    // The tag of the expression: materialised by implicit
                    // search unless the type mentions a free type, in which
                    // case only a creator sharing this body's symbol table
                    // can build it.
                    if r.needs_free_types(&arg) {
                        match r.standalone_type_value(&arg) {
                            Ok(v) => Ok((t, Some(v))),
                            Err(why) => Err(format!(
                                "the type of the expression cannot be rebuilt: {why}"
                            )),
                        }
                    } else {
                        Ok((t, None))
                    }
                }
                Err(why) => Err(why),
            }
        };
        let (built, tag) = match built {
            Ok(x) => x,
            Err(why) => {
                self.report_reify_gap(span, &why);
                tree.ty = Type::Error;
                return true;
            }
        };

        let Some(mirror) =
            self.reflect_class("scala.reflect.api.Mirror", "scala/reflect/api/Mirror")
        else {
            return false;
        };
        let Some(tree_api) = self.reflect_class(
            "scala.reflect.api.Trees.TreeApi",
            "scala/reflect/api/Trees$TreeApi",
        ) else {
            return false;
        };
        let Some(type_api) = self.reflect_class(
            "scala.reflect.api.Types.TypeApi",
            "scala/reflect/api/Types$TypeApi",
        ) else {
            return false;
        };
        // `Expr` is a nested `object` of the universe, supplied on demand
        // (`PickleSupply::install_nested_module`); nothing has asked for it
        // on this receiver yet.
        let universe_ty = universe.ty.clone();
        let _ = self.supply_from_pickle(&universe_ty, "Expr");
        if tag.is_some() && !self.ensure_weak_tag_module(&universe) {
            return false;
        }
        let mut built = crate::reify_expand::ReifyExpander {
            universe: &universe,
            creator_name: format!("$treecreator{n}"),
            body: built,
            arg,
            mirror_ty: Type::Class {
                sym: mirror,
                args: vec![],
            },
            tree_api: Type::Class {
                sym: tree_api,
                args: vec![],
            },
            type_api: Type::Class {
                sym: type_api,
                args: vec![],
            },
            universe_local,
            mirror_local,
            tag,
            tag_creator_name: format!("$typecreator{n}"),
            tag_bindings,
            span,
        }
        .build();
        // The `WeakTypeTag[T]` of `Expr.apply` is *materialised*, and
        // `Check::materialize_tag` needs a universe to build it in -- which it
        // reads off `import <universe>._`. `c.universe.reify { … }` brings no
        // such import, so the universe this expansion was written against is
        // offered for as long as it is being typed. Restored after: it is this
        // expansion's, not the enclosing scope's.
        //
        // Pushed unconditionally rather than only when no equal prefix is
        // there: `term_import_prefixes` is kept for the whole run, so an
        // `import c.universe._` in an *earlier* method leaves an entry that
        // spells the same path and is no longer in scope (its `c` is that
        // method's parameter). `universe_in_scope` would find that one and
        // reject it, and never reach this one.
        let owner = self.st.class_sym_of(&universe.ty).unwrap_or(SymbolId::NONE);
        let pushed = !owner.is_none();
        if pushed {
            self.term_import_prefixes.push((owner, universe.clone()));
        }
        self.type_expr(&mut built, pt);
        if pushed {
            self.term_import_prefixes.pop();
        }
        *tree = built;
        true
    }

    /// A type on its own, reified the way a `reify { … }` body's types are
    /// (`crate::reify::tree`): the value of `arg` as a `$u.Type`, with the
    /// free symbols it needs bound in front, against fresh `$u` / `$m`
    /// locals whose names are returned with it. `None` when the reifier
    /// cannot build it -- or, for a `TypeTag`, when it would need a free
    /// type, which nsc refuses too ("No TypeTag available"). The last
    /// element says whether the reification is concrete.
    pub(crate) fn reify_type_standalone(
        &mut self,
        universe: &Tree,
        arg: &Type,
        span: Span,
        tag: crate::materialize::Tag,
    ) -> Option<StandaloneType> {
        if !self.library_abi {
            return None;
        }
        let expr_cls = self.reflect_class(
            "scala.reflect.api.Exprs.Expr",
            "scala/reflect/api/Exprs$Expr",
        )?;
        self.gensym += 1;
        let n = self.gensym;
        let (universe_local, mirror_local) = (format!("$u${n}"), format!("$m${n}"));
        let empty = Tree::new(NodeId(0), span, TreeKind::Empty);
        let mut facts = self.reify_facts(&empty, &empty, arg, span);
        let strong_tags = self.strong_reify_tags(&facts.tags);
        let tag_bindings = self.bind_reify_tags(&mut facts.tags, span);
        let src = self
            .sources
            .get(self.file_index)
            .cloned()
            .unwrap_or_else(|| std::rc::Rc::from(""));
        let env = crate::reify::ReifyEnv {
            st: &self.st,
            local_syms: facts.local_syms,
            local_type_names: facts.local_type_names,
            this_classes: facts.this_classes,
            tags: facts.tags,
            strong_tags,
            types: facts.types,
            nested: HashMap::new(),
            splices: HashMap::new(),
            def_spans: self.def_spans.clone(),
            file_name: facts.file_name,
            expr_class: expr_cls,
        };
        let universe_ident = Tree::new(
            NodeId(0),
            span,
            TreeKind::Ident {
                name: universe_local.clone(),
            },
        );
        let r = crate::reify::Reifier::new(universe_ident, &[], &[], &[], span, &src).in_reify(
            crate::reify::ReifyCtx {
                env,
                mirror_local: mirror_local.clone(),
                universe_local: universe_local.clone(),
            },
        );
        if tag == crate::materialize::Tag::Strong && r.needs_free_types(arg) {
            return None;
        }
        let concrete = r.is_concrete(arg);
        let tree = r.standalone_type_value(arg).ok()?;
        let _ = universe;
        Some((tree, universe_local, mirror_local, concrete, tag_bindings))
    }

    /// The tags among `tags` that are `TypeTag`s, by the type of the
    /// expression each is.
    fn strong_reify_tags(&self, tags: &HashMap<SymbolId, Tree>) -> HashSet<SymbolId> {
        tags.iter()
            .filter(|(_, t)| match &t.ty {
                Type::Class { sym, .. } => {
                    self.st.jvm_internal(*sym) == crate::materialize::Tag::Strong.jvm()
                }
                _ => false,
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Bind each tag in scope to a local of the expansion (`val $tag1 =
    /// evidence$1`) and refer to the local from the creator. A creator is a
    /// local class; a class parameter's field -- `class C[T: TypeTag]` keeps
    /// its evidence in a private field -- is not readable from there, a
    /// local is.
    fn bind_reify_tags(
        &mut self,
        tags: &mut HashMap<SymbolId, Tree>,
        span: Span,
    ) -> Vec<(String, Tree)> {
        let mut out = Vec::new();
        let mut ids: Vec<SymbolId> = tags.keys().copied().collect();
        ids.sort_by_key(|s| s.0);
        for id in ids {
            self.gensym += 1;
            let name = format!("$tag{}", self.gensym);
            // Untyped, so the expansion's typing resolves it to the `val`.
            let local = Tree::new(NodeId(0), span, TreeKind::Ident { name: name.clone() });
            let tree = tags.remove(&id).expect("key from keys");
            out.push((name, tree));
            tags.insert(id, local);
        }
        out
    }

    /// Make `<universe>.WeakTypeTag.apply` callable, the way
    /// `Self::materialize_tag` does for a tag it builds itself.
    fn ensure_weak_tag_module(&mut self, universe: &Tree) -> bool {
        let tag = crate::materialize::Tag::Weak;
        let Some(tag_cls) = self.reflect_class(tag.pickle_name(), tag.jvm()) else {
            return false;
        };
        let Some(type_tags) =
            self.reflect_class("scala.reflect.api.TypeTags", "scala/reflect/api/TypeTags")
        else {
            return false;
        };
        let Some(mirror) =
            self.reflect_class("scala.reflect.api.Mirror", "scala/reflect/api/Mirror")
        else {
            return false;
        };
        let Some(creator) = self.reflect_class(
            "scala.reflect.api.TypeCreator",
            "scala/reflect/api/TypeCreator",
        ) else {
            return false;
        };
        let classes = crate::materialize::TagClasses {
            tag_cls,
            type_tags,
            mirror,
            creator,
        };
        if crate::materialize::ensure_tag_module(&mut self.st, tag, classes).is_none() {
            return false;
        }
        let universe_ty = universe.ty.clone();
        let _ = self.supply_from_pickle(&universe_ty, tag.simple());
        true
    }

    /// A form `reify` does not build. Named, with the reason, and pointed at
    /// the design note -- never accepted.
    fn report_reify_gap(&mut self, span: Span, why: &str) {
        self.error(
            span,
            format!(
                "cannot expand reify {{ ... }}: {why}. scala-rs reifies the typed \
                 body by symbol -- static objects and their members, members of \
                 the enclosing object, locals and parameters as free terms, \
                 definitions inside the body by name, type arguments from a tag \
                 in scope or as free types; see docs/notes/reify-design.md."
            ),
        );
    }

    /// Everything the reifier needs to know about a body that only the typer
    /// can answer, gathered while `self` is still mutably available: the
    /// symbols the body defines, the classes enclosing it, the type each
    /// written type resolved to, and the tags in scope for the abstract
    /// types it mentions.
    fn reify_facts(&mut self, body: &Tree, probe: &Tree, arg: &Type, span: Span) -> ReifyFacts {
        let file_name = self
            .source_paths
            .get(self.file_index)
            .map(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.clone())
            })
            .unwrap_or_default();
        let this_classes = self
            .st
            .enclosing_classes(self.st.owner)
            .into_iter()
            .filter(|c| self.st.get(*c).is_class_like())
            .collect();
        let mut facts = ReifyFacts {
            file_name,
            this_classes,
            ..ReifyFacts::default()
        };
        collect_reify_locals(probe, &mut facts.local_syms, &mut facts.local_type_names);
        // A local class's synthetic companion, and a local object's module
        // class, are local with it: neither has a definition in the tree.
        for sym in facts.local_syms.clone() {
            match self.st.get(sym).kind {
                SymKind::Class => {
                    if let Some(m) = self.st.companion_module(sym) {
                        facts.local_syms.insert(m);
                        facts.local_syms.insert(self.st.module_class_of(m));
                    }
                }
                SymKind::Module => {
                    facts.local_syms.insert(self.st.module_class_of(sym));
                }
                _ => {}
            }
        }
        collect_reify_splices(body, &mut facts.splices);
        self.collect_reify_types(probe, &facts.local_type_names.clone(), &mut facts.types);
        // Every abstract type the body mentions, in a node's type, a written
        // type, or the type of a value bound outside the body.
        let mut abstract_ids = Vec::new();
        crate::reify::collect_abstract(arg, &mut abstract_ids);
        for ty in facts.types.values() {
            crate::reify::collect_abstract(ty, &mut abstract_ids);
        }
        collect_reify_abstract_in_tree(&self.st, probe, &mut abstract_ids);
        abstract_ids.sort_by_key(|s| s.0);
        abstract_ids.dedup();
        for id in abstract_ids {
            if facts.local_syms.contains(&id) {
                continue;
            }
            if let Some(tag) = self.reify_tag_in_scope(id, span) {
                facts.tags.insert(id, tag);
            }
        }
        facts
    }

    /// The `WeakTypeTag[T]` in scope for the abstract type `id`, if any --
    /// found by ordinary implicit search, as `Self::tag_body` finds it.
    fn reify_tag_in_scope(&mut self, id: SymbolId, span: Span) -> Option<Tree> {
        let flat = match self.st.get(id).kind {
            SymKind::TypeParam => Type::TypeParam(id),
            _ => Type::TypeMember(id),
        };
        // A `TypeTag` is a `WeakTypeTag`, but only once its parent -- which
        // lives in the pickle of `TypeTags` -- has been read; asking for the
        // stronger tag first needs no such knowledge.
        for tag in [
            crate::materialize::Tag::Strong,
            crate::materialize::Tag::Weak,
        ] {
            let tag_cls = self.reflect_class(tag.pickle_name(), tag.jvm())?;
            self.pickle
                .ensure_parents(&mut self.st, &mut self.binary, tag_cls);
            let want = Type::Class {
                sym: tag_cls,
                args: vec![flat.clone()],
            };
            let mark = self.diags.len();
            self.warm_implicit_scope(&want);
            let found = match self.search_implicit(&want) {
                crate::implicits::ImplicitSearch::Found(sid) => {
                    Some(self.implicit_tree(sid, &want, span, 0))
                }
                _ => None,
            };
            self.diags.truncate(mark);
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// The type each written type tree of the body resolved to, keyed by the
    /// tree's node. Where the typer left the type on the enclosing node
    /// (`Typed`, `New`, a parameter) it is read from there; elsewhere the
    /// type tree is resolved again here, in the scope the `reify` was
    /// written in -- unless it names a type the body itself defines, which
    /// only the body's own scope could resolve and which the reifier builds
    /// by name anyway.
    fn collect_reify_types(
        &mut self,
        t: &Tree,
        local_type_names: &HashSet<String>,
        out: &mut HashMap<NodeId, Type>,
    ) {
        let usable = |ty: &Type| !ty.is_no_type() && !ty.is_error();
        let record = |out: &mut HashMap<NodeId, Type>, tpt: &Tree, ty: &Type| {
            if !tpt.is_empty() && tpt.id != NodeId(0) && usable(ty) {
                out.insert(tpt.id, ty.clone());
            }
        };
        match &t.kind {
            TreeKind::ValDef { tpt, rhs, .. } => {
                if !tpt.is_empty() && !mentions_name(tpt, local_type_names) {
                    let ty = if usable(&t.ty) {
                        t.ty.clone()
                    } else {
                        self.reify_tree_to_type(tpt)
                    };
                    record(out, tpt, &ty);
                }
                self.collect_reify_types(rhs, local_type_names, out);
            }
            TreeKind::DefDef {
                tparams,
                vparamss,
                tpt,
                rhs,
                ..
            } => {
                for tp in tparams {
                    self.collect_reify_tparam_bounds(tp, local_type_names, out);
                }
                for p in vparamss.iter().flatten() {
                    self.collect_reify_types(p, local_type_names, out);
                }
                if !tpt.is_empty() && !mentions_name(tpt, local_type_names) {
                    let ty = self.reify_tree_to_type(tpt);
                    record(out, tpt, &ty);
                }
                self.collect_reify_types(rhs, local_type_names, out);
            }
            TreeKind::TypeDef {
                tparams,
                rhs,
                lo,
                hi,
                ..
            } => {
                for tp in tparams {
                    self.collect_reify_tparam_bounds(tp, local_type_names, out);
                }
                for b in [Some(rhs), lo.as_ref(), hi.as_ref()].into_iter().flatten() {
                    if !b.is_empty() && !mentions_name(b, local_type_names) {
                        let ty = self.reify_tree_to_type(b);
                        record(out, b, &ty);
                    }
                }
            }
            TreeKind::ClassDef {
                tparams,
                vparamss,
                impl_,
                ..
            } => {
                for tp in tparams {
                    self.collect_reify_tparam_bounds(tp, local_type_names, out);
                }
                for p in vparamss.iter().flatten() {
                    self.collect_reify_types(p, local_type_names, out);
                }
                self.collect_reify_template(impl_, local_type_names, out);
            }
            TreeKind::ModuleDef { impl_, .. } => {
                self.collect_reify_template(impl_, local_type_names, out);
            }
            TreeKind::Function { vparams, body } => {
                for p in vparams {
                    self.collect_reify_types(p, local_type_names, out);
                }
                self.collect_reify_types(body, local_type_names, out);
            }
            TreeKind::Typed { expr, tpt } => {
                if !mentions_name(tpt, local_type_names) {
                    let ty = if usable(&t.ty) {
                        t.ty.clone()
                    } else {
                        self.reify_tree_to_type(tpt)
                    };
                    record(out, tpt, &ty);
                }
                self.collect_reify_types(expr, local_type_names, out);
            }
            TreeKind::New { tpt } => {
                let mut head = tpt.as_ref();
                while let TreeKind::Apply { fun, args } = &head.kind {
                    for a in args {
                        self.collect_reify_types(a, local_type_names, out);
                    }
                    head = fun;
                }
                if let TreeKind::ClassDef { .. } = &head.kind {
                    self.collect_reify_types(head, local_type_names, out);
                } else if !mentions_name(head, local_type_names) {
                    let ty = if usable(&t.ty) {
                        t.ty.clone()
                    } else {
                        self.reify_tree_to_type(head)
                    };
                    record(out, head, &ty);
                }
            }
            TreeKind::TypeApply { fun, args } => {
                self.collect_reify_types(fun, local_type_names, out);
                for a in args {
                    if !mentions_name(a, local_type_names) {
                        let ty = self.reify_tree_to_type(a);
                        record(out, a, &ty);
                    }
                }
            }
            TreeKind::Match { selector, cases } => {
                self.collect_reify_types(selector, local_type_names, out);
                for c in cases {
                    self.collect_reify_case(c, local_type_names, out);
                }
            }
            TreeKind::Try {
                block,
                catches,
                finalizer,
            } => {
                self.collect_reify_types(block, local_type_names, out);
                for c in catches {
                    self.collect_reify_case(c, local_type_names, out);
                }
                self.collect_reify_types(finalizer, local_type_names, out);
            }
            TreeKind::Bind { body, .. } => self.collect_reify_types(body, local_type_names, out),
            TreeKind::Apply { fun, args } | TreeKind::UnApply { fun, args } => {
                self.collect_reify_types(fun, local_type_names, out);
                for a in args {
                    self.collect_reify_types(a, local_type_names, out);
                }
            }
            TreeKind::Select { qual, .. } => self.collect_reify_types(qual, local_type_names, out),
            TreeKind::Block { stats, expr } => {
                for s in stats {
                    self.collect_reify_types(s, local_type_names, out);
                }
                self.collect_reify_types(expr, local_type_names, out);
            }
            TreeKind::If { cond, thenp, elsep } => {
                self.collect_reify_types(cond, local_type_names, out);
                self.collect_reify_types(thenp, local_type_names, out);
                self.collect_reify_types(elsep, local_type_names, out);
            }
            TreeKind::Assign { lhs, rhs } => {
                self.collect_reify_types(lhs, local_type_names, out);
                self.collect_reify_types(rhs, local_type_names, out);
            }
            TreeKind::While { cond, body } | TreeKind::DoWhile { body, cond } => {
                self.collect_reify_types(cond, local_type_names, out);
                self.collect_reify_types(body, local_type_names, out);
            }
            TreeKind::Return { expr } | TreeKind::Throw { expr } => {
                self.collect_reify_types(expr, local_type_names, out)
            }
            TreeKind::Alternative { trees } => {
                for a in trees {
                    self.collect_reify_types(a, local_type_names, out);
                }
            }
            TreeKind::Star { elem } => self.collect_reify_types(elem, local_type_names, out),
            TreeKind::InterpolatedString { args, .. } => {
                for a in args {
                    self.collect_reify_types(a, local_type_names, out);
                }
            }
            _ => {}
        }
    }

    fn collect_reify_case(
        &mut self,
        c: &CaseDef,
        local_type_names: &HashSet<String>,
        out: &mut HashMap<NodeId, Type>,
    ) {
        self.collect_reify_types(&c.pat, local_type_names, out);
        self.collect_reify_types(&c.guard, local_type_names, out);
        self.collect_reify_types(&c.body, local_type_names, out);
    }

    fn collect_reify_template(
        &mut self,
        impl_: &Template,
        local_type_names: &HashSet<String>,
        out: &mut HashMap<NodeId, Type>,
    ) {
        for p in &impl_.parents {
            let mut head = p;
            while let TreeKind::Apply { fun, args } = &head.kind {
                for a in args {
                    self.collect_reify_types(a, local_type_names, out);
                }
                head = fun;
            }
            if !mentions_name(head, local_type_names) {
                let ty = self.reify_tree_to_type(head);
                if !ty.is_no_type() && !ty.is_error() && head.id != NodeId(0) {
                    out.insert(head.id, ty);
                }
            }
        }
        for s in &impl_.body {
            self.collect_reify_types(s, local_type_names, out);
        }
    }

    fn collect_reify_tparam_bounds(
        &mut self,
        tp: &Tree,
        local_type_names: &HashSet<String>,
        out: &mut HashMap<NodeId, Type>,
    ) {
        let TreeKind::TypeDef { lo, hi, .. } = &tp.kind else {
            return;
        };
        for b in [lo.as_ref(), hi.as_ref()].into_iter().flatten() {
            if !mentions_name(b, local_type_names) {
                let ty = self.reify_tree_to_type(b);
                if !ty.is_no_type() && !ty.is_error() && b.id != NodeId(0) {
                    out.insert(b.id, ty);
                }
            }
        }
    }

    /// `tree_to_type` with its diagnostics rolled back: a type that does not
    /// resolve here is simply not recorded, and the reifier reports it.
    fn reify_tree_to_type(&mut self, tpt: &Tree) -> Type {
        let mark = self.diags.len();
        let ty = self.tree_to_type(tpt);
        self.diags.truncate(mark);
        ty
    }
    /// How each hole's argument becomes a reflect `Tree` -- `Liftable`.
    ///
    /// A hole is not required to be a `Tree`: nsc infers an implicit
    /// `Liftable[T]` for the argument's type and splices
    /// `Liftable.liftX[T](arg)` (`scala/reflect/api/StandardLiftables.scala`),
    /// which is how `q"($uTag) => $rTag"` works in slick's
    /// `ShapedValue.mapToImpl` where both holes are `WeakTypeTag`s.
    ///
    /// The argument's type is what picks the instance, so each argument is
    /// typed *speculatively* on a clone: the call site's own trees are
    /// untouched and typed once, as part of the reified tree, and the
    /// diagnostics of this probe are rolled back (the same shape as
    /// `probe_named_arg_types`). A function literal says nothing without an
    /// expected type, and is never anything but a tree here, so it is left
    /// alone.
    fn hole_lifts(&mut self, args: &[Tree], ranks: &[u8]) -> Vec<crate::reify::Lift> {
        let mark = self.diags.len();
        let mut out = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate() {
            if matches!(a.kind, TreeKind::Function { .. }) {
                out.push(crate::reify::Lift::Tree);
                continue;
            }
            let mut probe = a.clone();
            self.type_expr(&mut probe, &Type::NoType);
            let rank = ranks.get(i).copied().unwrap_or(0);
            out.push(self.lift_for(&probe.ty, rank));
        }
        self.diags.truncate(mark);
        out
    }

    /// The standard `Liftable` instance for a hole of type `ty` at `rank`.
    ///
    /// Matched by where the type is *declared*: every tree node type is an
    /// abstract type member of `scala.reflect.api.Trees`, every name of
    /// `Names`, every type of `Types`, and `WeakTypeTag` / `Expr` are classes
    /// nested in `TypeTags` / `Exprs`. Anything else has no standard instance
    /// and is reported rather than guessed at (`Lift::Unknown`).
    fn lift_for(&self, ty: &Type, rank: u8) -> crate::reify::Lift {
        use crate::reify::Lift;
        let unknown = || Lift::Unknown(self.st.display_type(ty));
        // `..$xs` lifts the elements, not the collection.
        if rank == 1 {
            let Type::Class { sym, args } = ty else {
                return unknown();
            };
            if args.len() != 1 {
                return unknown();
            }
            let elem = self.lift_for(&args[0], 0);
            // `Symbol` is not a `Liftable`: nsc special-cases the hole and
            // refuses it under `..$` ("consider omitting the dots or
            // providing an implicit instance of Liftable[Symbol]").
            if matches!(elem, Lift::Unknown(_) | Lift::Symbol) {
                return Lift::Unknown(self.st.display_type(&args[0]));
            }
            let to_list = self.st.get(*sym).jvm_name != "scala/collection/immutable/List";
            return Lift::Elems {
                to_list,
                elem: Box::new(elem),
            };
        }
        if rank != 0 {
            return unknown();
        }
        match ty {
            Type::Int
            | Type::Long
            | Type::Short
            | Type::Byte
            | Type::Char
            | Type::Float
            | Type::Double
            | Type::Boolean
            | Type::Unit
            | Type::String => Lift::Value,
            Type::Constant(_) => Lift::Value,
            Type::TypeMember(id) => {
                // A member reached through a term path (`c.Tree`, where `c` is
                // the macro `Context`) is its own symbol, owned by the path's
                // term rather than by the trait that declares it -- that is
                // what makes `p.T` and `q.T` different types. The `Liftable`
                // instance is chosen by where the member is *declared*, so ask
                // for the declaration first; `projected_decl` answers `None`
                // for an ordinary member and the lookup is unchanged. It
                // covers an abstract projection (`E#T`, owned by the type
                // parameter `E`) as well, which has the same hazard.
                let id = &self.st.projected_decl(*id).unwrap_or(*id);
                let owner = self.st.get(self.st.get(*id).owner).jvm_name.clone();
                match owner.as_str() {
                    "scala/reflect/api/Trees" => Lift::Tree,
                    "scala/reflect/api/Names" => Lift::Name,
                    "scala/reflect/api/Types" => Lift::Type,
                    "scala/reflect/api/Constants" => Lift::Constant,
                    "scala/reflect/api/Symbols" => Lift::Symbol,
                    _ => unknown(),
                }
            }
            Type::Class { sym, .. } => match self.st.get(*sym).jvm_name.as_str() {
                "scala/reflect/api/TypeTags$WeakTypeTag" | "scala/reflect/api/TypeTags$TypeTag" => {
                    Lift::TypeTag
                }
                "scala/reflect/api/Exprs$Expr" => Lift::Expr,
                _ => unknown(),
            },
            _ => unknown(),
        }
    }

    /// The expression naming a `scala.reflect.api.Universe` currently in
    /// scope, brought in by `import <universe>._`.
    ///
    /// That import is what makes `q"..."` resolve at all: `q` is a member of
    /// `Quasiquotes.Quasiquote`, which is a member of the universe. The most
    /// recent one wins, the same way the member lookup that found `q` does.
    pub(crate) fn universe_in_scope(&self) -> Option<Tree> {
        self.term_import_prefixes
            .iter()
            .rev()
            .find(|(owner, prefix)| {
                self.is_reflect_universe(*owner) && self.prefix_in_scope(prefix)
            })
            .map(|(_, prefix)| prefix.clone())
    }

    /// Whether `id` is `scala.reflect.api.Universe` or something that extends
    /// it (`api.JavaUniverse`, `runtime.JavaUniverse`, a macro `Context`'s
    /// universe).
    pub(crate) fn is_reflect_universe(&self, id: SymbolId) -> bool {
        if id.is_none() {
            return false;
        }
        let named = |s: SymbolId| {
            let jvm = &self.st.get(s).jvm_name;
            jvm == "scala/reflect/api/Universe" || jvm == "scala/reflect/api/JavaUniverse"
        };
        if named(id) {
            return true;
        }
        self.st
            .symbols
            .iter()
            .filter(|s| named(s.id))
            .any(|s| crate::pickle_supply::inherits_from(&self.st, id, s.id))
    }

    /// Report a quasiquote that could not be typed, saying which of the two
    /// gaps it hit. See `crates/typer/src/quasiquote.rs`.
    fn report_quasiquote(
        &mut self,
        span: Span,
        kind: crate::quasiquote::QuasiKind,
        parts: &[String],
        nargs: usize,
    ) {
        let p = kind.prefix();
        match crate::quasiquote::check_body(kind, parts, nargs) {
            Err(why) => self.error(
                span,
                format!("unimplemented syntax: quasiquote {p}\"...\" ({why})"),
            ),
            Ok(()) => self.error(
                span,
                format!(
                    "macro expansion is not implemented: cannot expand quasiquote {p}\"...\". \
                     Quasiquotes are compiler-internal macros with no implementation in \
                     scala-reflect.jar, so scala-rs has to reify them itself; \
                     see docs/macros.md \u{a7}6.2."
                ),
            ),
        }
    }

    pub(crate) fn type_expr_inner(&mut self, tree: &mut Tree, pt: &Type) {
        if let TreeKind::InterpolatedString {
            prefix,
            parts,
            args,
        } = &tree.kind
        {
            if !matches!(prefix.as_str(), "s" | "f" | "raw")
                && self.library_abi
                && !self.st.lookup("StringContext").is_empty()
            {
                // Kept before desugaring, which replaces the node.
                let quasi = crate::quasiquote::QuasiKind::of(prefix)
                    .map(|k| (k, parts.clone(), args.clone()));
                let span = tree.span;
                let before = self.diags.len();
                self.desugar_custom_interpolator(tree);
                self.type_expr_inner(tree, pt);
                if let Some((kind, parts, args)) = quasi {
                    if self.diags.len() > before {
                        // A user-defined `q` interpolator would have typed.
                        // This one did not, so it is the reflection quasiquote,
                        // whose real problem is not that `StringContext` lacks
                        // the member.
                        self.diags.truncate(before);
                        if !self.reify_quasiquote(tree, span, kind, &parts, &args, pt) {
                            self.report_quasiquote(span, kind, &parts, args.len());
                            tree.ty = Type::Error;
                        }
                    }
                }
                return;
            }
        }
        if matches!(&tree.kind, TreeKind::Assign { .. })
            && self.try_rewrite_dynamic_update(tree, pt)
        {
            return;
        }
        if matches!(&tree.kind, TreeKind::TypeApply { .. })
            && self.try_rewrite_dynamic_type_apply(tree, pt)
        {
            return;
        }
        // Read before the match takes `tree.kind` mutably. Only the `Block`
        // arm uses it, to name the scope its local definitions share.
        let tree_id = tree.id;
        match &mut tree.kind {
            TreeKind::Literal { lit } => {
                tree.ty = if matches!(lit, Lit::Symbol(_)) && self.library_abi {
                    // A symbol literal constructs scala.Symbol; it is not an
                    // unresolved type named Symbol (nor a literal type).
                    let owner = crate::classpath::ensure_package(&mut self.st, "scala");
                    self.load_binary_into("scala/Symbol", owner, tree.span, false);
                    if let Some(sym) = crate::classpath::find_by_jvm(&self.st, "scala/Symbol") {
                        Type::Class { sym, args: vec![] }
                    } else {
                        self.error(tree.span, "scala.Symbol is not available on the classpath");
                        Type::Error
                    }
                } else {
                    Type::Constant(lit.clone())
                };
            }
            TreeKind::This { qual } => {
                let q = qual.clone();
                let id = self.this_owner(q.as_deref());
                if id.is_none() {
                    // nsc `QualifyingClassError`, e.g. `this` in the early
                    // section of a top-level class, which is typed outside it.
                    let msg = match q.as_deref() {
                        Some(name) => format!("{name} is not an enclosing class"),
                        None => "this can be used only in a class, object, or template".into(),
                    };
                    self.error(tree.span, msg);
                    tree.ty = Type::Error;
                } else {
                    tree.sym = id;
                    let own = self.st.self_type_of_class(id);
                    tree.ty = match self.st.get(id).self_type.clone() {
                        Some(required) => Type::Refined {
                            parents: vec![own, required],
                            decls: vec![],
                        },
                        None => own,
                    };
                }
            }
            TreeKind::Select { .. } => self.type_select(tree, pt),
            TreeKind::Apply { .. } => self.type_apply(tree, pt),
            TreeKind::TypeApply { fun, args } => {
                // nsc types the callee of a `TypeApply` in FUNmode. When this
                // `TypeApply` is itself the callee of an `Apply`, the caller
                // hands down a `Method` expectation for exactly that reason,
                // and it has to reach the reference underneath: an overloaded
                // reference typed in *value* position keeps only its
                // parameterless alternative (SLS 6.26.3), and slick's
                // `object TableQuery` has one -- `def apply[E]: TableQuery[E]`
                // next to `def apply[E](cons: Tag => E): TableQuery[E]`. The
                // collapse made `TableQuery.apply[E](cons)` a `TableQuery[E]`
                // applied to an argument: "value apply is not a member of
                // TableQuery[E]". Explicit type arguments cannot break the tie
                // either (both alternatives take one), so the set has to
                // survive to the `Apply`, which picks on the arguments and
                // applies the type arguments through `pending_targs`.
                let fun_pt = match pt {
                    Type::Method { .. } => pt.clone(),
                    _ => Type::NoType,
                };
                self.typing_callee = true;
                self.typing_type_callee = true;
                self.type_expr(fun, &fun_pt);
                self.typing_callee = false;
                // The `Method` expectation is for the overload set's sake
                // alone. Everything else this position holds is still read in
                // value position: fs2's `Stream.fromIterator[F]` is a
                // *parameterless* method returning a value class whose `apply`
                // takes the arguments, and keeping its nullary method type
                // made `fromIterator[IO](it, chunkSize = 1)` an application of
                // the method itself -- "named arguments (method parameters not
                // resolved)", since a nullary method has none.
                if matches!(fun_pt, Type::Method { .. }) && !matches!(fun.ty, Type::Overload(_)) {
                    fun.ty = self.maybe_auto_apply(fun.ty.clone(), &Type::NoType);
                }
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
                    // The key `overload_member_types` was recorded under: the
                    // selection stored the as-seen-from types of the whole
                    // group under its *first* alternative, which is the symbol
                    // the tree carries before any narrowing below.
                    let group_key = fun.sym;
                    let mut base_ty = fun.ty.clone();
                    // nsc (SLS 6.26.3): explicit type arguments narrow an
                    // overloaded reference *before* anything else looks at it.
                    // A selection in value position has already dropped the
                    // alternatives that take parameters (`maybe_auto_apply`),
                    // so by the time the type arguments are read the overload
                    // may be gone. `scala.reflect.macros.Aliases` declares
                    // `Expr` twice -- `val Expr: universe.Expr.type` next to
                    // `def Expr[T: WeakTypeTag](tree: Tree): Expr[T]` -- and
                    // the collapse kept the val, so `c.Expr[Int](tree)` became
                    // `universe.Expr.apply[Int](tree)` and failed against that
                    // method's `(Mirror, TreeCreator)` parameters.
                    // `docs/macros.md` §7.11 residual 1.
                    if let Some((only, ty)) = self.alt_taking_targs(sym, targs.len()) {
                        sym = only;
                        base_ty = ty;
                    }
                    // `Module[T1, T2]` with no explicit `.apply` written still
                    // means the type args target the module's generic `apply`
                    // factory (`HashMap[String, Int]()`, `List[Int]()`) — the
                    // module symbol itself has no tparams, so naively
                    // substituting against it is a no-op and the caller sees
                    // an un-substituted `HashMap[K, V]`.
                    // The reference need not *be* the module symbol: the
                    // `scala` package object exports its aliases as accessors
                    // (`def Equiv(): Equiv$`), so `Equiv[Int]` arrives as a
                    // nullary method whose result is the module class. nsc
                    // reads both the same way -- a stable value of a module's
                    // type, with the type arguments meant for its `apply`.
                    let module_cls = if self.st.get(sym).kind == SymKind::Module {
                        Some(self.st.module_class_of(sym))
                    } else {
                        self.module_class_of_value(sym, &base_ty)
                    };
                    // Set when the redirect below lands on an `apply` the
                    // module *inherits*; see the rewrite after `tree.ty`.
                    let mut inherited_apply = false;
                    if let (true, Some(cls)) = (self.st.get(sym).tparams.is_empty(), module_cls) {
                        // A library companion's `apply` is read from the
                        // pickle on *selection*, and `Ordering[String]` never
                        // writes one: without this the redirect found nothing,
                        // the tree kept the module's own type, and
                        // `Ordering[String].compare` was either "not a member
                        // of Ordering$" or -- reached through the `scala`
                        // package alias -- compiled into `Ordering$.MODULE$`
                        // cast to `Ordering` (`ClassCastException` at run
                        // time). `Ordering.apply[T](implicit ord: Ordering[T])`
                        // is nsc's summoner and is what the type arguments
                        // target here.
                        //
                        // Safe next to the prelude's own factories because
                        // `PickleSupply` declines a copy of a hand-written
                        // member with the same erasure (`agent/setapply`);
                        // before that gate landed this made `List[Int](1, 2)`
                        // "ambiguous overload for apply".
                        self.adopt_cp_module_class(cls);
                        self.supply_from_pickle_class(cls, "apply");
                        let mut candidates: Vec<SymbolId> = self
                            .st
                            .lookup_member(cls, "apply")
                            .into_iter()
                            .filter(|id| self.st.get(*id).tparams.len() == targs.len())
                            .collect();
                        // An omitted `.apply` must see the same inherited
                        // alternatives as an explicit selection. In particular,
                        // loading a factory delegate must not add its overridden
                        // declaration beside the companion's factory.
                        candidates = self.drop_overridden_at(cls, candidates);
                        // Several `apply`s of the same type-parameter count.
                        // Explicit type arguments cannot separate them, so
                        // the position does: SLS 6.26.3 keeps only the
                        // alternatives that *take no parameters* when an
                        // overloaded reference is read in value position, and
                        // only the ones that take some when it is the callee
                        // of an `Apply`. slick's `object TableQuery` is the
                        // case -- `def apply[E](cons: Tag => E)` next to the
                        // macro `def apply[E]: TableQuery[E]` -- so
                        // `TableQuery[Issues]` means the second and
                        // `TableQuery[Issues](tag => new Issues(tag))` the
                        // first. Applied only as a tie-break: if it does not
                        // leave exactly one, the redirect gives up as before.
                        if candidates.len() > 1 {
                            for &c in &candidates {
                                self.complete_lazy_sig(c, tree.span);
                            }
                            let want_params = matches!(pt, Type::Method { .. });
                            let narrowed: Vec<SymbolId> = candidates
                                .iter()
                                .copied()
                                .filter(|&c| self.st.takes_value_params(c) == want_params)
                                .collect();
                            if narrowed.len() == 1 {
                                candidates = narrowed;
                            }
                        }
                        if let [only] = candidates[..] {
                            sym = only;
                            // The redirect reaches a symbol nothing has
                            // *selected*, so nothing has run its signature yet:
                            // `object SE extends SE[Any, Any] { def apply[T, U] =
                            // … }` named before its own definition came back
                            // `<notype>`. A selection completes what it finds;
                            // so does this.
                            self.complete_lazy_sig(only, tree.span);
                            // The symbol's own type still carries the method
                            // wrapper. A *parameterless* factory
                            // (`def apply[L, M, U]: Shape[L, M, U, M]`) is the
                            // value itself, so `RepShape[L, M, U]` must not
                            // keep a nullary method type -- an ordinary
                            // `RepShape.apply[L, M, U]` does not either.
                            //
                            // As seen from the module, not raw: an *inherited*
                            // factory states its result in the parameters of
                            // the trait that declares it. `object HashMap
                            // extends MapFactory[HashMap]` inherits
                            // `def apply[K, V](elems: (K, V)*): CC[K, V]`, so
                            // `mutable.HashMap[A, Int]()` came back as
                            // `CC[A, Int]` and every use of it was "value … is
                            // not a member of CC[A, Int]". A `.apply` written
                            // out goes through `type_select`, which does this
                            // substitution, which is why only the implicit
                            // redirect was wrong.
                            let raw = self.st.get(sym).ty.clone();
                            let recv = self.st.type_of_class(cls);
                            let seen = self.st.subst_as_seen_from(&recv, &raw);
                            base_ty = match seen {
                                Type::Method { paramss, ret } if paramss.is_empty() => *ret,
                                other => other,
                            };
                            inherited_apply = self.st.get(only).owner != cls
                                && self.st.get(fun.sym).kind == SymKind::Module;
                        }
                    }
                    // nsc (SLS 6.26.3): explicit type arguments first narrow an
                    // overloaded reference to the alternatives that take that
                    // many type parameters. Without this, `f.typed[Boolean](x)`
                    // keeps the whole overload as its type and the implicit
                    // clause is searched for the *uninstantiated* `TT[T]`.
                    if matches!(base_ty, Type::Overload(_)) {
                        if let Some(only) = self.only_alt_with_tparams(sym, targs.len()) {
                            self.complete_lazy_sig(only, tree.span);
                            // The *declaration*'s type is written in the
                            // declaring class's own type parameters, and an
                            // alternative inherited from a generic parent is
                            // only itself once the receiver's arguments are
                            // in. Taking `SymbolTable::get(only).ty` raw gave
                            // `s.map[Int](_.length)` on an
                            // `immutable.HashSet[String]` the signature
                            // `IterableOps[A, CC, C]` declares -- "value
                            // length is not a member of A", then "value toList
                            // is not a member of CC[Int]". `s.map(_.length)`,
                            // which never reaches this branch, was fine.
                            base_ty = self.member_ty_as_seen_from(group_key, only, fun);
                            sym = only;
                        }
                    }
                    // nsc reads a Java signature's `Object` as `ObjectTpeJava`,
                    // a type that is *both* `Any` and `AnyRef`: writing `Any`
                    // for a Java method's type parameter therefore instantiates
                    // it at `Object`, not at `scala.Any`.
                    // `java.util.Arrays.copyOf[Any](a: Array[AnyRef], n)`
                    // (slick's `ConstArray`) is what depends on it -- `Array`
                    // is invariant, so with a literal `Any` the argument does
                    // not fit. Real scalac takes the call and gives it back an
                    // `Array[Object]`, which is why `copyOf[Any](…): Array[Any]`
                    // is an error there too.
                    let targs = self.java_object_targs(sym, targs.clone());
                    tree.sym = sym;
                    tree.ty = self.st.subst_tparams(sym, &targs, &base_ty);
                    // Codegen's `peel_fun` walks straight through this
                    // TypeApply to the underlying Select/Ident and uses
                    // *that* node's `.sym`/`.ty` — propagate the redirect
                    // (module → its `apply` method) down so it sees the
                    // method, not the module itself.
                    //
                    // An `apply` the module *inherits* needs the receiver
                    // spelled out as well. `peel_fun` reads the owner off the
                    // method and the *receiver* off the tree shape, and a bare
                    // `Ident` inside a class means `this`: `object TM extends
                    // Fac` with `Fac.apply[K]` compiled `TM[Int](1)` in a
                    // class body into `invokeinterface Fac.apply` on `this`,
                    // and threw `ClassCastException: class Uses cannot be cast
                    // to class Fac` at run time -- the types stayed consistent
                    // so nothing before execution objected (`.agent-brief.md`,
                    // *`verify_failures` is a lower bound*). Written out,
                    // `TM.apply[Int](1)` was correct all along, so this builds
                    // that tree.
                    if inherited_apply {
                        let span = fun.span;
                        let mut qual = std::mem::replace(
                            &mut **fun,
                            Tree::dummy(scala_rs_parser::ast::TreeKind::Empty),
                        );
                        qual.ty = self.st.type_of_class(self.st.module_class_of(qual.sym));
                        **fun = Tree {
                            id: scala_rs_parser::ast::NodeId(0),
                            span,
                            kind: TreeKind::Select {
                                qual: Box::new(qual),
                                name: "apply".into(),
                            },
                            ty: tree.ty.clone(),
                            sym,
                            postfix: false,
                            scala_ref: false,
                            stable_pat: false,
                            byname_thunk: false,
                            byname_type_marker: false,
                        };
                    } else if sym != fun.sym {
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
                            // nsc: `AnyVal` has no runtime class to test for
                            // (every primitive and value class conforms), so
                            // the test is rejected rather than compiled into
                            // an `instanceof Object` that is true for everything.
                            if matches!(targs.first(), Some(Type::AnyVal)) {
                                self.error(
                                    tree.span,
                                    "type AnyVal cannot be used in a type pattern or isInstanceOf test",
                                );
                            }
                            // `x.isInstanceOf[p.type]` compares with `p`, so
                            // codegen needs `p` as a typed term.
                            if let (Some(a), Some(t)) = (args.first_mut(), targs.first()) {
                                self.type_singleton_type_ref(a, t);
                            }
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
                // Record which vals and defs are block statements before any of them
                // is typed: `check_tailrec` runs from the body pass and by
                // then the owner says nothing (a def in a `val`'s right-hand
                // side is owned by the enclosing class, like a member).
                for s in stats.iter() {
                    if matches!(s.kind, TreeKind::DefDef { .. } | TreeKind::ValDef { .. }) {
                        self.block_local_defs.insert((self.file_index, s.id));
                    }
                }
                // Retyping a macro tree preserves declaration identities but
                // rebuilds the block scope. Enter hoisted declarations again;
                // signature completion only enters newly allocated symbols.
                for s in stats.iter() {
                    if !s.sym.is_none()
                        && matches!(
                            s.kind,
                            TreeKind::DefDef { .. }
                                | TreeKind::ClassDef { .. }
                                | TreeKind::ModuleDef { .. }
                                | TreeKind::TypeDef { .. }
                        )
                    {
                        if let Some(name) = s.name() {
                            self.st.enter_in_current(name, s.sym);
                        }
                    }
                }
                // Local classes are visible to the whole block, including
                // statements that precede their definition.
                //
                // The block is also the *scope* a companion pair has to share
                // (nsc `Contexts.lookupSibling`), and two blocks of one method
                // share an owner -- so which block each local class or object
                // was declared in is recorded here, where it is known, and
                // read back by `Checker::companion_scope`. Recorded on every
                // pass, not only the one that allocates the symbol: the
                // signature and body passes both walk this block.
                let block_scope = (self.file_index as u64) << 32 | u64::from(tree_id.0);
                for s in stats.iter_mut() {
                    if !matches!(
                        s.kind,
                        TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. }
                    ) {
                        continue;
                    }
                    if s.sym.is_none() {
                        self.namer(s);
                    }
                    if !s.sym.is_none() {
                        self.st.get_mut(s.sym).local_scope = Some(block_scope);
                        // A `ModuleDef`'s symbol is the module; the access
                        // check walks module *classes*, so the scope has to
                        // reach the one that stands for it.
                        let mc = self.st.module_class_of(s.sym);
                        if !mc.is_none() && mc != s.sym {
                            self.st.get_mut(mc).local_scope = Some(block_scope);
                        }
                    }
                }
                // `implicit class C(x: P) { ... }` desugars to a synthetic
                // `implicit def C(x: P): C = new C(x)` (SLS: nsc does this at
                // the namer). `type_class`/`namer_class` and `namer_module`
                // already run this for class/module *members*
                // (`implicit_class_conversions` below); a block never did,
                // so a local `implicit class` had no conversion method at all
                // to search for, even after the local-implicit-def fix above.
                // `namer_member` both allocates the symbol with the full flag
                // set (`implicit` included) and enters it into the block's
                // current scope, exactly as for a class/module body.
                let conversions = implicit_class_conversions(stats);
                for mut conv in conversions {
                    self.namer_member(&mut conv);
                    stats.push(conv);
                }
                // A local `type` alias is in scope for the whole block, and it
                // had no symbol at all until now: a block never ran the namer
                // over its `TypeDef` statements, so `type B = List[Int]; val
                // v: B = xs` left `B` standing for nothing. That was invisible
                // while an unresolved name in a signature was tolerated; it is
                // not any more. cats' `Monad.ifElseM` is the shape --
                // `type Branches = List[(F[Boolean], F[A])]` followed by
                // `def step(branches: Branches)` -- so the aliases have to be
                // resolved before the local signatures that name them, exactly
                // as a template resolves its type members first.
                //
                // Only up to the first `import`, though: an import inside a
                // block takes effect from where it stands, and resolving a
                // later alias ahead of it types the alias in the wrong scope
                // (`pos/t5305` writes `import O.{F, v}` and then
                // `type x = { type l = (F, v.type) }`). An alias after an
                // import keeps the order it always had.
                let upto = stats
                    .iter()
                    .position(|s| matches!(s.kind, TreeKind::Import { .. }))
                    .unwrap_or(stats.len());
                for s in stats[..upto].iter_mut() {
                    if matches!(s.kind, TreeKind::TypeDef { .. }) {
                        if s.sym.is_none() {
                            self.namer(s);
                        }
                        self.type_member_sig(s);
                    }
                }
                self.finish_type_aliases(&mut stats[..upto]);
                // A local `def` is in scope for the whole block, so it may be
                // called before it is written -- and two of them may call each
                // other. Only the signature is built here (which is what a
                // reference needs); the body still waits its turn below. A
                // `def` with no result type has nothing to build yet: its type
                // comes from its own body, so a forward reference to it is the
                // cycle nsc reports.
                // Complete each hoisted signature in its source-order import
                // context. Restore imports before typing executable statements;
                // only the completed local declarations are block-wide.
                let before_imports = self.st.scopes.last().cloned().unwrap();
                let mut unresolved_local_import = false;
                for s in stats.iter_mut() {
                    if matches!(s.kind, TreeKind::Import { .. }) {
                        self.type_stat(s);
                        if let TreeKind::Import { expr, .. } = &s.kind {
                            unresolved_local_import |= expr.sym.is_none() || expr.ty.is_error();
                        }
                    }
                    if matches!(s.kind, TreeKind::TypeDef { .. }) {
                        if s.sym.is_none() {
                            self.namer(s);
                        }
                        self.type_member_sig(s);
                        self.finish_type_aliases(std::slice::from_mut(s));
                    }
                    if let TreeKind::DefDef { tpt, name, .. } = &s.kind {
                        if name != "<init>" && !tpt.is_empty() {
                            let mark = self.diags.len();
                            self.type_member_sig(s);
                            // A preceding eager local val is unavailable during
                            // hoisting. Complete this provisional signature again
                            // at its declaration, after that import has resolved.
                            if unresolved_local_import {
                                self.sig_done.remove(&(self.file_index, s.id));
                                self.diags.truncate(mark);
                            }
                        }
                    }
                    // A local `lazy val` is in scope for the whole block as well:
                    // `lazy val a: Int = b + 1; lazy val b: Int = 2` is legal (an
                    // eager `val` may not be forward-referenced). As above, only
                    // the signature is built here; the initialiser waits, and with
                    // it the point at which the `lazy val` is forced.
                    if let TreeKind::ValDef { tpt, mods, .. } = &s.kind {
                        if mods.flags.contains(Flags::LAZY)
                            && !tpt.is_empty()
                            && s.id != scala_rs_parser::NodeId(0)
                            && self.lazy_val_presig.insert((self.file_index, s.id))
                        {
                            self.type_val_sig(s);
                        }
                    }
                }
                let completed = self.st.scopes.last().cloned().unwrap();
                *self.st.scopes.last_mut().unwrap() = before_imports;
                for (name, entries) in completed.entries() {
                    for entry in entries {
                        if entry.rank == crate::symbol::BindRank::Definition {
                            self.st.enter_in_current(name, entry.sym);
                        }
                    }
                }
                for index in 0..stats.len() {
                    let (through, rest) = stats.split_at_mut(index + 1);
                    let s = &mut through[index];
                    // A repeated body pass rebuilds the block scope, but
                    // signature completion does not allocate the local again.
                    // Re-enter its existing symbol at its declaration point.
                    if let TreeKind::ValDef { name, .. } = &s.kind {
                        if !s.sym.is_none() {
                            self.st.enter_in_current(name, s.sym);
                        }
                    }
                    self.type_stat(s);
                    if unresolved_local_import && matches!(s.kind, TreeKind::Import { .. }) {
                        // Once its stable prefix is available, complete hoisted
                        // methods before any following statement can call them.
                        // The next import starts a distinct lexical context.
                        for later in rest
                            .iter_mut()
                            .take_while(|s| !matches!(s.kind, TreeKind::Import { .. }))
                        {
                            if matches!(&later.kind, TreeKind::DefDef { name, tpt, .. }
                                if name != "<init>" && !tpt.is_empty())
                            {
                                self.type_member_sig(later);
                            }
                        }
                    }
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
                let branch_tys = [thenp.ty.clone(), elsep.ty.clone()];
                if let Some(num) = self.numeric_branch_lub(pt, &branch_tys) {
                    self.adapt(thenp, &num);
                    self.adapt(elsep, &num);
                    tree.ty = num;
                } else {
                    let joined = self.lub_branches(&thenp.ty, &elsep.ty);
                    tree.ty = self.branch_result_ty(pt, &branch_tys, joined);
                }
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
                        scala_ref: false,
                        stable_pat: false,
                        byname_thunk: false,
                        byname_type_marker: false,
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
                        scala_ref: false,
                        stable_pat: false,
                        byname_thunk: false,
                        byname_type_marker: false,
                    };
                    tree.kind = TreeKind::Apply {
                        fun: Box::new(setter),
                        args: vec![rhs],
                    };
                    self.type_expr(tree, pt);
                    return;
                }
                // The same rewrite for the unqualified form: `bv = 5` where
                // `bv` is an accessor inherited from a class file, whose field
                // is private to the class that declares it.
                if let Some(setter_name) = self.ident_setter_assign_lhs(lhs) {
                    let lhs = std::mem::replace(lhs.as_mut(), Tree::dummy(TreeKind::Empty));
                    let rhs = std::mem::replace(rhs.as_mut(), Tree::dummy(TreeKind::Empty));
                    let setter = Tree {
                        id: lhs.id,
                        span: lhs.span,
                        kind: TreeKind::Ident { name: setter_name },
                        ty: Type::NoType,
                        sym: SymbolId::NONE,
                        postfix: false,
                        scala_ref: false,
                        stable_pat: false,
                        byname_thunk: false,
                        byname_type_marker: false,
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
                        scala_ref: false,
                        stable_pat: false,
                        byname_thunk: false,
                        byname_type_marker: false,
                    };
                    tree.kind = TreeKind::Apply {
                        fun: Box::new(setter),
                        args: vec![rhs],
                    };
                    self.type_expr(tree, pt);
                    return;
                }
                // Java Object storage accepts primitive boxing; reading it
                // remains AnyRef. A substituted generic field is not Object.
                let write_ty = if !lhs.sym.is_none() && self.st.get(lhs.sym).java_object_field {
                    Type::Any
                } else {
                    lhs.ty.clone()
                };
                self.type_expr(rhs, &write_ty);
                self.adapt(rhs, &write_ty);
                self.check_reassignment(lhs);
                tree.ty = Type::Unit;
            }
            TreeKind::Match { .. } => self.type_match(tree, pt),
            TreeKind::New { tpt } => {
                // Set by `type_apply` for the `new C(…)` shape, where the
                // argument list is the caller's business. Cleared here so the
                // sub-trees typed below do not inherit it.
                let applied = std::mem::take(&mut self.new_is_applied);
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
                    tpt.ty = self.with_strict_type_names(|s| s.tree_to_type(tpt));
                    if let Some(id) = self.st.class_sym_of(&tpt.ty) {
                        tpt.sym = id;
                    }
                    self.type_new_prefix(tpt);
                    // nsc's refchecks `checkBounds`, for the one position this
                    // compiler can reach it from: a written `new C[…]`. The
                    // arguments here are the ones the source wrote — an
                    // un-applied `new C` carries the class's own parameters as
                    // placeholders, which satisfy their own bounds and so pass
                    // this silently.
                    if let Type::Class { sym, args } = tpt.ty.clone() {
                        self.check_class_tparam_bounds(sym, &args, tpt.span);
                    }
                } else if matches!(&tpt.kind, TreeKind::Ident { name } if name == crate::materialize::RESOLVED_TYPE)
                {
                    // Already a type: `resolved_class_tpt` built this for a
                    // `copy` rewrite, where the class must not be looked up by
                    // name in whatever file the rewrite runs in.
                    if let Some(id) = self.st.class_sym_of(&tpt.ty) {
                        tpt.sym = id;
                    }
                } else if matches!(&tpt.kind, TreeKind::Ident { .. }) && !tpt.sym.is_none() {
                    // A synthetic `new C(...)` rebuilt from an already-resolved
                    // class symbol (`try_rewrite_case_copy`'s rewrite of
                    // `recv.copy(...)`, which knows `recv`'s class by its
                    // *type*, not by scanning for the name). The class may not
                    // even be lexically reachable by its simple name from this
                    // call site: slick's `ResultConverter.getDumpInfo` returns
                    // a `slick.util.DumpInfo`, and a subclass three files away
                    // that only ever writes `super.getDumpInfo.copy(...)`
                    // never imports `DumpInfo` itself. Falling through to the
                    // ordinary `Ident` branch below, which resolves purely by
                    // name, reported "not found: type DumpInfo" there. The
                    // resolution already happened; nothing left to look up.
                    if tpt.ty.is_no_type() {
                        tpt.ty = Type::Class {
                            sym: tpt.sym,
                            args: vec![],
                        };
                    }
                    self.type_new_prefix(tpt);
                } else if let TreeKind::Ident { name } = &tpt.kind {
                    let n = name.clone();
                    self.expose_unqualified(&n, tpt.span);
                    let mut found = self.st.lookup(&n);
                    // `new X` names a *type*, so a nearer scope that binds `X`
                    // only as a term must not end the search. `lookup` hands
                    // back the innermost slot that carries the name at all, so
                    // the standard library's
                    //
                    // ```scala
                    // class accum extends AbstractFunction2[K, V1, Unit] { … }
                    // …
                    // val accum = new accum
                    // ```
                    //
                    // (`HashMap.concat`) found only the `val` being defined,
                    // fell through to typing `accum` as an expression, and
                    // came back with the half-built value's own `NoType` --
                    // no class, and no diagnostic either, so every later
                    // `accum.current` reported "not a member of <notype>".
                    // `lookup_type` skips a term-only scope by construction.
                    if !found.iter().any(|&s| {
                        matches!(
                            self.st.get(s).kind,
                            SymKind::Class | SymKind::TypeParam | SymKind::TypeMember
                        )
                    }) {
                        self.expose_unqualified_type(&n, tpt.span);
                        let types = self.st.lookup_type(&n);
                        if !types.is_empty() {
                            found = types;
                        }
                    }
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
                    } else if let Some(id) = found.iter().copied().find(|&s| {
                        matches!(
                            self.st.get(s).kind,
                            SymKind::TypeParam | SymKind::TypeMember
                        )
                    }) {
                        // SLS 5.3.2: `new` needs a class type. A type
                        // parameter (`def f[T] = new T`) or an abstract type
                        // member with no `=` right-hand side (`new_alias_target`
                        // just declined it above, so anything reaching here is
                        // genuinely abstract, not a jar alias mid-dealias) is
                        // neither. nsc: "class type required but T found".
                        // Scoped to "resolved, and not a class" exactly --
                        // never to "did not resolve" -- so a name
                        // `expose_unqualified` still has to try (a jar
                        // `TypeMember` completed lazily) is never misjudged
                        // (`agent/parentcheck`'s `strict_type_names` precedent).
                        let desc = self.class_type_required_name(id, &n);
                        self.error(tpt.span, format!("class type required but {desc} found"));
                        tpt.ty = Type::Error;
                        tree.ty = Type::Error;
                        tree.sym = SymbolId::NONE;
                        return;
                    } else {
                        // `new Missing` names a *type* that is not there. Left
                        // to `type_expr` it came out as `not found: value
                        // Missing`, which is not what nsc says and points the
                        // reader at the wrong namespace. `new Obj`, where the
                        // only thing under the name is an `object`, is the
                        // same report in nsc: there is no *type* `Obj` to
                        // build, and letting it through emitted a `new` of the
                        // module class that no constructor answers.
                        self.expose_unqualified_type(&n, tree.span);
                        let types = self.st.lookup_type(&n);
                        let only_module = !types.is_empty()
                            && types.iter().all(|&s| {
                                matches!(
                                    self.st.get(s).kind,
                                    SymKind::Module | SymKind::ModuleClass
                                )
                            });
                        if only_module || (found.is_empty() && types.is_empty()) {
                            self.not_found_error(tpt.span, "type", &n);
                            tpt.ty = Type::Error;
                        } else {
                            self.type_expr(tpt, &Type::NoType);
                        }
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
                // SLS 5.2 / nsc `checkInstantiable`: a plain `new C` of an
                // abstract class or a trait is an error -- only an anonymous
                // subclass (`new C { ... }`, typed above) may instantiate one.
                // Accepted, it emitted `new C; invokespecial C.<init>`, an
                // `InstantiationError` at run time.
                // The prelude's hand-written stand-ins (scala-xml's
                // `NamespaceBinding`, ...) carry flags the real class does not,
                // so only a class read from a pickle or a class file, or
                // defined in source, is trusted.
                let stand_in = !tree.sym.is_none()
                    && tree.sym.0 < self.st.prelude_end
                    && self.st.get(tree.sym).pickled_origin.is_empty();
                if !tree.sym.is_none() && !stand_in && self.st.get(tree.sym).kind == SymKind::Class
                {
                    let s = self.st.get(tree.sym);
                    let is_trait =
                        s.flags.contains(Flags::TRAIT) || s.flags.contains(Flags::INTERFACE);
                    if is_trait || s.flags.contains(Flags::ABSTRACT) {
                        let what = if is_trait { "trait" } else { "class" };
                        let name = s.name.clone();
                        self.error(
                            tpt.span,
                            format!("{what} {name} is abstract; cannot be instantiated"),
                        );
                        tree.ty = Type::Error;
                        return;
                    }
                }
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
                // `new Array(n)` names the class with its element still
                // undetermined, and nsc solves that parameter against the
                // expected type the same way it solves any other constructor's
                // — `val a: Array[Int] = new Array(3)` is `newarray int`, and
                // `take(new Array(2))` at `(Array[String])Int` is `anewarray
                // java/lang/String`. `Array` cannot go through the
                // `Type::Class` rule below because an array type is spelled
                // `Type::Array`, so it is read here.
                //
                // Getting the element wrong is not a compile error but an
                // `ArrayStoreException`, so the *only* two answers taken are
                // the expected type's element and, when nothing constrains it,
                // `Nothing` — which is what nsc's solver also reaches, and
                // which erases to `anewarray java/lang/Object` on both sides
                // (`jvm_desc_array_elem`; scalac reaches the same array class
                // through `ClassTag.Nothing.newArray`). Nothing is guessed:
                // an element the expected type does not name stays `Nothing`
                // rather than being widened to `AnyRef`.
                //
                // The element is written back onto `tpt` as well as onto the
                // `New` node: `gen_new` reads the *prefix's* type to decide
                // between `newarray` and `new`, so leaving `tpt` naming the
                // class emitted `new "[java/lang/Object"` followed by an
                // `invokespecial` of a constructor no array class has.
                //
                // The written *syntax* is what says the element is open, not
                // the type: an argument position types its argument twice, and
                // by the second pass the first pass has already left
                // `Array[Nothing]` on both trees, so a rule that only asked
                // "is this the bare `Array` class?" never fired again and
                // `new C[K, V](0, new Array(0), gen)` kept its `Nothing`. A
                // written `new Array[Nothing](n)` is an `AppliedTypeTree` and
                // is left alone.
                let elem_unwritten = matches!(&tpt.kind,
                    TreeKind::Ident { name } if name != crate::materialize::RESOLVED_TYPE)
                    && self.st.is_array_class(tpt.sym);
                if elem_unwritten
                    || matches!(&tree.ty, Type::Class { sym, args }
                        if args.is_empty() && self.st.is_array_class(*sym))
                {
                    // An element an earlier pass already found is kept. The
                    // same tree is typed more than once with *different*
                    // expected types — a block is typed once for its value and
                    // once for its statements, and `a1 = new Array(WIDTH)`
                    // inside one (`immutable/Vector.scala`) came back through
                    // here with no expected type at all. Reading `pt`
                    // unconditionally overwrote the `AnyRef` the assignment had
                    // just supplied with `Nothing` and put 18 errors back.
                    let elem = match self.array_elem_expected(&tree.ty) {
                        Some(found) if !matches!(found, Type::Nothing) => found,
                        _ => self.array_elem_expected(pt).unwrap_or(Type::Nothing),
                    };
                    tree.ty = Type::Array(Box::new(elem));
                    tpt.ty = tree.ty.clone();
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
                            } else {
                                // `def mk[R]: RC[R, Unit] = new UnitRC` --
                                // the expected type names a *base* class, so
                                // the arguments come from the base type
                                // instance: `UnitRC[R] <: RC[R, Unit]` forces
                                // `R`. Same reading a constructor pattern
                                // does on its scrutinee.
                                let sym = *sym;
                                let targs = self.pattern_class_targs(sym, pt);
                                if !targs.is_empty() {
                                    tree.ty = Type::Class { sym, args: targs };
                                }
                            }
                        }
                    }
                }
                // `new TypedRep[Int]` writes no argument list at all, but the
                // class's only constructor clause is implicit and nsc still
                // passes it. Rewrite to the empty application so the ordinary
                // `new C()` path fills it; without this, codegen emits
                // `TypedRep.<init>()` and the program dies with
                // `NoSuchMethodError` at run time.
                if !applied {
                    // `new Array[Int]` with no argument list at all. `Array`'s
                    // one constructor takes the length, so nsc rejects it in
                    // the same words it uses for `new Array[Int]()`. This
                    // compiler accepted it as an `Array[Int]` *value* and
                    // codegen then emitted `new "[java/lang/Object"` with an
                    // `invokespecial` of a constructor no array class has --
                    // bytecode the verifier refuses, reached with no
                    // diagnostic. The arity check on the applied form lives in
                    // `type_apply`; this is the same check one node up, where
                    // there is no application to carry it.
                    if let Some(elem) = self.array_elem_expected(&tree.ty) {
                        let shown = if elem_unwritten {
                            "T".to_string()
                        } else {
                            self.st.display_type(&elem)
                        };
                        self.error(
                            tree.span,
                            format!(
                                "not enough arguments for constructor Array: \
                                 (_length: Int): Array[{shown}].\n\
                                 Unspecified value parameter _length."
                            ),
                        );
                        return;
                    }
                    let fillable = self
                        .st
                        .class_sym_of(&tree.ty)
                        .is_some_and(|cls| self.parent_ctor_is_fillable(cls));
                    if fillable {
                        let head = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
                        *tree = Tree {
                            id: head.id,
                            span: head.span,
                            kind: TreeKind::Apply {
                                fun: Box::new(head),
                                args: Vec::new(),
                            },
                            ty: Type::NoType,
                            sym: SymbolId::NONE,
                            postfix: false,
                            scala_ref: false,
                            stable_pat: false,
                            byname_thunk: false,
                            byname_type_marker: false,
                        };
                        self.type_apply(tree, pt);
                        return;
                    }
                    self.check_instantiated_self_type(&tree.ty, tree.span);
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
                //
                // A handler that does *not* conform to the body needs the lub
                // too: `try Success(f) catch { case NonFatal(e) => Failure(e) }`
                // is a `Try[R]`, and taking the body's type alone left codegen
                // parking a `Failure` in a slot it had declared `Success`
                // (`VerifyError: Inconsistent stackmap frames`).
                //
                // Not where a branch is `Unit`: nsc lubs `try f() /* Int */
                // catch { println }` to `Any` in statement position, and
                // `gen_try` already fills a default of the body's own sort for
                // that shape. Everything else is boxed into the result slot as
                // needed.
                let handlers: Vec<Type> = catches
                    .iter()
                    .map(|c| c.body.ty.clone())
                    .filter(|t| !matches!(t, Type::Nothing) && !t.is_no_type() && !t.is_error())
                    .collect();
                let no_unit = !matches!(block.ty, Type::Unit)
                    && !handlers.iter().any(|t| matches!(t, Type::Unit));
                // `numericLub`, as for an `if` (see `numeric_branch_lub`):
                // `val t = try 1 catch { case _: E => 2.0 }` is a `Double`, and
                // the body is widened to it so that both leave the same JVM
                // sort in the result slot (it printed `1` where scalac prints
                // `1.0`).
                let mut branch_tys = vec![block.ty.clone()];
                branch_tys.extend(handlers.iter().cloned());
                if let Some(num) = self.numeric_branch_lub(pt, &branch_tys) {
                    self.adapt(block, &num);
                    for c in catches.iter_mut() {
                        self.adapt(&mut c.body, &num);
                    }
                    tree.ty = num;
                } else if matches!(block.ty, Type::Nothing) {
                    tree.ty = handlers
                        .into_iter()
                        .reduce(|a, b| self.lub_ty(&a, &b))
                        .unwrap_or_else(|| block.ty.clone());
                } else if no_unit && !handlers.iter().all(|t| self.st.is_sub_type(t, &block.ty)) {
                    tree.ty = handlers
                        .into_iter()
                        .fold(block.ty.clone(), |a, b| self.lub_ty(&a, &b));
                } else {
                    tree.ty = block.ty.clone();
                }
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
    pub(crate) fn java_lang_package(&self) -> Option<SymbolId> {
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

    /// The name nsc prints for `class type required but <this> found`.
    ///
    /// A bare type parameter is just its name (`T`). An abstract type member
    /// referenced unqualified from inside its own class is the class's
    /// `this`-qualified path (`X.this.A`) -- nsc always resolves an
    /// unqualified name to an implicit `this.` prefix, and prints it that
    /// way even though the source never wrote it.
    fn class_type_required_name(&self, id: SymbolId, fallback: &str) -> String {
        match self.st.get(id).kind {
            SymKind::TypeParam => self.st.get(id).name.clone(),
            SymKind::TypeMember => {
                let owner = self.st.get(id).owner;
                let member = self.st.get(id).name.clone();
                if self.st.get(owner).is_class_like() {
                    let owner_name = self.st.get(owner).name.trim_end_matches('$').to_string();
                    format!("{owner_name}.this.{member}")
                } else {
                    member
                }
            }
            _ => fallback.to_string(),
        }
    }

    /// `not found: <what> <name>`, unless a package object declares `name` as
    /// an alias we could not rebuild -- then say so, rather than let the user
    /// hunt for a name that is really there.
    pub(crate) fn not_found_error(&mut self, span: Span, what: &str, name: &str) {
        if what == "value" && self.report_internal_universe_macro(span, name, false) {
            return;
        }
        match self.pkg_alias_gaps.get(name).cloned() {
            Some(msg) => self.error(span, msg),
            None => self.error(span, format!("not found: {what} {name}")),
        }
    }

    /// `reify { … }` is a *compiler-internal* macro, like the quasiquotes.
    ///
    /// `scala.reflect.api.Universe` declares `def reify[T](expr: T): Expr[T] =
    /// macro …`, but scala-reflect.jar holds no implementation for it: nsc
    /// short-circuits to one built into the compiler (`docs/macros.md` §6.2),
    /// so there is no method to call and the pickle's entry has no erased
    /// descriptor. Reporting "value reify is not a member of JavaUniverse"
    /// was the same untruth the quasiquotes used to draw from `StringContext`
    /// -- `reify` *is* a member, what is missing is the expansion.
    ///
    /// `on_universe` says the receiver is known to be a universe; an
    /// unqualified `reify` is accepted when one is in scope, which is what
    /// `import c.universe._` puts there.
    pub(crate) fn report_internal_universe_macro(
        &mut self,
        span: Span,
        name: &str,
        on_universe: bool,
    ) -> bool {
        if name != "reify" || !self.library_abi {
            return false;
        }
        if !on_universe && self.universe_in_scope().is_none() {
            return false;
        }
        self.error(
            span,
            "macro expansion is not implemented: cannot expand reify { ... }. \
             `reify` is a compiler-internal macro with no implementation in \
             scala-reflect.jar, so scala-rs would have to reify the expression \
             itself, the way it does quasiquotes; see docs/macros.md \u{a7}6.2."
                .to_string(),
        );
        true
    }

    /// The `scala` package, for the implicit `import scala._`.
    pub(crate) fn scala_package(&self) -> Option<SymbolId> {
        self.st
            .lookup_member(self.st.root, "scala")
            .into_iter()
            .find(|&s| self.st.get(s).kind == SymKind::Package)
    }
}

/// What `Check::reify_type_standalone` answers: the type value, the `$u`
/// and `$m` locals it is written against, whether the reification is
/// concrete, and the tag bindings to emit ahead of the creator.
pub(crate) type StandaloneType = (Tree, String, String, bool, Vec<(String, Tree)>);

/// What `Check::reify_facts` gathers.
#[derive(Default)]
struct ReifyFacts {
    local_syms: HashSet<SymbolId>,
    local_type_names: HashSet<String>,
    this_classes: Vec<SymbolId>,
    tags: HashMap<SymbolId, Tree>,
    types: HashMap<NodeId, Type>,
    splices: HashMap<NodeId, Tree>,
    file_name: String,
}

/// Every symbol the body defines, and the names of the types among them.
fn collect_reify_locals(t: &Tree, syms: &mut HashSet<SymbolId>, type_names: &mut HashSet<String>) {
    let mut note = |sym: SymbolId| {
        if !sym.is_none() {
            syms.insert(sym);
        }
    };
    match &t.kind {
        TreeKind::ValDef { rhs, .. } => {
            note(t.sym);
            collect_reify_locals(rhs, syms, type_names);
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            rhs,
            ..
        } => {
            note(t.sym);
            for tp in tparams {
                collect_reify_locals(tp, syms, type_names);
            }
            for p in vparamss.iter().flatten() {
                collect_reify_locals(p, syms, type_names);
            }
            collect_reify_locals(rhs, syms, type_names);
        }
        TreeKind::TypeDef { name, tparams, .. } => {
            note(t.sym);
            type_names.insert(name.clone());
            for tp in tparams {
                collect_reify_locals(tp, syms, type_names);
            }
        }
        TreeKind::ClassDef {
            name,
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            note(t.sym);
            type_names.insert(name.clone());
            for tp in tparams {
                collect_reify_locals(tp, syms, type_names);
            }
            for p in vparamss.iter().flatten() {
                collect_reify_locals(p, syms, type_names);
            }
            for s in &impl_.body {
                collect_reify_locals(s, syms, type_names);
            }
        }
        TreeKind::ModuleDef { name, impl_, .. } => {
            note(t.sym);
            type_names.insert(name.clone());
            for s in &impl_.body {
                collect_reify_locals(s, syms, type_names);
            }
        }
        TreeKind::Function { vparams, body } => {
            for p in vparams {
                collect_reify_locals(p, syms, type_names);
            }
            collect_reify_locals(body, syms, type_names);
        }
        TreeKind::Bind { body, .. } => {
            note(t.sym);
            collect_reify_locals(body, syms, type_names);
        }
        TreeKind::Match { selector, cases } => {
            collect_reify_locals(selector, syms, type_names);
            for c in cases {
                collect_reify_locals(&c.pat, syms, type_names);
                collect_reify_locals(&c.guard, syms, type_names);
                collect_reify_locals(&c.body, syms, type_names);
            }
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            collect_reify_locals(block, syms, type_names);
            for c in catches {
                collect_reify_locals(&c.pat, syms, type_names);
                collect_reify_locals(&c.guard, syms, type_names);
                collect_reify_locals(&c.body, syms, type_names);
            }
            collect_reify_locals(finalizer, syms, type_names);
        }
        TreeKind::Block { stats, expr } => {
            for s in stats {
                collect_reify_locals(s, syms, type_names);
            }
            collect_reify_locals(expr, syms, type_names);
        }
        TreeKind::Apply { fun, args } | TreeKind::UnApply { fun, args } => {
            collect_reify_locals(fun, syms, type_names);
            for a in args {
                collect_reify_locals(a, syms, type_names);
            }
        }
        TreeKind::TypeApply { fun, .. } => collect_reify_locals(fun, syms, type_names),
        TreeKind::Select { qual, .. } => collect_reify_locals(qual, syms, type_names),
        TreeKind::New { tpt } => collect_reify_locals(tpt, syms, type_names),
        TreeKind::Typed { expr, .. } => collect_reify_locals(expr, syms, type_names),
        TreeKind::If { cond, thenp, elsep } => {
            collect_reify_locals(cond, syms, type_names);
            collect_reify_locals(thenp, syms, type_names);
            collect_reify_locals(elsep, syms, type_names);
        }
        TreeKind::Assign { lhs, rhs } => {
            collect_reify_locals(lhs, syms, type_names);
            collect_reify_locals(rhs, syms, type_names);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { body, cond } => {
            collect_reify_locals(cond, syms, type_names);
            collect_reify_locals(body, syms, type_names);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => {
            collect_reify_locals(expr, syms, type_names)
        }
        TreeKind::Alternative { trees } => {
            for a in trees {
                collect_reify_locals(a, syms, type_names);
            }
        }
        TreeKind::Star { elem } => collect_reify_locals(elem, syms, type_names),
        TreeKind::InterpolatedString { args, .. } => {
            for a in args {
                collect_reify_locals(a, syms, type_names);
            }
        }
        TreeKind::Ident { .. } if t.stable_pat => {}
        _ => {}
    }
}

/// The qualifier of every `<e>.splice` in the *untyped* body, by the
/// selection's node: what `.splice` becomes is `<e>.in[$u.type]($m).tree`,
/// and `<e>` is typed again as part of the expansion, so it must be the
/// tree as written and not the typed clone's.
fn collect_reify_splices(t: &Tree, out: &mut HashMap<NodeId, Tree>) {
    if let TreeKind::Select { qual, name } = &t.kind {
        if name == "splice" {
            out.insert(t.id, (**qual).clone());
        }
    }
    for c in tree_children(t) {
        collect_reify_splices(c, out);
    }
}

/// Every abstract type mentioned by a node's type, or by the type of a value
/// the body refers to.
fn collect_reify_abstract_in_tree(st: &SymbolTable, t: &Tree, out: &mut Vec<SymbolId>) {
    crate::reify::collect_abstract(&t.ty, out);
    if let TreeKind::Ident { .. } | TreeKind::Select { .. } = &t.kind {
        if !t.sym.is_none() {
            let s = st.get(t.sym);
            if matches!(s.kind, SymKind::Term | SymKind::Method) {
                crate::reify::collect_abstract(&s.ty, out);
            }
        }
    }
    for c in tree_children(t) {
        collect_reify_abstract_in_tree(st, c, out);
    }
}

/// Whether a written type mentions one of `names` as a leaf.
fn mentions_name(t: &Tree, names: &HashSet<String>) -> bool {
    if names.is_empty() {
        return false;
    }
    match &t.kind {
        TreeKind::Ident { name } => names.contains(name),
        TreeKind::AppliedTypeTree { tpt, args } => {
            mentions_name(tpt, names) || args.iter().any(|a| mentions_name(a, names))
        }
        TreeKind::SelectFromTypeTree { qual, .. } => mentions_name(qual, names),
        TreeKind::CompoundTypeTree { parents, .. } => {
            parents.iter().any(|p| mentions_name(p, names))
        }
        TreeKind::ExistentialTypeTree { tpt, .. } => mentions_name(tpt, names),
        TreeKind::AnnotatedTypeTree { tpt, .. } => mentions_name(tpt, names),
        TreeKind::SingletonTypeTree { ref_ } => match &ref_.kind {
            TreeKind::Ident { name } => names.contains(name),
            _ => false,
        },
        TreeKind::Select { qual, .. } => mentions_name(qual, names),
        TreeKind::TypeDef { lo, hi, .. } => {
            lo.as_ref().is_some_and(|b| mentions_name(b, names))
                || hi.as_ref().is_some_and(|b| mentions_name(b, names))
        }
        _ => false,
    }
}

/// The direct subtrees of `t`, for the walks above.
fn tree_children(t: &Tree) -> Vec<&Tree> {
    let mut out: Vec<&Tree> = Vec::new();
    match &t.kind {
        TreeKind::PackageDef { pid, stats } => {
            out.push(pid);
            out.extend(stats.iter());
        }
        TreeKind::Import { expr, .. } => out.push(expr),
        TreeKind::ClassDef {
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            out.extend(tparams.iter());
            out.extend(vparamss.iter().flatten());
            out.extend(impl_.parents.iter());
            out.extend(impl_.body.iter());
        }
        TreeKind::ModuleDef { impl_, .. } => {
            out.extend(impl_.parents.iter());
            out.extend(impl_.body.iter());
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            out.push(tpt);
            out.push(rhs);
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            tpt,
            rhs,
            ..
        } => {
            out.extend(tparams.iter());
            out.extend(vparamss.iter().flatten());
            out.push(tpt);
            out.push(rhs);
        }
        TreeKind::MacroRhs { impl_ref } => out.push(impl_ref),
        TreeKind::TypeDef {
            tparams,
            rhs,
            lo,
            hi,
            ..
        } => {
            out.extend(tparams.iter());
            out.push(rhs);
            out.extend(lo.iter().map(|b| b.as_ref()));
            out.extend(hi.iter().map(|b| b.as_ref()));
        }
        TreeKind::LabelDef { params, rhs, .. } => {
            out.extend(params.iter());
            out.push(rhs);
        }
        TreeKind::Block { stats, expr } => {
            out.extend(stats.iter());
            out.push(expr);
        }
        TreeKind::If { cond, thenp, elsep } => {
            out.push(cond);
            out.push(thenp);
            out.push(elsep);
        }
        TreeKind::Match { selector, cases } => {
            out.push(selector);
            for c in cases {
                out.push(&c.pat);
                out.push(&c.guard);
                out.push(&c.body);
            }
        }
        TreeKind::Function { vparams, body } => {
            out.extend(vparams.iter());
            out.push(body);
        }
        TreeKind::Assign { lhs, rhs } => {
            out.push(lhs);
            out.push(rhs);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { body, cond } => {
            out.push(cond);
            out.push(body);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => out.push(expr),
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            out.push(block);
            for c in catches {
                out.push(&c.pat);
                out.push(&c.guard);
                out.push(&c.body);
            }
            out.push(finalizer);
        }
        TreeKind::New { tpt } => out.push(tpt),
        TreeKind::Typed { expr, tpt } => {
            out.push(expr);
            out.push(tpt);
        }
        TreeKind::TypeApply { fun, args } | TreeKind::Apply { fun, args } => {
            out.push(fun);
            out.extend(args.iter());
        }
        TreeKind::Select { qual, .. } => out.push(qual),
        TreeKind::Bind { body, .. } => out.push(body),
        TreeKind::Star { elem } => out.push(elem),
        TreeKind::Alternative { trees } => out.extend(trees.iter()),
        TreeKind::UnApply { fun, args } => {
            out.push(fun);
            out.extend(args.iter());
        }
        TreeKind::AppliedTypeTree { tpt, args } => {
            out.push(tpt);
            out.extend(args.iter());
        }
        TreeKind::SingletonTypeTree { ref_ } => out.push(ref_),
        TreeKind::AnnotatedTypeTree { tpt, annot } => {
            out.push(tpt);
            out.push(annot);
        }
        TreeKind::SelectFromTypeTree { qual, .. } => out.push(qual),
        TreeKind::CompoundTypeTree {
            parents,
            refinements,
        } => {
            out.extend(parents.iter());
            out.extend(refinements.iter());
        }
        TreeKind::ExistentialTypeTree { tpt, clauses } => {
            out.push(tpt);
            out.extend(clauses.iter());
        }
        TreeKind::InterpolatedString { args, .. } => out.extend(args.iter()),
        TreeKind::Empty
        | TreeKind::Super { .. }
        | TreeKind::This { .. }
        | TreeKind::Ident { .. }
        | TreeKind::Literal { .. }
        | TreeKind::Wildcard
        | TreeKind::Unimplemented { .. } => {}
    }
    out
}
