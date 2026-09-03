//! Def macros: `def f: T = macro Impl.method[A]`.
//!
//! Phase 1 handles the *definition* side only. A macro def is resolved to its
//! implementation, checked against the shape rules nsc enforces, and recorded
//! on the symbol as a [`MacroBinding`]. Nothing is expanded yet: every call
//! site is diagnosed by [`Typer::report_macro_calls`] rather than silently
//! accepted, and [`Typer::strip_macro_defs`] keeps the macro def itself out of
//! the bytecode the way nsc does. See `docs/macros.md` for the full design.

use scala_rs_parser::{CaseDef, SymbolId, Template, Tree, TreeKind, Type};
use scala_rs_span::Span;

use crate::check::Typer;
use crate::symbol::{MacroBinding, SymKind};

/// Fully-qualified names of the two macro `Context` types.
const BLACKBOX_CONTEXT: &str = "scala.reflect.macros.blackbox.Context";
const WHITEBOX_CONTEXT: &str = "scala.reflect.macros.whitebox.Context";

/// `Some(true)` for the blackbox `Context`, `Some(false)` for the whitebox
/// one, `None` for any other class.
fn context_kind_of_name(name: &str) -> Option<bool> {
    if name == BLACKBOX_CONTEXT || name.ends_with("blackbox.Context") {
        Some(true)
    } else if name == WHITEBOX_CONTEXT || name.ends_with("whitebox.Context") {
        Some(false)
    } else {
        None
    }
}

/// Peel `Impl.method[A, B]` down to the reference and its explicit type args.
fn split_type_apply(t: &Tree) -> (&Tree, usize) {
    match &t.kind {
        TreeKind::TypeApply { fun, args } => (fun, args.len()),
        _ => (t, 0),
    }
}

/// Render `Impl.method` back to source form for diagnostics.
fn path_of(t: &Tree) -> Option<String> {
    match &t.kind {
        TreeKind::Ident { name } => Some(name.clone()),
        TreeKind::Select { qual, name } => Some(format!("{}.{}", path_of(qual)?, name)),
        _ => None,
    }
}

impl Typer {
    /// Type the right-hand side of `def f = macro <impl_ref>`.
    ///
    /// Returns `true` when `tree` is a macro def, whether or not the binding
    /// could be resolved; the caller must not then type the rhs as an
    /// expression.
    pub(crate) fn type_macro_def(&mut self, tree: &mut Tree) -> bool {
        let (impl_ref, tpt_is_empty, name) = match &tree.kind {
            TreeKind::DefDef { rhs, tpt, name, .. } => match &rhs.kind {
                TreeKind::MacroRhs { impl_ref } => {
                    ((**impl_ref).clone(), tpt.is_empty(), name.clone())
                }
                _ => return false,
            },
            _ => return false,
        };

        // nsc: "macro defs must have explicitly specified return types".
        // Without one there is nothing to check the expansion against.
        if tpt_is_empty {
            self.error(
                tree.span,
                format!("macro definition {name} must have an explicitly specified result type"),
            );
        }

        if let Some(binding) = self.resolve_macro_impl(&impl_ref, tree.span) {
            if !tree.sym.is_none() {
                self.st.get_mut(tree.sym).macro_impl = Some(binding);
                self.has_macro_defs = true;
            }
        }
        true
    }

