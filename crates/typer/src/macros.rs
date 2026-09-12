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
use crate::symbol::{MacroBinding, MacroPickle, MacroTarg, SymKind};

/// Fully-qualified names of the two macro `Context` types.
const BLACKBOX_CONTEXT: &str = "scala.reflect.macros.blackbox.Context";
const WHITEBOX_CONTEXT: &str = "scala.reflect.macros.whitebox.Context";

/// The implementation `def f(…): T = macro ???` names: `scala.Predef.???`.
///
/// nsc treats this reference as a *placeholder*, not as an implementation.
/// `scala/reflect/macros/compiler/Validators.scala` runs only the "confidence
/// check" on it (is it a public, non-overloaded method of a static object?) and
/// skips `checkMacroDefMacroImplCorrespondence` outright:
///
/// ```text
/// confidenceCheck()
/// if (macroImpl != Predef_???) checkMacroDefMacroImplCorrespondence()
/// ```
///
/// Every check the reference would fail -- a leading `Context` parameter, a
/// parameter list matching the definition's -- lives in that second half, so
/// `macro ???` is accepted at the *definition* site and rejected only when
/// someone calls it ("macro implementation is missing"). The library declares
/// its compiler-intrinsic macros this way (`StringContext.s/f/raw`,
/// `scala.reflect.materializeClassTag`); nsc fills them in from its own
/// `FastTrack` table, keyed on the macro *def* symbol.
pub(crate) const PLACEHOLDER_IMPL: (&str, &str) = ("scala.Predef$", "???");

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
///
/// The type arguments are **not** decoration and are not thrown away: they are
/// nsc's `MacroImplBinding.targs`, which decide what each `WeakTypeTag` the
/// implementation asks for stands for (`docs/macros.md` §7.22). They used to
/// be reduced to a count here.
fn split_type_apply(t: &Tree) -> (&Tree, &[Tree]) {
    match &t.kind {
        TreeKind::TypeApply { fun, args } => (fun, args),
        _ => (t, &[]),
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

/// Classify one already-resolved type argument of a macro implementation
/// reference, in the vocabulary [`MacroTarg`] uses.
///
/// Shared by the source path ([`Typer::classify_macro_targ`]) and the pickled
/// one (`crates/typer/src/pickle_supply.rs`), because the two differ only in
/// how the type is obtained: nsc writes the same `MacroImplBinding.targs`
/// either way.
/// The *head* symbol of a tag's type argument, as nsc's `targ.typeSymbol`
/// reads it: `F` for `F[Any]`, `F` for `F`, nothing for a concrete type.
///
/// `impl_tparams` is consulted by name as well as by identity, because a
/// method read back from a class file scala-rs wrote carries its parameter
/// types by simple name (`WeakTypeTag[F[Any]]` arrives as `Type::Named`).
fn tag_head_symbol(
    ty: &Type,
    impl_tparams: &[SymbolId],
    st: &crate::symbol::SymbolTable,
) -> Option<SymbolId> {
    match ty {
        Type::TypeParam(tp) => Some(*tp),
        Type::Applied { ctor, .. } => tag_head_symbol(ctor, impl_tparams, st),
        Type::Named { name, .. } => impl_tparams
            .iter()
            .copied()
            .find(|&t| st.get(t).name == *name),
        _ => None,
    }
}

pub(crate) fn macro_targ_of_type(
    st: &crate::symbol::SymbolTable,
    ty: &Type,
    def_sym: SymbolId,
) -> MacroTarg {
    if let Type::TypeParam(tp) = ty {
        let name = st.get(*tp).name.clone();
        let owner = st.get(*tp).owner;
        if owner == def_sym {
            if let Some(index) = st.get(def_sym).tparams.iter().position(|t| t == tp) {
                return MacroTarg::DefParam { index, name };
            }
        } else if !owner.is_none() {
            if let Some(index) = st.get(owner).tparams.iter().position(|t| t == tp) {
                return MacroTarg::OwnerParam { owner, index, name };
            }
        }
        return MacroTarg::Unresolved(name);
    }
    match ty {
        // A class with no type arguments of its own, and the primitives. nsc
        // takes the written type's *symbol* and then that symbol's own type,
        // which for these is the same type back again.
        Type::Class { args, .. } if args.is_empty() => MacroTarg::Fixed(ty.clone()),
        Type::Unit
        | Type::Boolean
        | Type::Byte
        | Type::Short
        | Type::Int
        | Type::Long
        | Type::Float
        | Type::Double
        | Type::Char
        | Type::String
        | Type::Any
        | Type::AnyRef
        | Type::AnyVal
        | Type::Null
        | Type::Nothing => MacroTarg::Fixed(ty.clone()),
        // Everything else, and an *applied* type constructor above all. That
        // last one is where nsc's own reading stops being a reading -- it
        // reduces `List[R]` to `List[A]`, `A` being `List`'s own type
        // parameter -- so scala-rs refuses it by name rather than reproducing a
        // type with a free parameter in it or substituting a different answer.
        // See [`MacroTarg::Unresolved`].
        _ => MacroTarg::Unresolved(st.display_type(ty)),
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

        if let Some(binding) = self.resolve_macro_impl(&impl_ref, tree.span, tree.sym) {
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
    fn resolve_macro_impl(
        &mut self,
        impl_ref: &Tree,
        span: Span,
        def_sym: SymbolId,
    ) -> Option<MacroBinding> {
        let (base, ref_targs) = split_type_apply(impl_ref);
        let ref_targs = ref_targs.to_vec();
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

        let impl_class = {
            let jvm = self.st.get(owner).jvm_name.clone();
            if jvm.is_empty() {
                self.st.get(owner).name.clone()
            } else {
                jvm.replace('/', ".")
            }
        };

        // `macro ???`. Everything below this point is the correspondence check
        // nsc skips for the placeholder (see [`PLACEHOLDER_IMPL`]), so the
        // binding is built here and the definition stands. It carries the
        // reference nsc pickles -- `scala.Predef$.???` with an empty signature,
        // `???` taking no parameter list -- so the `MACRO` flag still reaches
        // the class file and a separately compiled caller still sees a macro
        // rather than a method with no bytecode. `blackbox` is arbitrary: a
        // call site is refused before boxity can matter.
        if (impl_class.as_str(), name.as_str()) == PLACEHOLDER_IMPL {
            return Some(MacroBinding {
                pickle: Some(MacroPickle {
                    signature: Vec::new(),
                    targs: Vec::new(),
                }),
                impl_class,
                impl_method: name,
                blackbox: true,
                tag_params: 0,
                expr_args: Vec::new(),
                tag_targs: Vec::new(),
            });
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

        let tag_params = self.macro_impl_tag_params(sym);
        let pickle = self.macro_pickle_binding(sym, def_sym, &ref_targs, span)?;
        Some(MacroBinding {
            pickle: Some(pickle),
            impl_class,
            impl_method: name,
            blackbox,
            tag_params,
            expr_args: self.macro_impl_expr_args(sym),
            tag_targs: self.macro_ref_tag_targs(sym, def_sym, &ref_targs, tag_params),
        })
    }

    fn macro_pickle_binding(
        &mut self,
        impl_sym: SymbolId,
        def_sym: SymbolId,
        ref_targs: &[Tree],
        span: Span,
    ) -> Option<MacroPickle> {
        let implementation = self.st.get(impl_sym);
        let tparams = implementation.tparams.clone();
        // Binary method symbols may already be uncurried. Macro value clauses
        // must have the definition's shape; Context is a separate leading
        // clause and WeakTypeTags form the trailing implementation-only clause.
        let flat = self.macro_impl_params(impl_sym);
        let fingerprints: Vec<i32> = flat
            .iter()
            .map(|&p| {
                if self.is_tag_param(p) {
                    if let Some(index) = self.tag_param_tparam(p, &tparams) {
                        return index as i32;
                    }
                }
                let display = self.st.display_type(&self.st.get(p).ty);
                if display.contains("Expr") {
                    -2
                } else if display.contains("Tree") {
                    -3
                } else {
                    -1
                }
            })
            .collect();
        let sizes: Vec<usize> = match &self.st.get(def_sym).ty {
            Type::Method { paramss, .. } => paramss.iter().map(Vec::len).collect(),
            _ => Vec::new(),
        };
        let value_end = 1 + sizes.iter().sum::<usize>();
        if fingerprints.first() != Some(&-1)
            || value_end > fingerprints.len()
            || fingerprints[value_end..].iter().any(|f| *f < 0)
        {
            self.error(
                span,
                "macro implementation parameter shape does not match the macro definition",
            );
            return None;
        }
        let mut signature = vec![fingerprints[..1].to_vec()];
        let mut offset = 1;
        for size in sizes {
            let end = offset + size;
            signature.push(fingerprints[offset..end].to_vec());
            offset = end;
        }
        if offset < fingerprints.len() {
            signature.push(fingerprints[offset..].to_vec());
        }
        self.st.push_scope();
        let owner = self.st.get(def_sym).owner;
        for scope in [owner, def_sym] {
            if !scope.is_none() {
                for tp in self.st.get(scope).tparams.clone() {
                    let name = self.st.get(tp).name.clone();
                    self.st.enter_in_current(&name, tp);
                }
            }
        }
        let mark = self.diags.len();
        let targs: Vec<Type> = ref_targs.iter().map(|t| self.tree_to_type(t)).collect();
        self.diags.truncate(mark);
        self.st.pop_scope();
        if targs.iter().any(Type::is_error) {
            self.error(
                span,
                "cannot resolve macro implementation reference type arguments",
            );
            return None;
        }
        Some(MacroPickle { signature, targs })
    }

    /// What each tag the implementation asks for stands for, read off the
    /// type arguments written on the implementation *reference*.
    ///
    /// nsc's fingerprint for a tag parameter is the index of the
    /// implementation type parameter it is a tag for, and the reference's type
    /// arguments line up one for one with those (nsc refuses the definition
    /// otherwise: "macro implementation reference has too few type arguments").
    /// So the tag in position *j* of the trailing clause is resolved by asking
    /// which implementation type parameter its `WeakTypeTag[T]` names, and
    /// taking the reference's type argument at that index. The order of the
    /// trailing clause is *not* assumed to be the order of the type
    /// parameters: `(implicit uTag: c.WeakTypeTag[U], rTag: c.WeakTypeTag[R])`
    /// is legal and means the other thing.
    ///
    /// Returns an empty vector -- "not known", which leaves the older
    /// one-for-one rule in place -- when any part of that does not line up,
    /// rather than resolving some tags and guessing at the rest.
    fn macro_ref_tag_targs(
        &mut self,
        impl_sym: SymbolId,
        def_sym: SymbolId,
        ref_targs: &[Tree],
        tag_params: usize,
    ) -> Vec<MacroTarg> {
        if tag_params == 0 || def_sym.is_none() {
            return Vec::new();
        }
        let flat = self.macro_impl_params(impl_sym);
        let tags: Vec<SymbolId> = flat[flat.len() - tag_params..].to_vec();
        let impl_tparams = self.st.get(impl_sym).tparams.clone();
        let mut out = Vec::with_capacity(tag_params);
        for p in tags {
            let Some(index) = self.tag_param_tparam(p, &impl_tparams) else {
                return Vec::new();
            };
            let Some(written) = ref_targs.get(index) else {
                return Vec::new();
            };
            out.push(self.classify_macro_targ(written, def_sym));
        }
        out
    }

    /// `T` of a `c.WeakTypeTag[T]` parameter.
    fn tag_param_argument(&self, p: SymbolId) -> Option<Type> {
        match &self.st.get(p).ty {
            Type::Class { args, .. } | Type::Named { args, .. } | Type::Applied { args, .. }
                if args.len() == 1 =>
            {
                Some(args[0].clone())
            }
            _ => None,
        }
    }

    /// Which implementation type parameter a `c.WeakTypeTag[T]` is a tag *for*.
    ///
    /// nsc reads this off `targ.typeSymbol`
    /// (`Helpers.transformTypeTagEvidenceParams`), which looks through an
    /// application: `c.WeakTypeTag[F[Any]]` on `def lift[F[_], G[_]]` is a tag
    /// for `F`, and that is the index its fingerprint carries. cats' only
    /// macro is written that way --
    /// `(implicit evF: c.WeakTypeTag[F[Any]], evG: c.WeakTypeTag[G[Any]])` on
    /// `FunctionKMacros.lift` -- and reading only the bare-parameter form made
    /// the whole trailing clause look like ordinary implicit values, so the
    /// definition was refused with "macro implementation parameter shape does
    /// not match the macro definition".
    fn tag_param_tparam(&self, p: SymbolId, impl_tparams: &[SymbolId]) -> Option<usize> {
        let arg = self.tag_param_argument(p)?;
        let head = tag_head_symbol(&arg, impl_tparams, &self.st)?;
        impl_tparams.iter().position(|&t| t == head)
    }

    /// Decide what one type argument written on the implementation reference
    /// is, in the vocabulary [`MacroTarg`] uses.
    ///
    /// A bare name is matched against the macro def's own type parameters and
    /// then its owner's **by name**, which is both what nsc does
    /// (`macroDef.typeParams.indexWhere(_.name == targ.name)`) and the only
    /// thing available here: a macro def's right-hand side is resolved in the
    /// *enclosing* scope, with no parameter scope pushed -- the reference names
    /// a method of some other object and must not see `R` as anything -- so
    /// typing `R` here would report "not found: type R" and answer nothing.
    ///
    /// Anything else is typed, and the diagnostics that attempt raises are
    /// rolled back: a type argument this does not recognise is refused at the
    /// *call site* with a reason, not reported twice at the definition.
    fn classify_macro_targ(&mut self, written: &Tree, def_sym: SymbolId) -> MacroTarg {
        if let TreeKind::Ident { name } = &written.kind {
            if name != "_" && name != crate::materialize::RESOLVED_TYPE {
                let named =
                    |syms: &[SymbolId]| syms.iter().position(|&t| self.st.get(t).name == *name);
                if let Some(index) = named(&self.st.get(def_sym).tparams) {
                    return MacroTarg::DefParam {
                        index,
                        name: name.clone(),
                    };
                }
                let owner = self.st.get(def_sym).owner;
                if !owner.is_none() {
                    if let Some(index) = named(&self.st.get(owner).tparams) {
                        return MacroTarg::OwnerParam {
                            owner,
                            index,
                            name: name.clone(),
                        };
                    }
                }
            }
        }
        let probe = written.clone();
        let mark = self.diags.len();
        let ty = self.tree_to_type(&probe);
        self.diags.truncate(mark);
        macro_targ_of_type(&self.st, &ty, def_sym)
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
            // `def f(…): T = macro ???` has no implementation to run at all:
            // nsc accepts the definition and reports this at the call site
            // (`MacroImplementationNotFoundError`). The library's own
            // `StringContext.s` is one of these; nsc supplies it from
            // `FastTrack`, scala-rs from its interpolation path in the typer,
            // and a *user* macro declared this way is simply uncallable.
            if (binding.impl_class.as_str(), binding.impl_method.as_str()) == PLACEHOLDER_IMPL {
                self.error(tree.span, "macro implementation is missing");
                return;
            }
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
    /// binding in the pickle (nsc's `MACRO` flag plus `@macroImpl`) lets a
    /// separately compiled consumer expand it. The retained payload predates
    /// uncurry, so its implementation fingerprint keeps the proper clauses.
    pub(crate) fn strip_macro_defs(&self, tree: &mut Tree) {
        let is_macro_def = |t: &Tree| -> bool {
            matches!(&t.kind, TreeKind::DefDef { rhs, .. } if matches!(rhs.kind, TreeKind::MacroRhs { .. }))
        };
        match &mut tree.kind {
            TreeKind::PackageDef { stats, .. } | TreeKind::Block { stats, .. } => {
                stats.retain(|s| !is_macro_def(s));
            }
            TreeKind::ClassDef { impl_, .. } | TreeKind::ModuleDef { impl_, .. } => {
                impl_.body.retain(|s| !is_macro_def(s));
            }
            _ => {}
        }
        // Anonymous classes live below New/ValDef/Apply rather than directly
        // in a template's statement list. Their macro declarations also have
        // no runtime method body and must not reach bytecode generation.
        for child in crate::lazy_local::children_mut(tree) {
            self.strip_macro_defs(child);
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
