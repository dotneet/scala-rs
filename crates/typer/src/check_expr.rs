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
use crate::symbol::SymKind;
use crate::uncurry::is_eta_marker;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;
use std::collections::HashMap;

impl Typer {
    pub(crate) fn type_expr(&mut self, tree: &mut Tree, pt: &Type) {
        // Taken, not read: everything typed below this point is no longer the
        // callee of the application that set it. See `typing_callee`.
        let callee = std::mem::take(&mut self.typing_callee);
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
        self.adapt_implicit_apply(tree, pt);
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
    /// `crate::reify_expand`).
    ///
    /// Returns false only when this is not a `reify` application at all. A
    /// body reify cannot build is an *error* here, never a silent pass: an
    /// expansion that reified a local as the bare name it was written with
    /// would compile, run, and mean whatever stood at the call site.
    pub(crate) fn try_expand_reify(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let Some(universe) = self.reify_universe(tree) else {
            return false;
        };
        let span = tree.span;
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
        self.type_expr(&mut probe, &Type::NoType);
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

        self.gensym += 1;
        let n = self.gensym;
        let (universe_local, mirror_local) = (format!("$u${n}"), format!("$m${n}"));
        let refs = self.reify_refs(&body);
        let needs_mirror = !refs.is_empty();
        let src = self
            .sources
            .get(self.file_index)
            .cloned()
            .unwrap_or_else(|| std::rc::Rc::from(""));
        let built = {
            let universe_ident = Tree::new(
                NodeId(0),
                span,
                TreeKind::Ident {
                    name: universe_local.clone(),
                },
            );
            let r = crate::reify::Reifier::new(universe_ident, &[], &[], &[], span, &src).in_reify(
                crate::reify::ReifyCtx {
                    refs,
                    mirror_local: mirror_local.clone(),
                    universe_local: universe_local.clone(),
                },
            );
            match r.reify(crate::quasiquote::QuasiKind::Term, &body) {
                Ok(t) => t,
                Err(why) => {
                    self.report_reify_gap(span, &why);
                    tree.ty = Type::Error;
                    return true;
                }
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
        // `Expr` is a nested `object` of the universe, supplied on demand
        // (`PickleSupply::install_nested_module`); nothing has asked for it
        // on this receiver yet.
        let universe_ty = universe.ty.clone();
        let _ = self.supply_from_pickle(&universe_ty, "Expr");
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
            universe_local,
            mirror_local,
            needs_mirror,
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

    /// A form `reify` does not build. Named, with the reason, and pointed at
    /// the design note -- never accepted.
    fn report_reify_gap(&mut self, span: Span, why: &str) {
        self.error(
            span,
            format!(
                "cannot expand reify {{ ... }}: {why}. scala-rs reifies literals, \
                 applications and selections over static `object` references, \
                 `.splice`d expressions, and type arguments it can rebuild from a \
                 tag; see docs/macros.md \u{a7}7.15."
            ),
        );
    }

    /// Classify every identifier of a `reify { … }` body; see
    /// `crate::reify::ReifyRef`.
    ///
    /// Each candidate is typed on a *clone* and rolled back, the way
    /// `Self::hole_lifts` types a hole's argument: what a name means is a
    /// question only the typer can answer, and asking it must not type the
    /// body twice for real. A name this does not classify is left out, and
    /// `crate::reify` refuses it by name.
    fn reify_refs(&mut self, body: &Tree) -> HashMap<NodeId, crate::reify::ReifyRef> {
        let mut out = HashMap::new();
        self.reify_refs_in(body, &mut out);
        out
    }

    fn reify_refs_in(&mut self, t: &Tree, out: &mut HashMap<NodeId, crate::reify::ReifyRef>) {
        match &t.kind {
            // `x.splice`: `Expr[T].splice` is the marker nsc replaces with the
            // argument's own tree. The receiver is left as it was written --
            // it names something in the macro implementation, and the
            // expansion keeps it there.
            TreeKind::Select { qual, name } if name == "splice" => {
                let probed = self.reify_probe(qual).ty;
                if matches!(&probed, Type::Class { sym, .. }
                    if self.st.get(*sym).jvm_name == "scala/reflect/api/Exprs$Expr")
                {
                    out.insert(
                        t.id,
                        crate::reify::ReifyRef::Splice(Box::new((**qual).clone())),
                    );
                    return;
                }
                self.reify_refs_in(qual, out);
            }
            TreeKind::Ident { .. } | TreeKind::Select { .. } => {
                let probe = self.reify_probe(t);
                if let Some(name) = self.static_module_name(&probe.ty) {
                    out.insert(t.id, crate::reify::ReifyRef::StaticModule(name));
                    return;
                }
                if matches!(t.kind, TreeKind::Ident { .. }) {
                    if let Some(r) = self.static_member_ref(probe.sym) {
                        out.insert(t.id, r);
                        return;
                    }
                }
                if let TreeKind::Select { qual, .. } = &t.kind {
                    self.reify_refs_in(qual, out);
                }
            }
            TreeKind::Apply { fun, args } => {
                self.reify_refs_in(fun, out);
                // A bare `println` is overloaded, so typing it on its own
                // settles nothing and the walk above left it unclassified.
                // The *application* settles it, so the callee is resolved
                // from the typed whole and recorded against the node that
                // was written.
                let callee = reify_callee(fun);
                if matches!(callee.kind, TreeKind::Ident { .. }) && !out.contains_key(&callee.id) {
                    if let Some(r) = self.applied_static_member(t) {
                        out.insert(callee.id, r);
                    }
                }
                for a in args {
                    self.reify_refs_in(a, out);
                }
            }
            TreeKind::Block { stats, expr } => {
                for s in stats {
                    self.reify_refs_in(s, out);
                }
                self.reify_refs_in(expr, out);
            }
            // A type argument is rebuilt rather than named: `f[E]` inside a
            // macro implementation means the `E` *that implementation* was
            // instantiated at, which is knowable only through the tag in
            // scope. Same three shapes a `TypeTag` is built from, so the same
            // builder answers -- and the same refusal when it cannot.
            TreeKind::TypeApply { fun, args } => {
                self.reify_refs_in(fun, out);
                for a in args {
                    let ty = self.tree_to_type(a);
                    if ty.is_no_type() || ty.is_error() {
                        continue;
                    }
                    let mark = self.diags.len();
                    let body = self.tag_body(crate::materialize::Tag::Weak, &ty, a.span);
                    self.diags.truncate(mark);
                    out.insert(
                        a.id,
                        match body {
                            Ok(b) => crate::reify::ReifyRef::Type(Box::new(b)),
                            Err(why) => crate::reify::ReifyRef::TypeGap(why),
                        },
                    );
                }
            }
            TreeKind::If { cond, thenp, elsep } => {
                self.reify_refs_in(cond, out);
                self.reify_refs_in(thenp, out);
                self.reify_refs_in(elsep, out);
            }
            _ => {}
        }
    }

    /// One subtree of a reify body, typed speculatively on a clone: the
    /// result carries both the type it has and the symbol it resolved to,
    /// and the call site's own tree is untouched.
    fn reify_probe(&mut self, t: &Tree) -> Tree {
        let mark = self.diags.len();
        let mut probe = t.clone();
        self.type_expr(&mut probe, &Type::NoType);
        self.diags.truncate(mark);
        probe
    }

    /// The full name `Mirror.staticModule` is given for a reference to a
    /// static `object`, if that is what `ty` is.
    ///
    /// Static means reachable through packages alone, which is what
    /// `staticModule` walks: an `object` nested in a class or another object
    /// has a `$` in its class file's simple name and is reached by
    /// `selectTerm` on the enclosing symbol instead -- a second shape, and
    /// `crate::reify` refuses the name rather than building the wrong one.
    fn static_module_name(&self, ty: &Type) -> Option<String> {
        let Type::ModuleRef(mcls) = ty else {
            return None;
        };
        self.static_module_full_name(*mcls)
    }

    /// The same, from the module class itself.
    fn static_module_full_name(&self, mcls: SymbolId) -> Option<String> {
        if self.st.get(mcls).kind != SymKind::ModuleClass {
            return None;
        }
        let jvm = self.st.jvm_internal(mcls);
        let full = jvm.strip_suffix('$').unwrap_or(&jvm);
        if full.is_empty() || full.rsplit('/').next().is_some_and(|s| s.contains('$')) {
            return None;
        }
        Some(full.replace('/', "."))
    }

    /// A term member *declared by* a static `object` and named without its
    /// owner -- `println`, or a name an `import P4Helper._` brought in.
    ///
    /// This is what lets a name that is not itself an `object` be reified by
    /// symbol: nsc's typer has already turned `println` into
    /// `scala.Predef.println` by the time its reifier sees it, and it builds
    /// `Select(mkIdent(staticModule("scala.Predef")), TermName("println"))`
    /// (measured with `-Xprint:typer`). Requiring the *declaring* owner to be
    /// a static `object` is what keeps this honest: a local, a parameter, a
    /// member of a class, or a member of an `object` nested in one all fail
    /// the test and stay refused, because none of them can be found again
    /// through a mirror.
    ///
    /// Two members of a static `object` are still refused.
    ///
    /// * One whose owner **lexically encloses** the `reify` -- a `val` of the
    ///   very `object` the macro implementation is written in. nsc's typer
    ///   spells that `Impls.this.x`, and its reifier builds a *different*
    ///   tree for it (`mkThis(staticModule("Impls").asModule.moduleClass)`,
    ///   measured on `test/files/run/macro-reify-ref-to-packageless`).
    ///   Building the `mkIdent` form would evaluate to the same member but
    ///   print as a different tree, and would lose access to a `private` one.
    /// * One whose name is not a legal JVM identifier once encoded. nsc
    ///   escapes the rest (` ` is `$u0020`) and `NameTransformer` here does
    ///   not, so the `TermName` would name a member that does not exist.
    fn static_member_ref(&self, sym: SymbolId) -> Option<crate::reify::ReifyRef> {
        if sym.is_none() {
            return None;
        }
        let s = self.st.get(sym);
        if !matches!(s.kind, SymKind::Method | SymKind::Term) {
            return None;
        }
        if s.flags.contains(Flags::PARAM) {
            return None;
        }
        let (owner_sym, name) = (s.owner, s.name.clone());
        if name.is_empty()
            || !scala_rs_pickle::names::encode_method_name(&name)
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        {
            return None;
        }
        let owner = self.static_module_full_name(owner_sym)?;
        if self
            .st
            .enclosing_classes(self.st.owner)
            .contains(&owner_sym)
        {
            return None;
        }
        Some(crate::reify::ReifyRef::StaticMember { owner, name })
    }

    /// The callee of an application, resolved by typing the application --
    /// which is the only thing that picks between overloads.
    fn applied_static_member(&mut self, apply: &Tree) -> Option<crate::reify::ReifyRef> {
        let probe = self.reify_probe(apply);
        let mut head = &probe;
        while let TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } = &head.kind {
            head = fun;
        }
        if !matches!(head.kind, TreeKind::Ident { .. } | TreeKind::Select { .. }) {
            return None;
        }
        self.static_member_ref(head.sym)
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
        match &mut tree.kind {
            TreeKind::Literal { lit } => {
                tree.ty = Type::Constant(lit.clone());
            }
            TreeKind::This { qual } => {
                let q = qual.clone();
                let id = self.this_owner(q.as_deref());
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
                            base_ty = match self.st.get(sym).ty.clone() {
                                Type::Method { paramss, ret } if paramss.is_empty() => *ret,
                                other => other,
                            };
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
                // Record which `def`s are block statements before any of them
                // is typed: `check_tailrec` runs from the body pass and by
                // then the owner says nothing (a def in a `val`'s right-hand
                // side is owned by the enclosing class, like a member).
                for s in stats.iter() {
                    if matches!(s.kind, TreeKind::DefDef { .. }) {
                        self.block_local_defs.insert((self.file_index, s.id));
                    }
                }
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
                for s in stats.iter_mut() {
                    if let TreeKind::DefDef { tpt, name, .. } = &s.kind {
                        if name != "<init>" && !tpt.is_empty() {
                            self.type_member_sig(s);
                        }
                    }
                }
                // A local `lazy val` is in scope for the whole block as well:
                // `lazy val a: Int = b + 1; lazy val b: Int = 2` is legal (an
                // eager `val` may not be forward-referenced). As above, only
                // the signature is built here; the initialiser waits, and with
                // it the point at which the `lazy val` is forced.
                for s in stats.iter_mut() {
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
                tree.ty = pt_or_lub(pt, self.lub_branches(&thenp.ty, &elsep.ty));
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
                        self.expose_unqualified_type(&n);
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
                        };
                        self.type_apply(tree, pt);
                        return;
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
                tree.ty = if matches!(block.ty, Type::Nothing) {
                    handlers
                        .into_iter()
                        .reduce(|a, b| self.lub_ty(&a, &b))
                        .unwrap_or_else(|| block.ty.clone())
                } else if no_unit && !handlers.iter().all(|t| self.st.is_sub_type(t, &block.ty)) {
                    handlers
                        .into_iter()
                        .fold(block.ty.clone(), |a, b| self.lub_ty(&a, &b))
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