    /// Resolve `Impl.method[A]` to the class/method pair the expander needs.
    ///
    /// Diagnoses and returns `None` when the reference has a shape nsc also
    /// rejects, or when it does not name a method of an object.
    fn resolve_macro_impl(&mut self, impl_ref: &Tree, span: Span) -> Option<MacroBinding> {
        let (base, _targs) = split_type_apply(impl_ref);
        let Some(path) = path_of(base) else {
            self.error(
                span,
                "macro implementation reference has wrong shape. required:\n\
                 macro [<static object>].<method name>[[<type args>]]",
            );
            return None;
        };

        let Some(sym) = self.lookup_macro_impl_method(base) else {
            self.error(span, format!("macro implementation not found: {path}"));
            return None;
        };

        let owner = self.st.get(sym).owner;
        if self.st.get(owner).kind != SymKind::ModuleClass {
            // nsc also allows a "macro bundle" (`class B(val c: Context)`), which
            // we do not implement. Either way this is not a static object.
            self.error(
                span,
                format!(
                    "macro implementation reference has wrong shape: \
                     {path} is not a method of an object"
                ),
            );
            return None;
        }

        // The implementation must be resolvable by name alone at expansion time:
        // nsc's runtime picks `getMethods.filter(_.getName == methodName).head`.
        let name = self.st.get(sym).name.clone();
        let overloads = self
            .st
            .lookup_member(owner, &name)
            .into_iter()
            .filter(|&s| self.st.get(s).kind == SymKind::Method)
            .count();
        if overloads > 1 {
            self.error(
                span,
                format!("macro implementation {path} cannot be overloaded"),
            );
            return None;
        }

        let blackbox = match self.macro_context_kind(sym) {
            Some(b) => b,
            None => {
                self.error(
                    span,
                    format!(
                        "macro implementation {path} must take \
                         {BLACKBOX_CONTEXT} (or the whitebox one) as its first parameter"
                    ),
                );
                return None;
            }
        };
        if !blackbox {
            // The design deliberately implements blackbox first; see docs/macros.md.
            self.error(
                span,
                format!("whitebox macros are not implemented (macro implementation {path})"),
            );
            return None;
        }

        let impl_class = {
            let jvm = self.st.get(owner).jvm_name.clone();
            if jvm.is_empty() {
                self.st.get(owner).name.clone()
            } else {
                jvm.replace('/', ".")
            }
        };
        Some(MacroBinding {
            impl_class,
            impl_method: name,
            blackbox,
            tag_params: self.macro_impl_tag_params(sym),
            expr_args: self.macro_impl_expr_args(sym),
        })
    }

    /// Which of the implementation's value parameters are `c.Expr[T]`.
    ///
    /// The leading `Context` and the trailing tag clause are not arguments of
    /// the macro application, so they are left out; what is left lines up one
    /// for one with the call site's arguments.
    fn macro_impl_expr_args(&self, impl_sym: SymbolId) -> Vec<bool> {
        let mut flat = self.macro_impl_params(impl_sym);
        // The leading `Context`, then the trailing tags.
        if !flat.is_empty() {
            flat.remove(0);
        }
        while flat.last().is_some_and(|&p| self.is_tag_param(p)) {
            flat.pop();
        }
        flat.iter()
            .map(|&p| self.st.display_type(&self.st.get(p).ty).contains("Expr"))
            .collect()
    }

    /// Every value parameter of the implementation, clauses flattened.
    ///
    /// A method read back from a class file may carry its parameters in
    /// `paramss` or, when the pickle recorded no clause structure, in `params`.
    fn macro_impl_params(&self, impl_sym: SymbolId) -> Vec<SymbolId> {
        let sym = self.st.get(impl_sym);
        if sym.paramss.is_empty() {
            sym.params.clone()
        } else {
            sym.paramss.iter().flatten().copied().collect()
        }
    }

    /// Is this parameter one of the `c.WeakTypeTag[T]` a macro implementation's
    /// trailing implicit clause takes?
    ///
    /// The simple-name arm is for implementations read from a class file
    /// scala-rs wrote: its pickle subset records member types by simple name,
    /// so `c.WeakTypeTag[T]` arrives as an unresolved `WeakTypeTag`.
    fn is_tag_param(&self, p: SymbolId) -> bool {
        let ty = self.st.get(p).ty.clone();
        if crate::materialize::tag_request(&self.st, &ty).is_some() {
            return true;
        }
        matches!(&ty, Type::Named { name, args }
            if args.len() == 1 && (name == "WeakTypeTag" || name == "TypeTag"))
    }

    /// How many `c.WeakTypeTag[T]` the implementation's trailing clause takes.
    ///
    /// nsc allows the clause to be left out entirely -- an implementation that
    /// does not look at its type arguments simply does not ask for tags -- so
    /// the count has to come off the implementation's own signature and not
    /// off the macro def's type parameters.
    fn macro_impl_tag_params(&self, impl_sym: SymbolId) -> usize {
        self.macro_impl_params(impl_sym)
            .iter()
            .rev()
            .take_while(|&&p| self.is_tag_param(p))
            .count()
    }

    /// Look up the method a macro implementation reference names.
    ///
    /// The reference is *typed*, not looked up by hand. nsc requires the
    /// implementation to be compiled by an earlier run, so it is normally a
    /// class file on `-cp`, and reaching a class file's members means going
    /// through the same lazy loading (`install_java_class`, the pickle
    /// supply, companion modules) that an ordinary selection goes through --
    /// a hand-rolled scope walk found only the implementations that this run
    /// happens to compile itself, which are exactly the ones that *cannot*
    /// be expanded.
    ///
    /// The probe types a copy, and its diagnostics are rolled back: what a
    /// failure means here is "macro implementation not found", which the
    /// caller reports.
    fn lookup_macro_impl_method(&mut self, base: &Tree) -> Option<SymbolId> {
        let mut probe = base.clone();
        // A method type as the expected type, the way `type_apply` types a
        // callee: a nullary implementation must not be auto-applied, and an
        // implementation with parameters must not be eta-expanded.
        let dummy = Type::Method {
            paramss: Vec::new(),
            ret: Box::new(Type::NoType),
        };
        let mark = self.diags.len();
        let saved_callee = std::mem::replace(&mut self.typing_callee, true);
        self.type_expr(&mut probe, &dummy);
        self.typing_callee = saved_callee;
        self.diags.truncate(mark);
        let sym = probe.sym;
        if sym.is_none() || self.st.get(sym).kind != SymKind::Method {
            return None;
        }
        Some(sym)
    }

    /// `Some(true)` for a blackbox `Context` first parameter, `Some(false)` for
    /// whitebox, `None` when the first parameter is not a macro `Context`.
    fn macro_context_kind(&mut self, impl_sym: SymbolId) -> Option<bool> {
        let sym = self.st.get(impl_sym);
        let first = sym
            .paramss
            .first()
            .and_then(|c| c.first())
            .or_else(|| sym.params.first())?;
        let ty = self.st.get(*first).ty.clone();
        let mut names = Vec::new();
        // `blackbox.Context { type PrefixType = ShapedValue[_, U] }`: nsc's
        // own idiom for a macro that wants `c.prefix` at a useful type, and
        // how slick declares `ShapedValue.mapToImpl`. The refinement only pins
        // a type member down; which `Context` it refines is still what decides
        // blackbox from whitebox, so the parents are candidates too.
        Self::context_type_names(&self.st, &ty, &mut names);
        // Last resort, and the only answer when the implementation came back
        // from a class file. scala-rs's own pickle subset records a member's
        // parameter types by *simple* name -- and a refined one not at all: it
        // reads back as `Any`. A simple name cannot say blackbox from
        // whitebox, and `Any` says nothing, but the erased descriptor says
        // exactly what the JVM will be handed and cannot be refined away. A
        // first parameter that really is `Any` erases to `java.lang.Object`,
        // classifies as neither context, and is still refused.
        names.extend(self.macro_context_from_descriptor(impl_sym));
        names.iter().find_map(|n| context_kind_of_name(n))
    }

    /// Every dotted class name a macro implementation's first parameter type
    /// could be naming, best first. A type that is not a class contributes
    /// nothing.
    fn context_type_names(st: &crate::symbol::SymbolTable, ty: &Type, out: &mut Vec<String>) {
        match ty {
            Type::Class { sym, .. } => {
                let s = st.get(*sym);
                out.push(if s.jvm_name.is_empty() {
                    s.name.clone()
                } else {
                    s.jvm_name.replace(['/', '$'], ".")
                });
            }
            Type::Refined { parents, .. } => {
                for p in parents {
                    Self::context_type_names(st, p, out);
                }
            }
            _ => {}
        }
    }

    /// The first parameter type of the implementation, read off its class
    /// file's method descriptor.
    fn macro_context_from_descriptor(&mut self, impl_sym: SymbolId) -> Option<String> {
        let owner = self.st.get(impl_sym).owner;
        let jvm = self.st.get(owner).jvm_name.clone();
        let name = self.st.get(impl_sym).name.clone();
        if jvm.is_empty() {
            return None;
        }
        let bytes = self.binary.find_class(&jvm).ok().flatten()?;
        let jc = crate::javaclass::parse_java_classfile(&bytes).ok()?;
        let m = jc.methods.iter().find(|m| m.name == name)?;
        let first = m.desc.strip_prefix('(')?;
        let end = first.find(';')?;
        Some(first.get(1..end)?.replace('/', "."))
    }

    /// Report every macro application the expander left standing.
    ///
    /// `crates/typer/src/expand.rs` replaces the ones it can expand while the
    /// call site is typed; whatever is still a macro application here could
    /// not be expanded, and this is the sweep that guarantees it is an error
    /// rather than silently accepted -- the macro def has no bytecode, so the
    /// emitted class file would reference a method that does not exist.
    ///
    /// When the expander recorded *why* it gave up, that reason is part of the
    /// message: "cannot expand" with no reason at all was the phase-1 answer
    /// and is now only what an application nobody even tried gets.
    pub(crate) fn report_macro_calls(&mut self, tree: &Tree) {
        // Report the *macro application* — the outermost Apply/TypeApply — the
        // way nsc does, so `M.f(1)` is one error rather than one per node.
        if let Some(sym) = self.macro_symbol_of(tree) {
            let name = self.st.get(sym).name.clone();
            let binding = self.st.get(sym).macro_impl.clone().expect("macro symbol");
            let why = self
                .macro_failures
                .get(&self.macro_failure_key(tree.span))
                .cloned()
                .map(|w| format!(": {w}"))
                .unwrap_or_default();
            self.error(
                tree.span,
                format!(
                    "macro expansion is not implemented: cannot expand {name} \
                     (implementation {}.{}){why}. See docs/macros.md.",
                    binding.impl_class, binding.impl_method
                ),
            );
            return;
        }
        // A macro def's own rhs holds an unresolved reference to the
        // implementation, not a call to it.
        if matches!(tree.kind, TreeKind::MacroRhs { .. }) {
            return;
        }
        let mut kids: Vec<&Tree> = Vec::new();
        push_children(tree, &mut kids);
        for k in kids {
            self.report_macro_calls(k);
        }
    }

    /// Drop macro defs from the tree so the backend emits no method for them.
    ///
    /// nsc does the same: a macro def's body is `EmptyTree` and no JVM method
    /// is generated, which is why macros cannot be called from Java. Reaching
    /// here means no call site needed expanding, so the def is simply dead.
    ///
    /// The *symbol* stays in the table and is still pickled. Recording the
    /// binding in the pickle (nsc's `MACRO` flag plus `@macroImpl`) so that a
    /// separately compiled macro def can be expanded is phase 2; see
    /// `docs/macros.md` §5.
    pub(crate) fn strip_macro_defs(&self, tree: &mut Tree) {
        let is_macro_def = |t: &Tree| -> bool {
            matches!(&t.kind, TreeKind::DefDef { rhs, .. } if matches!(rhs.kind, TreeKind::MacroRhs { .. }))
        };
        match &mut tree.kind {
            TreeKind::PackageDef { stats, .. } => {
                stats.retain(|s| !is_macro_def(s));
                for s in stats {
                    self.strip_macro_defs(s);
                }
            }
            TreeKind::ClassDef { impl_, .. } | TreeKind::ModuleDef { impl_, .. } => {
                impl_.body.retain(|s| !is_macro_def(s));
                for s in &mut impl_.body {
                    self.strip_macro_defs(s);
                }
            }
            _ => {}
        }
    }

    /// The macro symbol this tree applies, if it is a macro application.
    pub(crate) fn macro_symbol_of(&self, tree: &Tree) -> Option<SymbolId> {
        let head = match &tree.kind {
            TreeKind::Apply { .. } | TreeKind::TypeApply { .. } => {
                let mut t = tree;
                while let TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } = &t.kind {
                    t = fun;
                }
                t
            }
            TreeKind::Select { .. } | TreeKind::Ident { .. } => tree,
            _ => return None,
        };
        let sym = if head.sym.is_none() {
            tree.sym
        } else {
            head.sym
        };
        if sym.is_none() {
            return None;
        }
        self.st.get(sym).macro_impl.as_ref().map(|_| sym)
    }
}

/// Collect every direct child tree of `t`.
///
/// The match is deliberately exhaustive with no wildcard arm: adding a
/// `TreeKind` variant must be a compile error here, so a macro application
/// nested inside new syntax can never be missed and silently emitted.
pub(crate) fn push_children<'a>(t: &'a Tree, out: &mut Vec<&'a Tree>) {
    fn all<'a>(v: &'a [Tree], out: &mut Vec<&'a Tree>) {
        out.extend(v.iter());
    }
    fn template<'a>(tp: &'a Template, out: &mut Vec<&'a Tree>) {
        all(&tp.parents, out);
        if let Some(st) = &tp.self_tpt {
            out.push(st);
        }
        all(&tp.body, out);
    }
    fn cases<'a>(cs: &'a [CaseDef], out: &mut Vec<&'a Tree>) {
        for c in cs {
            out.push(&c.pat);
            out.push(&c.guard);
            out.push(&c.body);
        }
    }
    match &t.kind {
        TreeKind::Empty
        | TreeKind::Super { .. }
        | TreeKind::This { .. }
        | TreeKind::Ident { .. }
        | TreeKind::Literal { .. }
        | TreeKind::Wildcard
        | TreeKind::Unimplemented { .. } => {}

        TreeKind::PackageDef { pid, stats } => {
            out.push(pid);
            all(stats, out);
        }
        TreeKind::Import { expr, .. } => out.push(expr),
        TreeKind::ClassDef {
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            all(tparams, out);
            for c in vparamss {
                all(c, out);
            }
            template(impl_, out);
        }
        TreeKind::ModuleDef { impl_, .. } => template(impl_, out),
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
            all(tparams, out);
            for c in vparamss {
                all(c, out);
            }
            out.push(tpt);
            out.push(rhs);
        }
        TreeKind::MacroRhs { impl_ref } => out.push(impl_ref),
        TreeKind::TypeDef {
            tparams,
            rhs,
            lo,
            hi,
            views,
            ctx_bounds,
            ..
        } => {
            all(tparams, out);
            out.push(rhs);
            if let Some(l) = lo {
                out.push(l);
            }
            if let Some(h) = hi {
                out.push(h);
            }
            all(views, out);
            all(ctx_bounds, out);
        }
        TreeKind::LabelDef { params, rhs, .. } => {
            all(params, out);
            out.push(rhs);
        }
        TreeKind::Block { stats, expr } => {
            all(stats, out);
            out.push(expr);
        }
        TreeKind::If { cond, thenp, elsep } => {
            out.push(cond);
            out.push(thenp);
            out.push(elsep);
        }
        TreeKind::Match {
            selector,
            cases: cs,
        } => {
            out.push(selector);
            cases(cs, out);
        }
        TreeKind::Function { vparams, body } => {
            all(vparams, out);
            out.push(body);
        }
        TreeKind::Assign { lhs, rhs } => {
            out.push(lhs);
            out.push(rhs);
        }
        TreeKind::While { cond, body } => {
            out.push(cond);
            out.push(body);
        }
        TreeKind::DoWhile { body, cond } => {
            out.push(body);
            out.push(cond);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => out.push(expr),
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            out.push(block);
            cases(catches, out);
            out.push(finalizer);
        }
        TreeKind::New { tpt } => out.push(tpt),
        TreeKind::Typed { expr, tpt } => {
            out.push(expr);
            out.push(tpt);
        }
        TreeKind::TypeApply { fun, args }
        | TreeKind::Apply { fun, args }
        | TreeKind::UnApply { fun, args } => {
            out.push(fun);
            all(args, out);
        }
        TreeKind::Select { qual, .. } => out.push(qual),
        TreeKind::Bind { body, .. } => out.push(body),
        TreeKind::Star { elem } => out.push(elem),
        TreeKind::Alternative { trees } => all(trees, out),
        TreeKind::AppliedTypeTree { tpt, args } => {
            out.push(tpt);
            all(args, out);
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
            all(parents, out);
            all(refinements, out);
        }
        TreeKind::ExistentialTypeTree { tpt, clauses } => {
            out.push(tpt);
            all(clauses, out);
        }
        TreeKind::InterpolatedString { args, .. } => all(args, out),
    }
}
