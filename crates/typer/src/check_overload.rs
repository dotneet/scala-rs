#![allow(dead_code)]
//! Overload resolution and the applicability test behind it.
//!
//! Given the alternatives of an overloaded symbol and the argument trees,
//! decides which alternative applies, and among those which is most specific.
//! Applicability is scored rather than answered yes/no, because an argument
//! may fit only after a lambda's parameter types are guessed from the
//! parameter it is passed to, after tupling, after a default is dropped, or
//! after an implicit view is applied. Function literals are typed here.

use crate::check::*;
use crate::implicits::ImplicitSearch;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;

/// nsc `Infer.shapeType`, as `Typers.preSelectOverloaded` uses it: what an
/// argument *tree* stands for before any alternative has been chosen, and so
/// before a function literal can have been typed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ArgShape {
    /// Anything that is not a function literal. Its own type is its shape.
    Value,
    /// `x => e` (annotated or not): `FunctionN[Any, …, Nothing]`.
    Function,
    /// `{ case … }`: `PartialFunction[Any, Nothing]`.
    CaseBlock,
}

/// A formal's type with the by-name / repeated / annotation wrappers off, so
/// that `=> PartialFunction[A, B]` and `PartialFunction[A, B]*` are recognised
/// as the `PartialFunction` formals they are.
fn strip_param_wrappers(ty: &Type) -> &Type {
    match strip_annotations(ty) {
        Type::ByName(inner) | Type::Repeated(inner) => strip_param_wrappers(inner),
        other => other,
    }
}

/// [`ArgShape`] for each argument of one application.
pub(crate) fn arg_shapes(args: &[Tree]) -> Vec<ArgShape> {
    args.iter()
        .map(|a| match &a.kind {
            TreeKind::Function { vparams, body } => {
                if is_case_block_literal(vparams, body) {
                    ArgShape::CaseBlock
                } else {
                    ArgShape::Function
                }
            }
            _ => ArgShape::Value,
        })
        .collect()
}

impl Typer {
    /// The single overloaded alternative of `sym` that takes `n` type
    /// parameters, if there is exactly one.
    /// The alternative an explicit `[T1, …, Tn]` picks out of an overload set
    /// that value position has already collapsed.
    ///
    /// Only fires when the set really had more than one alternative at this
    /// receiver (`overload_member_types`, recorded by the selection), exactly
    /// one of which takes `n` type parameters, and the symbol the collapse
    /// left behind takes a different number. So an ordinary
    /// `Ordering[String]`, whose `Ordering` is a single accessor, is untouched
    /// and still redirects to the module's `apply`.
    pub(crate) fn alt_taking_targs(&self, sym: SymbolId, n: usize) -> Option<(SymbolId, Type)> {
        if n == 0 || sym.is_none() || self.st.get(sym).tparams.len() == n {
            return None;
        }
        let alts = self.overload_member_types.get(&sym.0)?;
        if alts.len() < 2 || !alts.iter().any(|(s, _)| *s == sym) {
            return None;
        }
        let mut hits = alts.iter().filter(|(s, t)| {
            self.st.get(*s).tparams.len() == n && matches!(t, Type::Method { .. })
        });
        let first = hits.next()?;
        hits.next().is_none().then(|| (first.0, first.1.clone()))
    }

    /// The alternative nsc's `inferExprAlternative` picks for `x.m[T1, …, Tn]`
    /// in value position, when value position has already collapsed the set to
    /// its nullary alternative but it also holds one whose only clause is
    /// implicit (and that takes the same `n` type parameters).
    ///
    /// cats declares exactly that pair on `Alternative`: `MonoidK.compose[G[_]]:
    /// MonoidK[λ[α => F[G[α]]]]` beside `Alternative.compose[G[_]:
    /// Applicative]: Alternative[λ[α => F[G[α]]]]`. The nullary one is not
    /// "the" value-position alternative: nsc's `isAsSpecific` looks straight
    /// through an implicit clause, so the two are compared by their results
    /// once the type arguments are in, and the owner that is a proper subclass
    /// scores a point too (`isStrictlyMoreSpecific`). An alternative the
    /// expected type rules out and the other does not loses first.
    /// `Alternative[F].compose[G]` is therefore the `Alternative`, and at a
    /// declared `MonoidK[…]` it still is -- scalac then reports the missing
    /// `Applicative[G]` instead of quietly taking the `MonoidK`.
    ///
    /// `None` leaves the collapse as it was, including the tie nsc would
    /// report as ambiguous.
    pub(crate) fn implicit_alt_beats_nullary(
        &self,
        fun: &Tree,
        targs: &[Type],
        pt: &Type,
    ) -> Option<(SymbolId, Type)> {
        let sym = fun.sym;
        if sym.is_none() || !self.is_nullary_method_sym(sym) {
            return None;
        }
        // `overload_member_types` is keyed by symbol, not by selection: the set
        // recorded when `Alternative[F].compose` collapsed to `MonoidK.compose`
        // is still filed under that symbol when a plain `MonoidK[F].compose`
        // selects it later. Only a receiver that really has the implicit
        // alternative as a member may be steered to it.
        let TreeKind::Select { qual, .. } = &fun.kind else {
            return None;
        };
        let recv = self.st.class_sym_of(&self.st.widen_type_param(&qual.ty))?;
        let alts = self.overload_member_types.get(&sym.0)?;
        let nullary = alts.iter().find(|(s, _)| *s == sym)?.1.clone();
        let implicit_only = |s: SymbolId| {
            let decl = &self.st.get(s).paramss;
            decl.len() == 1
                && !decl[0].is_empty()
                && decl[0]
                    .iter()
                    .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
        };
        let mut hits = alts.iter().filter(|(s, t)| {
            *s != sym
                && self.st.get(*s).tparams.len() == targs.len()
                && matches!(t, Type::Method { .. })
                && implicit_only(*s)
        });
        let (isym, ity) = hits.next()?;
        if hits.next().is_some() || self.st.get(sym).tparams.len() != targs.len() {
            return None;
        }
        let owner_i = self.st.get(*isym).owner;
        if owner_i != recv && !self.st.is_ancestor_of(owner_i, recv) {
            return None;
        }
        let result = |s: SymbolId, t: &Type| {
            let t = self.st.subst_tparams(s, targs, t);
            match t {
                Type::Method { ret, .. } => *ret,
                other => other,
            }
        };
        let r_imp = result(*isym, ity);
        let r_nul = result(sym, &nullary);
        if r_imp.is_error() || r_nul.is_error() {
            return None;
        }
        let value_pt = !matches!(
            pt,
            Type::NoType
                | Type::Error
                | Type::Wildcard
                | Type::Method { .. }
                | Type::Function { .. }
        );
        if value_pt {
            let imp_fits = self.st.is_sub_type(&r_imp, pt);
            let nul_fits = self.st.is_sub_type(&r_nul, pt);
            if imp_fits != nul_fits {
                return imp_fits.then(|| (*isym, ity.clone()));
            }
        }
        let owner_n = self.st.get(sym).owner;
        let mut score = 0i32;
        if self.st.is_sub_type(&r_imp, &r_nul) {
            score += 1;
        }
        if self.st.is_sub_type(&r_nul, &r_imp) {
            score -= 1;
        }
        if owner_i != owner_n && self.st.is_ancestor_of(owner_n, owner_i) {
            score += 1;
        }
        if owner_i != owner_n && self.st.is_ancestor_of(owner_i, owner_n) {
            score -= 1;
        }
        (score > 0).then(|| (*isym, ity.clone()))
    }

    /// `chosen`'s declared type, as seen from the receiver of `fun`, with its
    /// own type parameters still un-instantiated.
    ///
    /// The selection that produced `fun` already did this work for every
    /// alternative and filed it under `group_key` (`overload_member_types`), so
    /// that is the first place to look; a symbol reached some other way is
    /// substituted here from the qualifier's type. The raw declaration is the
    /// last resort, and is right only for a member of a non-generic owner.
    pub(crate) fn member_ty_as_seen_from(
        &self,
        group_key: SymbolId,
        chosen: SymbolId,
        fun: &Tree,
    ) -> Type {
        if let Some(alts) = self.overload_member_types.get(&group_key.0) {
            if let Some((_, t)) = alts.iter().find(|(s, _)| *s == chosen) {
                return t.clone();
            }
        }
        let raw = self.st.get(chosen).ty.clone();
        if let TreeKind::Select { qual, .. } = &fun.kind {
            if !matches!(qual.ty, Type::NoType | Type::Error) {
                let t = self.st.subst_as_seen_from(&qual.ty, &raw);
                return self.st.expand_in_type(&qual.ty, &t);
            }
        }
        raw
    }

    pub(crate) fn only_alt_with_tparams(&self, sym: SymbolId, n: usize) -> Option<SymbolId> {
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

    /// Rewrite `fun` from `m` to `m.apply` when `m` is a parameterless method
    /// whose result type has an `apply` member.
    ///
    /// An *overload set* counts too, as long as exactly one alternative is
    /// value-shaped. `scala.reflect`'s tree factories are written that way --
    /// `val Ident: IdentExtractor` next to `def Ident(name: String): Ident`,
    /// `val Bind: BindExtractor` next to `def Bind(sym: Symbol, body: Tree)`,
    /// and the same for `This` and `New` -- so `Ident(TermName("x"))` matches
    /// no alternative and is `Ident.apply(TermName("x"))`. Without this every
    /// one of them was rejected, `TableQuery`'s macro implementation among
    /// them.
    ///
    /// Returns `false` — leaving `fun` untouched — for anything else, so the
    /// caller reports the original failure.
    pub(crate) fn insert_apply_on_nullary(&mut self, fun: &mut Tree) -> bool {
        if self.apply_function_value_alternative(fun) {
            return true;
        }
        let ret = match fun.ty.clone() {
            Type::Method { paramss, ret }
                if paramss.is_empty() || paramss.iter().all(|c| c.is_empty()) =>
            {
                ret
            }
            Type::Overload(alts) => {
                let mut vals = alts.iter().filter_map(|a| match a {
                    Type::Method { paramss, ret }
                        if paramss.is_empty() || paramss.iter().all(|c| c.is_empty()) =>
                    {
                        Some((**ret).clone())
                    }
                    Type::Class { .. } | Type::ModuleRef(_) => Some(a.clone()),
                    _ => None,
                });
                let Some(only) = vals.next() else {
                    return false;
                };
                if vals.next().is_some() {
                    return false;
                }
                Box::new(only)
            }
            _ => return false,
        };
        if matches!(*ret, Type::Array(_)) {
            // A JVM zero-arg descriptor does not tell `def value` from
            // `def next()`. Keep the source clause identity or a matching
            // Scala property; Java/approximate prelude methods are not getters.
            let sym = self.st.get(fun.sym);
            let owner = self.st.get(sym.owner);
            let is_getter = sym.flags.contains(Flags::ACCESSOR)
                || owner.members.iter().any(|&id| {
                    let field = self.st.get(id);
                    field.kind == SymKind::Term
                        && field.name == sym.name
                        && (field.via_accessor
                            || field.flags.contains(Flags::LAZY)
                            || owner.ctor_fields.contains(&id))
                        && matches!(&sym.ty, Type::Method { ret, .. } if **ret == field.ty)
                });
            if sym.parameterless_method != Some(true) && !is_getter {
                return false;
            }
        } else if !matches!(*ret, Type::Class { .. } | Type::ModuleRef(_)) {
            return false;
        }
        // A Scala method with an explicit empty clause must be called before
        // applying its result, even when that result is an ordinary class.
        if self.st.get(fun.sym).parameterless_method == Some(false) {
            return false;
        }
        self.ensure_apply_supplied(&ret, fun.span);
        let Some(cls) = self.st.class_sym_of(&ret) else {
            return false;
        };
        if self.st.lookup_member(cls, "apply").is_empty() {
            return false;
        }
        // The receiver keeps its own symbol and span; only its type is the
        // auto-applied one, which is exactly the shape `maybe_auto_apply`
        // produces in value position, so the backend still emits the
        // parameterless call before the `apply`.
        let mut inner = fun.clone();
        inner.ty = (*ret).clone();
        // The receiver's own type parameters are this call's to solve, not
        // fixed types. `IorT.liftF(fb) = right(fb)` reaches
        // `RightPartiallyApplied[A].apply` through `def right[A]`, whose `A`
        // nothing has pinned yet; the enclosing expected type is what says
        // what it is. See `record_open_tparams`.
        self.record_open_tparams(fun.sym, &ret);
        let span = fun.span;
        *fun = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(inner),
                name: "apply".into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
            byname_thunk: false,
            byname_type_marker: false,
        };
        self.type_select(fun, &Type::NoType);
        !fun.ty.is_error() && !fun.ty.is_no_type()
    }

    /// An overload set whose only value-shaped alternative is a function-typed
    /// `val`, applied to arguments no method alternative takes: nsc's
    /// `followApply` makes the value an alternative through its `apply`.
    ///
    /// ```scala
    /// val h: Any => String = _ => "value"
    /// def h(s: String): String = "method"
    /// h(1)   // the value: h.apply(1)
    /// ```
    ///
    /// scala-rs offered only the method and reported "no matching overload".
    /// The receiver is rebuilt on the value's own symbol: the overloaded
    /// reference carries the group's first alternative, which may be the
    /// method.
    fn apply_function_value_alternative(&mut self, fun: &mut Tree) -> bool {
        let Type::Overload(alts) = &fun.ty else {
            return false;
        };
        if fun.sym.is_none()
            || !matches!(fun.kind, TreeKind::Ident { .. } | TreeKind::Select { .. })
        {
            return false;
        }
        let value_shaped = alts
            .iter()
            .filter(|a| match a {
                Type::Method { paramss, .. } => {
                    paramss.is_empty() || paramss.iter().all(|c| c.is_empty())
                }
                Type::Class { .. } | Type::ModuleRef(_) | Type::Function { .. } => true,
                _ => false,
            })
            .count();
        let fns: Vec<&Type> = alts
            .iter()
            .filter(|a| matches!(a, Type::Function { .. }))
            .collect();
        let ([fn_ty], 1) = (fns.as_slice(), value_shaped) else {
            return false;
        };
        let fn_ty = (*fn_ty).clone();
        let Some(group) = self.overload_member_types.get(&fun.sym.0) else {
            return false;
        };
        let mut owners = group
            .iter()
            .filter(|(s, t)| *t == fn_ty && self.st.get(*s).kind == SymKind::Term);
        let (Some((vsym, _)), None) = (owners.next(), owners.next()) else {
            return false;
        };
        let vsym = *vsym;
        let mut inner = fun.clone();
        inner.sym = vsym;
        inner.ty = fn_ty;
        let span = fun.span;
        let saved = fun.clone();
        *fun = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(inner),
                name: "apply".into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
            byname_thunk: false,
            byname_type_marker: false,
        };
        self.type_select(fun, &Type::NoType);
        if fun.ty.is_error() || fun.ty.is_no_type() {
            *fun = saved;
            return false;
        }
        true
    }

    /// An overloaded module member competes through its apply methods. Once
    /// one wins, preserve the original receiver in `receiver.module.apply`.
    pub(crate) fn select_overloaded_module_apply(&mut self, fun: &mut Tree, chosen: SymbolId) {
        if chosen.is_none() || !matches!(fun.ty, Type::Overload(_)) {
            return;
        }
        let Some(alts) = self.overload_member_types.get(&fun.sym.0) else {
            return;
        };
        let module = alts.iter().find_map(|(sym, ty)| {
            if let Type::ModuleRef(owner) = ty {
                if self.st.lookup_member(*owner, "apply").contains(&chosen) {
                    return Some((*sym, ty.clone()));
                }
            }
            None
        });
        if let Some((sym, ty)) = module {
            let mut inner = fun.clone();
            inner.sym = sym;
            inner.ty = ty;
            fun.kind = TreeKind::Select {
                qual: Box::new(inner),
                name: "apply".into(),
            };
            fun.sym = chosen;
            fun.ty = self.st.get(chosen).ty.clone();
        }
    }

    /// Make sure a value used as a function has whatever `apply` its pickle
    /// declares before overloads are weighed.
    ///
    /// `u.Constant(1)` selects a `Constants.ConstantExtractor`, a library
    /// class the prelude never declares, and then applies it. The receiver's
    /// members are only ever fetched by `type_select`, which never runs for
    /// the implied `.apply`, so the extractor looked empty and every
    /// `Literal(Constant(x))` in the reflection API was rejected.
    pub(crate) fn ensure_apply_supplied(&mut self, fun_ty: &Type, span: scala_rs_span::Span) {
        // A parameterless accessor can return a module just as it can
        // return an ordinary class instance. Both need their apply members
        // completed before insert_apply_on_nullary decides whether to select.
        if let Type::Overload(alts) = fun_ty {
            for alt in alts {
                self.ensure_apply_supplied(alt, span);
            }
            return;
        }
        if !matches!(fun_ty, Type::Class { .. } | Type::ModuleRef(_)) {
            return;
        }
        let Some(cls) = self.st.class_sym_of(fun_ty) else {
            return;
        };
        if !self.st.lookup_member(cls, "apply").is_empty() {
            return;
        }
        // Nested binary classes may have no pickle index until their own
        // classfile has been loaded. Use the ordinary selection supply path.
        self.ensure_classfile_members_loaded(cls, "apply", span);
        self.supply_from_pickle(fun_ty, "apply");
    }

    pub(crate) fn resolve_overload(
        &self,
        fun_ty: &Type,
        fun_sym: SymbolId,
        arg_tys: &[Type],
        pt: &Type,
    ) -> OverloadPick {
        self.resolve_overload_inner(fun_ty, fun_sym, arg_tys, pt, None, &[], &[])
    }

    /// [`Self::resolve_overload`], with the arguments' [`ArgShape`]s.
    ///
    /// A caller that still has the argument *trees* can say what nsc's
    /// `shapeType` would: whether a literal is `{ case … }` or not is the
    /// difference between a `PartialFunction[Any, Nothing]` shape and a
    /// `FunctionN[Any, …, Nothing]` one, and `preSelectOverloaded` throws out
    /// alternatives on it.
    pub(crate) fn resolve_overload_shaped(
        &self,
        fun_ty: &Type,
        fun_sym: SymbolId,
        arg_tys: &[Type],
        pt: &Type,
        shapes: &[ArgShape],
    ) -> OverloadPick {
        self.resolve_overload_inner(fun_ty, fun_sym, arg_tys, pt, None, shapes, &[])
    }

    /// [`Self::resolve_overload_shaped`], with the type arguments the caller
    /// *wrote* on the callee.
    ///
    /// nsc applies those to every alternative before applicability is weighed
    /// (`Infer.inferPolyAlternatives`, SLS 6.26.3: explicit type arguments
    /// *are* the instantiation). Here they used to reach the call only after
    /// an alternative had been picked, so each alternative was instantiated by
    /// inferring from the value arguments alone -- and an argument that
    /// disagrees with the written argument sank the whole set.
    pub(crate) fn resolve_overload_targs(
        &self,
        fun_ty: &Type,
        fun_sym: SymbolId,
        arg_tys: &[Type],
        pt: &Type,
        shapes: &[ArgShape],
        targs: &[Type],
    ) -> OverloadPick {
        self.resolve_overload_inner(fun_ty, fun_sym, arg_tys, pt, None, shapes, targs)
    }

    /// [`Self::resolve_overload`], with the alternatives' types supplied by the
    /// caller rather than looked up in `overload_member_types`.
    ///
    /// A constructor group is read at the type arguments the parent is applied
    /// at (`extends SortedMapEq[K, V]()(V)`), and only `pick_ctor_at` knows
    /// what those are.
    pub(crate) fn resolve_overload_with(
        &self,
        fun_ty: &Type,
        fun_sym: SymbolId,
        arg_tys: &[Type],
        pt: &Type,
        supplied: Option<&Vec<(SymbolId, Type)>>,
    ) -> OverloadPick {
        self.resolve_overload_inner(fun_ty, fun_sym, arg_tys, pt, supplied, &[], &[])
    }

    fn resolve_overload_inner(
        &self,
        fun_ty: &Type,
        fun_sym: SymbolId,
        arg_tys: &[Type],
        _pt: &Type,
        supplied: Option<&Vec<(SymbolId, Type)>>,
        shapes: &[ArgShape],
        targs: &[Type],
    ) -> OverloadPick {
        let mut cands: Vec<(SymbolId, Vec<Type>, Type)> = Vec::new();
        let mut module_apply_candidates = Vec::new();
        // An inner class behind a prefix (`prefix.rs`) is applied as the
        // class -- `Literal(c)` on `val Literal: Trees.this.LiteralExtractor`
        // -- while its `apply` is still read through the view, which is what
        // instantiates the enclosing class's parameters.
        let fun_ty_full = fun_ty;
        let stripped;
        let fun_ty = if crate::prefix::view_prefix(fun_ty).is_some() {
            stripped = crate::prefix::strip_view(fun_ty).clone();
            &stripped
        } else {
            fun_ty
        };
        // Value alternatives of function type (`val f: String => Int` beside
        // `def f(s: String)`): nsc's `followApply` makes them compete through
        // their `apply`. Only used to detect a tie below.
        let mut function_values: Vec<(SymbolId, Vec<Type>, Type)> = Vec::new();
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
                    cands.clear();
                    let mut methods =
                        self.drop_overridden(self.overload_alternatives(fun_sym, &name));
                    // Constructors are not inherited. `overload_alternatives`
                    // ends in `lookup_member`, which walks the parents, so
                    // `java.util.Properties`'s alternatives came back carrying
                    // `Hashtable`'s `(Int, Float)` and `(Map[_ <: K, _ <: V])`
                    // as well. `new Properties(null)` was then `ambiguous
                    // overload for constructor` between `Properties(Properties)`
                    // and `Hashtable(Map)` -- nsc only ever sees the class's
                    // own. `pick_ctor_at` already filters this way before
                    // building the `Overload`; this branch rebuilt the list
                    // from the symbol and lost the filter.
                    //
                    // What is dropped is a constructor whose owner is a proper
                    // *superclass*, not everything the owner does not declare
                    // itself: the same classfile can reach this table twice,
                    // and `java.io.OutputStreamWriter` does -- with only one of
                    // the two copies' `OutputStream` being the one
                    // `System.out`'s `PrintStream` extends. Demanding the exact
                    // owner threw the working copy away and turned
                    // `new OutputStreamWriter(System.out)` into a `no matching
                    // overload`.
                    if name == "<init>" {
                        methods.retain(|&m| !self.owner_is_proper_subclass(fun_sym, m));
                    }
                    // The declaration on the symbol is written in its own
                    // owner's type parameters; the selection already worked
                    // out what each alternative looks like at this receiver,
                    // and that is what specificity has to compare.
                    let instantiated =
                        supplied.or_else(|| self.overload_member_types.get(&fun_sym.0));
                    for m in methods {
                        let ty = instantiated
                            .and_then(|g| g.iter().find(|(s, _)| *s == m).map(|(_, t)| t))
                            .unwrap_or(&self.st.get(m).ty);
                        if let Type::Method { paramss, ret } = ty {
                            cands.push((
                                m,
                                paramss.first().cloned().unwrap_or_default(),
                                (**ret).clone(),
                            ));
                        }
                        if let Type::Function { params, ret } = ty {
                            if self.st.get(m).kind == SymKind::Term {
                                function_values.push((m, params.clone(), (**ret).clone()));
                            }
                        }
                        if let Type::ModuleRef(module) = ty {
                            for apply in
                                self.drop_overridden(self.st.lookup_member(*module, "apply"))
                            {
                                let apply_ty =
                                    self.st.subst_as_seen_from(ty, &self.st.get(apply).ty);
                                if let Type::Method { paramss, ret } = apply_ty {
                                    module_apply_candidates.push(apply);
                                    cands.push((
                                        apply,
                                        paramss.first().cloned().unwrap_or_default(),
                                        *ret,
                                    ));
                                }
                            }
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
            // A self alias (`class C { self => ... self(i) ... }`) types as
            // `C.this.type`, and the `apply` that `self(i)` means is the
            // class's own. Only the `Select` path widened a `this.type` to the
            // class; the application path stopped at `_ => None` and reported
            // `value apply is not a member of C.this.type`
            // (slick `util/ConstArray.scala`'s `def apply(idx: Int) = self(idx)`).
            Type::ThisType(sym) => {
                let args: Vec<Type> = self
                    .st
                    .get(*sym)
                    .tparams
                    .iter()
                    .map(|t| Type::TypeParam(*t))
                    .collect();
                let cls = Type::Class { sym: *sym, args };
                return self
                    .resolve_overload_inner(&cls, fun_sym, arg_tys, _pt, supplied, shapes, targs);
            }
            Type::Class { sym, .. } => {
                // `drop_overridden`, as everywhere else a member is looked up
                // by name: a case class's companion declares `apply(String): C`
                // *and* inherits the abstract `Function1.apply(T1): R` through
                // `scala.runtime.AbstractFunction1`, and the second is the
                // first, not a second alternative.
                //
                // `not_inherited_static`: scalac mirrors every companion
                // member onto the class's own file as a static forwarder, and
                // a class file reader installs it like a real member --
                // inherited by a subclass exactly like anything else, which a
                // *static* member never is (nsc: "static Java members belong
                // to companion objects; they are not inherited", and the same
                // holds for this bytecode-only mirror). Selecting through the
                // exact class it is declared on is what the forwarder is for
                // (`cats.effect.IO.apply(...)`, read this way before the
                // on-demand pickle path gets a chance to refine the erased
                // by-name signature); reaching it only by inheriting from an
                // ancestor is not (a Twirl `object` extending the case class
                // `BaseScalaTemplate` inherited its companion's static
                // `apply` alongside its own written one and could not tell
                // the two apart -- `docs/gitbucket.md` root 26).
                let apply = self
                    .drop_overridden(self.st.lookup_member(*sym, "apply"))
                    .into_iter()
                    .filter(|&m| not_inherited_static(&self.st, m, *sym))
                    .collect::<Vec<_>>();
                for m in apply {
                    // As seen from the receiver, like `type_select`: an
                    // `apply` inherited from a generic parent is only itself
                    // once the receiver's arguments are in. `trait Mono
                    // extends (Int => String)` gets `Function1.apply`, and
                    // reading it raw made `m(3)` report
                    // `found: 3  required: T1`.
                    let mty = self.st.subst_as_seen_from(fun_ty_full, &self.st.get(m).ty);
                    if let Type::Method { paramss, ret } = &mty {
                        cands.push((
                            m,
                            paramss.first().cloned().unwrap_or_default(),
                            (**ret).clone(),
                        ));
                    }
                }
            }
            Type::ModuleRef(id) => {
                let apply = self
                    .drop_overridden(self.st.lookup_member(*id, "apply"))
                    .into_iter()
                    .filter(|&m| not_inherited_static(&self.st, m, *id));
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
            Type::Array(elem) => {
                for m in self.st.lookup_member(self.st.array_sym, "apply") {
                    cands.push((m, vec![Type::Int], (**elem).clone()));
                }
            }
            _ => return OverloadPick::None,
        }
        let single_candidate = cands.len() == 1;
        let (applicable, with_views): (Vec<(SymbolId, Vec<Type>, Type)>, bool) = {
            let no_view: Vec<_> = cands
                .iter()
                .filter(|(sym, ps, ret)| {
                    self.is_applicable(
                        *sym,
                        clause,
                        ps,
                        arg_tys,
                        false,
                        targs,
                        single_candidate.then_some((ret, _pt)),
                    )
                })
                .cloned()
                .collect();
            if !no_view.is_empty() {
                (no_view, false)
            } else {
                (
                    cands
                        .into_iter()
                        .filter(|(sym, ps, ret)| {
                            self.is_applicable(
                                *sym,
                                clause,
                                ps,
                                arg_tys,
                                true,
                                targs,
                                single_candidate.then_some((ret, _pt)),
                            )
                        })
                        .collect(),
                    true,
                )
            }
        };
        let applicable = self.narrow_by_lambda_shape(applicable, arg_tys, shapes);
        // A value expectation eliminates an unapplied explicit clause when
        // an alternative with the same first domain supplies that clause
        // implicitly. FUNmode keeps both: another written application cannot
        // use its arguments to disambiguate the preceding clause.
        let applicable = if !matches!(_pt, Type::Method { .. } | Type::Function { .. }) {
            applicable
                .iter()
                .filter(|a| {
                    if a.0.is_none()
                        || self.st.get(a.0).paramss.len() != 2
                        || self.residual_clause_is_implicit(a.0)
                    {
                        return true;
                    }
                    !applicable.iter().any(|b| {
                        self.residual_clause_is_implicit(b.0)
                            && self.canonical_sig(a.0, &a.1, &Type::NoType)
                                == self.canonical_sig(b.0, &b.1, &Type::NoType)
                    })
                })
                .cloned()
                .collect()
        } else {
            applicable
        };
        // nsc fills a default only when no alternative applies without one
        // (`Infer.inferMethodAlternative`). Applied before the specificity
        // comparison, because the two alternatives it separates are often
        // equally specific on the arguments actually written -- which is
        // exactly how `new Prefer(1)` came out `ambiguous overload for
        // constructor` rather than the primary.
        let applicable = if applicable.len() > 1 {
            let no_default: Vec<(SymbolId, Vec<Type>, Type)> = applicable
                .iter()
                .filter(|(sym, ps, _)| !self.needs_a_default(*sym, clause, arg_tys.len(), ps.len()))
                .cloned()
                .collect();
            if no_default.is_empty() {
                applicable
            } else {
                no_default
            }
        } else {
            applicable
        };
        match applicable.len() {
            0 => OverloadPick::None,
            1 => {
                let (s, p, r) = applicable.into_iter().next().unwrap();
                if self.function_value_ties(
                    &(s, p.clone(), r.clone()),
                    &function_values,
                    arg_tys,
                    with_views,
                ) {
                    return OverloadPick::Ambiguous;
                }
                OverloadPick::Found(s, p, r)
            }
            _ => {
                let winners: Vec<(SymbolId, Vec<Type>, Type)> = applicable
                    .iter()
                    .filter(|a| {
                        applicable.iter().all(|b| {
                            a.0 == b.0
                                || self.is_as_specific_method(a.0, b.0, &a.1, &b.1, with_views)
                        })
                    })
                    .cloned()
                    .collect();
                // The same method can reach us by two routes -- a package and
                // its package object both carry `math.max`. Identical
                // signatures are one alternative, not an ambiguity.
                let mut winners = winners;
                winners.dedup_by(|a, b| {
                    module_apply_candidates.contains(&a.0) == module_apply_candidates.contains(&b.0)
                        && self.st.get(a.0).name == self.st.get(b.0).name
                        && a.1 == b.1
                        && a.2 == b.2
                        && self.overload_residual_key(a.0) == self.overload_residual_key(b.0)
                });
                // The same test again, but blind to *which* symbols a
                // candidate's own type parameters are. `mutable.HashMap`
                // reaches `getOrElse[V1 >: V](K, => V1): V1` twice -- once as
                // the prelude's declaration on `mutable.Map`, once as the
                // pickled `collection.MapOps` one adopted off the jar -- and
                // the two `V1`s are different symbols, so `==` above saw two
                // alternatives where nsc sees one member. Renaming one side's
                // type parameters to the other's makes the comparison the one
                // that was meant. The first survivor is kept, which is the one
                // `lookup_member` reached first, i.e. nearest the receiver.
                if winners.len() > 1 {
                    let keyed: Vec<Vec<Type>> = winners
                        .iter()
                        .map(|(s, ps, r)| self.canonical_sig(*s, ps, r))
                        .collect();
                    let mut seen = Vec::new();
                    let mut i = 0;
                    winners.retain(|(sym, _, _)| {
                        let k = (
                            module_apply_candidates.contains(sym),
                            self.st.get(*sym).name.clone(),
                            keyed[i].clone(),
                            self.overload_residual_key(*sym),
                        );
                        i += 1;
                        if seen.contains(&k) {
                            false
                        } else {
                            seen.push(k.clone());
                            true
                        }
                    });
                }
                // nsc `isStrictlyMoreSpecific`: when neither signature is more
                // specific than the other, the one whose *owner* is the proper
                // subclass wins (`relativeWeight`'s `isInProperSubClassOf`).
                // 2.13 declares `map[B](f)(implicit Ordering[B])` on
                // `SortedSetOps` and `map[B](f)` on `IterableOps`, and both are
                // applicable to a one-argument call: without the owners' own
                // subclass relation every `TreeSet.map(f)` was `ambiguous
                // overload`.
                if winners.len() > 1 {
                    let sub: Vec<(SymbolId, Vec<Type>, Type)> = winners
                        .iter()
                        .filter(|a| {
                            winners
                                .iter()
                                .all(|b| a.0 == b.0 || self.owner_is_proper_subclass(a.0, b.0))
                        })
                        .cloned()
                        .collect();
                    if sub.len() == 1 {
                        winners = sub;
                    }
                }
                // A prelude stand-in is not a second overload of the member it
                // stands in for.
                //
                // `prelude_coll.rs` writes `Set.map(A => Any): Set[Any]` and
                // `Map.+((K, Any)): Map[K, Any]` by hand -- monomorphic
                // approximations of members the real jar declares
                // polymorphically on `IterableOps` / `MapOps`. A receiver that
                // reaches *both* (`immutable.HashSet`, `immutable.HashMap`:
                // the pickled ops traits above them, the prelude's `Set` /
                // `Map` beside them) offered two alternatives that no rule
                // above can separate -- neither owner is the other's subclass,
                // and each is as specific as the other, since `A => B`
                // conforms to `A => Any` and `map[B]` is applicable with
                // `B = Any`. Every `HashSet.map(f)` and `HashMap + kv` was
                // `ambiguous overload`.
                //
                // nsc sees one member here, and it is the jar's. Keep it.
                // Scoped to the ambiguity: with one alternative already
                // chosen, nothing changes.
                if winners.len() > 1 {
                    let from_jar: Vec<(SymbolId, Vec<Type>, Type)> = winners
                        .iter()
                        .filter(|a| !self.st.get(a.0).pickled_origin.is_empty())
                        .cloned()
                        .collect();
                    let stand_ins = winners.iter().filter(|a| {
                        a.0 .0 < self.st.prelude_end && self.st.get(a.0).pickled_origin.is_empty()
                    });
                    if from_jar.len() == 1 && stand_ins.count() == winners.len() - 1 {
                        winners = from_jar;
                    }
                }
                // Applicability can solve receiver variables while comparing
                // signatures. Break that tie only when the monomorphic domain
                // is strictly narrower than each rigid polymorphic domain.
                // Equal domains (for example `1` and `A <: 1`) stay ambiguous.
                if winners.len() > 1 {
                    let mono: Vec<_> = winners
                        .iter()
                        .filter(|a| !a.0.is_none() && self.st.get(a.0).tparams.is_empty())
                        .cloned()
                        .collect();
                    if mono.len() == 1
                        && winners.iter().all(|b| {
                            if b.0 == mono[0].0 {
                                return true;
                            }
                            let rigid = self.rigidify_own_tparams(b.0, &b.1);
                            mono[0].1.len() == rigid.len()
                                && mono[0]
                                    .1
                                    .iter()
                                    .zip(&rigid)
                                    .all(|(a, b)| self.st.is_sub_type(a, b))
                                && !rigid
                                    .iter()
                                    .zip(&mono[0].1)
                                    .all(|(b, a)| self.st.is_sub_type(b, a))
                        })
                    {
                        winners = mono;
                    }
                }
                if let [w] = winners.as_slice() {
                    if self.function_value_ties(w, &function_values, arg_tys, with_views) {
                        return OverloadPick::Ambiguous;
                    }
                }
                // nsc `isStrictlyMoreSpecific` weighs the owners too: an
                // alternative earns a point for being as specific as the
                // other and another for being defined in a proper subclass of
                // the other's owner, and only a strictly greater total wins.
                // The sole winner above is as specific as everything, but an
                // alternative a *subclass* defines, that is not as specific
                // back, ties it: `class Child extends Parent { override def
                // who(x: Any) }` against `Parent.who(x: String)` makes
                // `child.who("s")` ambiguous in scalac 2.13.16, and picking
                // `who(String)` compiled a call nsc refuses.
                //
                // Only between members this run defines from source: a Java
                // class's package-private members (`AbstractStringBuilder`'s
                // `append(StringBuilder)`) and a library member reached both
                // from its pickle and from source are alternatives nsc never
                // weighs against each other, and this table cannot yet tell.
                let from_source = |m: SymbolId| {
                    m.0 >= self.st.prelude_end
                        && self.st.get(m).pickled_origin.is_empty()
                        && !self.st.get(m).flags.contains(Flags::JAVA)
                };
                if winners.len() == 1 && !with_views {
                    let w = &winners[0];
                    let tied = applicable.iter().any(|b| {
                        b.0 != w.0
                            && !b.0.is_none()
                            && !w.0.is_none()
                            && from_source(b.0)
                            && from_source(w.0)
                            && self.st.get(b.0).name == self.st.get(w.0).name
                            && self.owner_is_proper_subclass(b.0, w.0)
                            && !self.owner_is_proper_subclass(w.0, b.0)
                            && !self.is_as_specific_method(b.0, w.0, &b.1, &w.1, with_views)
                    });
                    if tied {
                        return OverloadPick::Ambiguous;
                    }
                }
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

    /// Whether a function-typed value alternative is applicable to the call
    /// and ties with the method alternative `w` that won: nsc weighs the
    /// value through its `apply` (`followApply`), and two alternatives of
    /// the same owner whose parameter lists are each as specific as the
    /// other are ambiguous.
    ///
    /// ```scala
    /// val f: String => Int = _.length
    /// def f(s: String): Int = 2
    /// f("")   // ambiguous reference to overloaded definition
    /// ```
    ///
    /// scala-rs never offered the value as an alternative and called the
    /// method. Only the tie is reported here; a value alternative that is
    /// strictly *more* specific than every method is not selected by this
    /// resolver, and is left as it was.
    fn function_value_ties(
        &self,
        w: &(SymbolId, Vec<Type>, Type),
        values: &[(SymbolId, Vec<Type>, Type)],
        arg_tys: &[Type],
        with_views: bool,
    ) -> bool {
        if w.0.is_none() || values.is_empty() {
            return false;
        }
        values.iter().any(|v| {
            if v.1.len() != arg_tys.len()
                || !self.is_applicable(v.0, 0, &v.1, arg_tys, with_views, &[], None)
            {
                return false;
            }
            let weight = |a: &(SymbolId, Vec<Type>, Type), b: &(SymbolId, Vec<Type>, Type)| {
                u8::from(self.is_as_specific_method(a.0, b.0, &a.1, &b.1, with_views))
                    + u8::from(self.owner_is_proper_subclass(a.0, b.0))
            };
            weight(w, v) <= weight(v, w) && weight(v, w) <= weight(w, v)
        })
    }

    /// One alternative's parameter and result types with its *own* type
    /// parameters rewritten to positional markers, so two declarations of the
    /// same signature compare equal however their type parameters were
    /// allocated. The markers are `TypeMember` of ids no symbol can have; they
    /// are only ever compared, never looked up.
    fn canonical_sig(&self, sym: SymbolId, ps: &[Type], ret: &Type) -> Vec<Type> {
        let mut out: Vec<Type> = ps.to_vec();
        out.push(ret.clone());
        if sym.is_none() {
            return out;
        }
        let tps = &self.st.get(sym).tparams;
        if tps.is_empty() {
            return out;
        }
        let marks: Vec<Type> = (0..tps.len())
            .map(|i| Type::TypeMember(SymbolId(u32::MAX - i as u32)))
            .collect();
        out.iter()
            .map(|t| crate::symbol::subst_tparams_slice(tps, &marks, t))
            .collect()
    }

    /// Complete declaring owners preserved by synthetic inherited overloads.
    /// Their installation owner is a lookup location, not their specificity owner.
    pub(crate) fn complete_overload_owners(&mut self, fun: &Tree) {
        if fun.sym.is_none() || !matches!(fun.ty, Type::Overload(_)) {
            return;
        }
        let name = self.st.get(fun.sym).name.clone();
        let owners: Vec<_> = self
            .overload_alternatives(fun.sym, &name)
            .into_iter()
            .map(|m| self.st.get(m).declaring_class.clone())
            .filter(|owner| !owner.is_empty())
            .collect();
        for owner in owners {
            let cls = crate::classpath::find_or_stub_java_class(&mut self.st, &owner);
            self.ensure_java_loaded(cls, fun.span);
        }
    }

    /// nsc's `isInProperSubClassOf`: `a`'s owner is a class, `b`'s owner is a
    /// different class, and the first is a subclass of the second. Only real
    /// classes count -- two alternatives owned by the same class, or by
    /// anything that is not a class, are not ordered by this rule.
    pub(crate) fn owner_is_proper_subclass(&self, a: SymbolId, b: SymbolId) -> bool {
        if a.is_none() || b.is_none() {
            return false;
        }
        if let (Some((ao, _)), Some((bo, _))) = (
            self.st.get(a).pickled_origin.split_once('#'),
            self.st.get(b).pickled_origin.split_once('#'),
        ) {
            return ao != bo
                && self
                    .st
                    .get(a)
                    .pickled_owner_bases
                    .iter()
                    .any(|base| base == bo);
        }
        let declaration_owner = |m: SymbolId| {
            let symbol = self.st.get(m);
            crate::classpath::find_by_jvm(&self.st, &symbol.declaring_class).unwrap_or(symbol.owner)
        };
        let ao = declaration_owner(a);
        let bo = declaration_owner(b);
        if ao.is_none() || bo.is_none() || ao == bo {
            return false;
        }
        if !self.st.get(ao).is_class_like() || !self.st.get(bo).is_class_like() {
            return false;
        }
        self.class_has_base(ao, bo)
    }

    /// Whether `sup` is among `sub`'s base classes.
    ///
    /// The question is about *symbols*, so unlike
    /// [`Self::base_type_instance`] no type arguments have to be carried
    /// through the walk -- and that is what lets it read a parent written as a
    /// function type. `class AndThen[-T, +R] extends (T => R)` (cats
    /// `data/AndThen.scala`) records `Type::Function` as its parent, which
    /// both `class_reaches` and `base_type_instance` stop dead at, so
    /// `AndThen.andThen`/`compose` and the `Function1` members they override
    /// were two equally specific alternatives with no owner relation to
    /// separate them -- `ambiguous overload` where nsc's `isInProperSubClassOf`
    /// takes the subclass's.
    fn class_has_base(&self, sub: SymbolId, sup: SymbolId) -> bool {
        let mut seen: Vec<u32> = vec![sub.0];
        let mut work: Vec<SymbolId> = vec![sub];
        while let Some(c) = work.pop() {
            for p in &self.st.get(c).parents {
                let owned;
                let p = match p {
                    Type::Function { .. } => match self.st.function_class_form(p) {
                        Some(c) => {
                            owned = c;
                            &owned
                        }
                        None => continue,
                    },
                    other => other,
                };
                let Type::Class { sym, .. } = p else { continue };
                if *sym == sup {
                    return true;
                }
                if !seen.contains(&sym.0) {
                    seen.push(sym.0);
                    work.push(*sym);
                }
            }
        }
        false
    }

    fn residual_clause_is_implicit(&self, sym: SymbolId) -> bool {
        if sym.is_none() {
            return false;
        }
        let clauses = &self.st.get(sym).paramss;
        clauses.len() == 2
            && !clauses[1].is_empty()
            && clauses[1]
                .iter()
                .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
    }

    fn overload_residual_key(&self, sym: SymbolId) -> Vec<Type> {
        if sym.is_none() {
            return Vec::new();
        }
        match &self.st.get(sym).ty {
            Type::Method { paramss, .. } if paramss.len() > 1 => self.canonical_sig(
                sym,
                &[],
                &Type::Method {
                    paramss: paramss[1..].to_vec(),
                    ret: Box::new(Type::NoType),
                },
            ),
            _ => Vec::new(),
        }
    }

    /// nsc: `A` is as specific as `B` when `B` is applicable to `A`'s parameter types.
    ///
    /// `B`'s own type parameters are undetermined for that test, exactly as
    /// they are for a real call. `StringOps` has both
    /// `map(f: Char => Char): String` and `map[B](f: Char => B): IndexedSeq[B]`;
    /// without instantiating `B := Char` neither alternative is as specific as
    /// the other and every `"…".map(…)` was `ambiguous overload`.
    pub(crate) fn is_as_specific_method(
        &self,
        a_sym: SymbolId,
        b_sym: SymbolId,
        a_ps: &[Type],
        b_ps: &[Type],
        with_views: bool,
    ) -> bool {
        // `A`'s own type parameters stand in for the *arguments* of this
        // hypothetical call, so they are rigid: `B` in `map[B](Char => B)` is
        // not a `Char`. Its upper bound is the closest rigid stand-in we have.
        // Without this every polymorphic alternative was as specific as every
        // monomorphic one and the pair came out `ambiguous overload`.
        let a_ps = self.rigidify_own_tparams(a_sym, a_ps);
        let a_ps = self.spec_argtpes(&a_ps, b_ps);
        // `B`'s type parameters are undetermined, exactly as for a real call.
        let inst = if b_sym.is_none() || self.st.get(b_sym).tparams.is_empty() {
            Vec::new()
        } else {
            self.infer_method_tparams(b_sym, b_ps, &a_ps)
        };
        let b_ps: Vec<Type> = if inst.is_empty() {
            b_ps.to_vec()
        } else {
            let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
            let tys: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
            b_ps.iter()
                .map(|p| crate::symbol::subst_tparams_slice(&tps, &tys, p))
                .collect()
        };
        let saved = self.spec_probe.replace(true);
        let out = self.is_applicable(SymbolId::NONE, 0, &b_ps, &a_ps, with_views, &[], None)
            && self.function_params_conform(&a_ps, &b_ps);
        self.spec_probe.set(saved);
        out
    }

    /// `A`'s parameter types read as the *argument* types of the hypothetical
    /// call that decides specificity -- nsc `Infer.isAsSpecific`'s
    /// `if (isVarArgsList(params) && isVarArgsList(ftpe2.params))` clause.
    ///
    /// A repeated parameter is unwrapped to its element type only when *both*
    /// signatures are varargs lists (`f(Int, Int*)` against `f(Int*)` compares
    /// `Int, Int` with `Int*`). When only `A` is a varargs list, its trailing
    /// `T*` stays a repeated type on the argument side, and `arg_score` under
    /// [`Typer::spec_probe`] scores it against nothing at all. That single
    /// asymmetry is what gives a fixed-arity alternative the win over its own
    /// varargs sibling:
    ///
    /// ```text
    /// def g(x: Int)   as specific as g(Int*)?  Int  vs Int* -> yes
    /// def g(x: Int*)  as specific as g(Int)?   Int* vs Int  -> no
    /// ```
    ///
    /// "Against nothing at all" is stronger than the `<repeated>[T] <: Seq[T]`
    /// that nsc's own definition of the wrapper suggests, and it is what
    /// scalac 2.13.16 does. Six measurements, three of them Java read back
    /// from a class file, agree on it and on no weaker rule -- the `Any` and
    /// `Object` rows are the ones a `Seq[T]` model gets wrong:
    ///
    /// | alternatives | call | scalac 2.13.16 |
    /// |---|---|---|
    /// | `a(Int)`, `a(Int*)` (Java `int...`) | `a(1)` | fixed |
    /// | `c(Object)`, `c(Object*)` (Java) | `c("z")` | fixed |
    /// | `e(java.util.List[Object])`, `e(Object*)` (Java) | `e(list)` | fixed |
    /// | `b(Object)`, `b(Int*)` (Java) | `b(1)` | **ambiguous** |
    /// | `s(Any)`, `s(Int*)` (Scala) | `s(1)` | **ambiguous** |
    /// | `u(Int, Int*)`, `u(Int*)` (Scala) | `u(1)` | **ambiguous** |
    ///
    /// The last two are why the both-varargs unwrap has to stay, and why the
    /// rule cannot simply be "a fixed-arity alternative wins": `u` is a tie
    /// between two varargs lists, and `s`/`b` are ties in which the
    /// fixed-arity alternative is *not* as specific as the varargs one.
    ///
    /// Nothing here is Java-specific. `def g(x: Int)` beside
    /// `def g(x: Int*)` in plain Scala had the same `ambiguous overload`, and
    /// scalac resolves both to the fixed-arity alternative; the defect showed
    /// up on `java.lang.reflect.Array.newInstance`, which declares exactly
    /// this pair, because that is what the standard library calls.
    fn spec_argtpes(&self, a_ps: &[Type], b_ps: &[Type]) -> Vec<Type> {
        let (_, a_rep) = split_repeated(a_ps);
        let (_, b_rep) = split_repeated(b_ps);
        match (a_rep, b_rep) {
            (Some(a_elem), Some(_)) => {
                let mut out = a_ps[..a_ps.len() - 1].to_vec();
                out.push(a_elem.clone());
                out
            }
            _ => a_ps.to_vec(),
        }
    }

    /// `arg_score` deliberately scores any two function types with the same
    /// parameter shape as a match, because an un-inferred literal has no
    /// result type yet. Specificity compares two *signatures*, where both
    /// results are known, so `Char => Any` must not count as a `Char => Char`.
    fn function_params_conform(&self, a_ps: &[Type], b_ps: &[Type]) -> bool {
        a_ps.iter().zip(b_ps).all(|(a, b)| match (a, b) {
            (Type::Function { .. }, Type::Function { .. })
                if !mentions_no_type(a) && !mentions_no_type(b) =>
            {
                self.st.is_sub_type(a, b)
            }
            _ => true,
        })
    }

    /// nsc's rule for a stable-identifier pattern: the pattern's type and the
    /// scrutinee only have to be *inhabitable together*, not conforming.
    ///
    /// Two open classes always are -- a subclass of one could extend the other
    /// -- so `case Ids.other =>` against an unrelated `ST[Int]` is accepted,
    /// which is what scalac 2.13.16 does. A `final` class (`String`, a value
    /// class, an array) or a primitive rules the pattern out, and scalac
    /// reports the same `type mismatch` there.
    pub(crate) fn stable_pattern_compatible(&self, pat_ty: &Type, sel_ty: &Type) -> bool {
        if pat_ty.is_no_type() || pat_ty.is_error() || sel_ty.is_no_type() || sel_ty.is_error() {
            return true;
        }
        let pat_ty = pat_ty.widen_constant();
        if self.st.is_sub_type(&pat_ty, sel_ty) || self.st.is_sub_type(sel_ty, &pat_ty) {
            return true;
        }
        !self.is_final_like(&pat_ty) && !self.is_final_like(sel_ty)
    }

    /// A type no further subclass can widen: primitives, `String`, arrays,
    /// objects, and anything declared `final`.
    fn is_final_like(&self, ty: &Type) -> bool {
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
            | Type::String
            | Type::Nothing
            | Type::Null
            | Type::Array(_)
            | Type::ModuleRef(_)
            | Type::Tuple(_) => true,
            Type::Class { sym, .. } => {
                let s = self.st.get(*sym);
                s.flags.contains(Flags::FINAL) || s.flags.contains(Flags::MODULE)
            }
            _ => false,
        }
    }

    /// nsc `Infer.protoTypeArgs`. Before a single argument has been typed, the
    /// expected type already says what some of the callee's type parameters
    /// have to be, and an argument that fills a bare one of them deserves that
    /// as its prototype: `(new Select(…), Map(s -> a2))` checked against
    /// `(Node, Map[TermSymbol, Aggregate])` types its second component against
    /// `Map[TermSymbol, Aggregate]`, and an invariant `Map` keyed by the
    /// argument's own `AnonSymbol` is no longer what comes back.
    ///
    /// Only a parameter that *is* a type parameter, and (for the type-argument
    /// route) only a callee with one alternative: anything else and the
    /// prototype would be weighing the alternatives instead of the arguments.
    /// A *fully determined function-shaped* parameter is the exception --
    /// nothing about it is still being solved, and a function literal nested in
    /// the argument has nowhere else to read its parameter types from.
    /// `NoType` means "as before".
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn proto_arg_type(
        &self,
        fun_ty: &Type,
        sym: SymbolId,
        idx: usize,
        nargs: usize,
        pt: &Type,
        recv: Option<&Type>,
        use_lower_bounds: bool,
    ) -> Type {
        if sym.is_none() {
            return Type::NoType;
        }
        // nsc `preSelectOverloaded`: alternatives whose arity cannot take
        // `nargs` arguments are dropped before the arguments are typed, so
        // `Predef.println(if (c) 1 else 2.0)` types its argument against
        // `println(x: Any)` -- `println()` never was a candidate -- and the
        // `if` keeps `Any` rather than joining its branches to `Double`.
        if let Type::Overload(alts) = fun_ty {
            if let Some(p) = sole_arity_match_param(alts, idx, nargs) {
                return p;
            }
        }
        // An overloaded reference has no single parameter type -- except where
        // every alternative wants the *same* one, which is `Infer.pretypeArgs`
        // again (`agreed_lambda_params` does it for a literal that is itself
        // the argument). A case class's companion `apply` arrives here as an
        // overload, and slick's
        // `StatementParameters(…, if (…) … else { s => …; … }, …)` needed the
        // function parameter's type to reach the literal inside the if/else.
        if let Type::Overload(alts) = fun_ty {
            let agreed = self.agreed_function_param(alts, idx);
            if !agreed.is_no_type() {
                return agreed;
            }
            return self.only_concrete_param(alts, idx);
        }
        // `rewrite_receiver_apply` deliberately leaves `Obj(args)` as a
        // reference to the *module* (`named_arg_param_ids` says why), so the
        // callee's type here is a `ModuleRef` and the parameters live on its
        // `apply`. slick's `StatementParameters(…, if (…) … else { s => …;
        // … }, …)` is exactly that shape.
        if let Type::ModuleRef(cls) = fun_ty {
            // A case class's companion also *inherits* `AbstractFunctionN.apply`,
            // whose signature is written in that parent's own type parameters
            // (`(T1, T2, T3)R`). Read it through the companion, or the two
            // alternatives never agree.
            let recv = Type::Class {
                sym: *cls,
                args: Vec::new(),
            };
            // A JVM `static` is no member of the object: twirl's templates are
            // `object edithook extends BaseScalaTemplate(…)`, a case class
            // from a jar whose class file carries the companion's
            // `public static BaseScalaTemplate apply(F)` forwarder. Counted as
            // a second alternative it made the two disagree, so
            // `html.edithook(hook, Set(WebHook.Push), …)` typed its `Set`
            // argument with no prototype and missed the invariant
            // `Set[WebHook.Event]` parameter.
            let syms: Vec<SymbolId> = self
                .st
                .lookup_member(*cls, "apply")
                .into_iter()
                .filter(|a| !self.st.get(*a).flags.contains(Flags::STATIC))
                .collect();
            let alts: Vec<Type> = syms
                .iter()
                .map(|a| self.st.subst_as_seen_from(&recv, &self.st.get(*a).ty))
                .collect();
            if alts.is_empty() {
                return Type::NoType;
            }
            let f = self.agreed_function_param(&alts, idx);
            if !f.is_no_type() {
                return f;
            }
            let v = self.agreed_value_param(&alts, idx);
            if !v.is_no_type() {
                return v;
            }
            // A generic case class's companion has one `apply`, and it is
            // polymorphic: what the expected type settles about its type
            // parameters is the argument's prototype, exactly as for any other
            // polymorphic method (nsc's `protoTypeArgs` does not care how the
            // callee was spelled). cats' `EitherK(run match { case Right(ga) =>
            // Right(G.coflatMap(ga)(x => rightc(x))) … })` at a declared
            // `EitherK[F, G, EitherK[F, G, A]]` has no other way to tell
            // `rightc` its `F`, and `Kleisli((fe: Either[A, B]) => …)` no other
            // way to give the literal's body its `F[Either[C, D]]`.
            let mut poly = syms.iter().zip(&alts).filter(|(s, t)| {
                !self.st.get(**s).tparams.is_empty() && matches!(t, Type::Method { .. })
            });
            if let (Some((s, t)), None) = (poly.next(), poly.next()) {
                return self.proto_arg_type(t, *s, idx, nargs, pt, Some(&recv), use_lower_bounds);
            }
            return Type::NoType;
        }
        let Type::Method { paramss, ret } = fun_ty else {
            return Type::NoType;
        };
        let tps = self.st.get(sym).tparams.clone();
        let Some(params) = paramss.first() else {
            return Type::NoType;
        };
        if tps.is_empty() {
            // nsc types every argument against its parameter type; scala-rs
            // only handed one out where inference needs it, so a monomorphic
            // callee gave none. A function literal that *is* the argument gets
            // its parameter types in `type_apply_in` instead -- but one that
            // merely sits inside the argument (`f(if (c) { s => … } else { s
            // => …; … })`, slick's `JdbcBackend.createStatement`) had nothing
            // to read them from. The one-expression branch only ever worked by
            // accident: `section_param_types` recovers `s` from the call in
            // `{ s => si(s) }`, and a two-statement body has no such call.
            //
            // Restricting it to a function-shaped parameter left the argument
            // of every other monomorphic call with no expected type, and an
            // argument whose *own* type parameters are inferred has nothing
            // else to read them from: `def f(b: Box[Any])` applied to
            // `f(Box(n))` for an `n: String` inferred `Box[String]` from the
            // argument alone and then reported `no matching overload for
            // (Box[Any])Int with arguments (Box[String])`. nsc solves `E` from
            // the prototype, which is what slick's
            // `errors += RefId(n1)` (`RefId[E <: AnyRef]`, invariant, an
            // `errors: Set[RefId[Dumpable]]`) needs.
            //
            // The prototype stays a hint: the caller re-types the argument
            // with none whenever it does not fit.
            // A wildcard in the parameter is no reason to withhold the
            // prototype: `case class Column(…, options: Set[ColumnOption[_]])`
            // taking `Set() ++ …` has nothing *but* the parameter to say what
            // `++`'s element type is, and without it the call was
            // `Set[ColumnOption[Nothing]]` against an invariant `Set`
            // (`jdbc/JdbcModelBuilder.scala:279`). The prototype stays a hint
            // -- the caller re-types with none when the argument does not fit.
            return match param_at(params, idx) {
                Some(p) if !p.is_no_type() && !p.is_error() => {
                    // A by-name or repeated formal expects the *value*;
                    // wrapping it is `adapt`'s job and the caller would throw
                    // a prototype the argument cannot be a subtype of away.
                    match p {
                        Type::ByName(inner) | Type::Repeated(inner) => (**inner).clone(),
                        other => other.clone(),
                    }
                }
                _ => Type::NoType,
            };
        }
        // Explicit type arguments have already settled this parameter, and
        // that settled type *is* the expected type of the argument. Typing it
        // against nothing leaves whatever only an expectation can fix
        // unsolved: `take[F, State[F]](State(max, min, TreeMap.empty))` --
        // slick's `ConnectionArbiter.create` -- has to read the higher-kinded
        // `F` of `case class State[F[_]]` off `State[F]`, since no argument
        // mentions it, and the argument came back `State[_]`.
        if let Some(p) = param_at(params, idx) {
            if !p.is_no_type()
                && !p.is_error()
                && !mentions_tparam(p, &tps)
                && !type_mentions_wildcard(p)
            {
                return p.clone();
            }
        }
        let Some(param) = param_at(params, idx) else {
            return Type::NoType;
        };
        if !mentions_tparam(param, &tps) {
            return Type::NoType;
        }
        let mut solved: Vec<(SymbolId, Type)> = self
            .add_expected_constraints_in(sym, ret, pt, Vec::new(), true)
            .into_iter()
            .filter(|(_, t)| {
                !t.is_no_type()
                    && !t.is_error()
                    && !matches!(t, Type::Nothing | Type::Any | Type::Wildcard)
                    && !mentions_tparam(t, &tps)
            })
            .collect();
        for &tp in &tps {
            if use_lower_bounds && !solved.iter().any(|(id, _)| *id == tp) {
                if let Some(lo) = self.tparam_lower_bound(sym, tp, recv) {
                    solved.push((tp, lo));
                }
            }
        }
        if let Type::TypeParam(tp) = param {
            if !tps.contains(tp) {
                return Type::NoType;
            }
            return solved
                .into_iter()
                .find(|(id, _)| id == tp)
                .map(|(_, t)| t)
                .unwrap_or(Type::NoType);
        }
        // nsc `protoTypeArgs` does not stop at a formal that *is* a variable:
        // it substitutes what the expected type settled into every formal.
        // cats' `def >>[B](fb: => F[B])(implicit F: FlatMap[F]): F[B]` checked
        // against `F[Unit]` says `B = Unit`, so the argument's prototype is
        // `=> F[Unit]`. Without it the argument was typed against nothing and
        // `e.fold(F.raiseError, _ => F.unit)` came back as the `lub` of
        // `F[A]` and `F[Unit]` -- `AnyRef` -- which fits no `F[B]`.
        //
        // Only when the expected type settles *every* variable the formal
        // mentions: a formal that still carries one is a prototype that
        // constrains the argument by a variable's bound, which is what
        // `open_to_bounds` is for later on.
        if solved.is_empty() {
            return Type::NoType;
        }
        let ids: Vec<SymbolId> = solved.iter().map(|(id, _)| *id).collect();
        let vals: Vec<Type> = solved.iter().map(|(_, t)| t.clone()).collect();
        let out = crate::symbol::subst_tparams_slice(&ids, &vals, param);
        if type_mentions_wildcard(&out) {
            return Type::NoType;
        }
        // nsc's *lenient* prototype (`protoTypeArgs` / `typedArgToPoly`): a
        // variable the expected type did not settle is `WildcardType` in the
        // argument's expected type, which constrains nothing there while the
        // settled positions still reach the argument. The library's
        // `override def map[B](f: A => B): CC[B] =
        // strictOptimizedMap(iterableFactory.newBuilder, f)` is the shape:
        // `strictOptimizedMap[B', C2](b: Builder[B', C2], f: A => B')` has
        // `C2 := CC[B]` from the declared result, and `iterableFactory
        // .newBuilder` typed at `Builder[_, CC[B]]` reads its own `A := B`
        // out of the invariant `CC[A]` -- which is the only place `A` can be
        // read from, `Builder` being contravariant in its element. Typed with
        // no prototype instead it stayed `Builder[?A, CC[?A]]`, and the outer
        // call reported `found: Builder[A, CC[A]] required: Builder[B, CC[A]]`
        // (13 errors across `StrictOptimized{Iterable,Map,Seq,SortedMap}Ops`).
        let out = if mentions_tparam(&out, &tps) {
            let rest: Vec<SymbolId> = tps
                .iter()
                .copied()
                .filter(|tp| type_mentions_tparam(&out, *tp))
                .collect();
            let wilds = vec![Type::Wildcard; rest.len()];
            crate::symbol::subst_tparams_slice(&rest, &wilds, &out)
        } else {
            out
        };
        // A by-name formal expects the *value*: `is_sub_type(F[Unit],
        // => F[Unit])` is false, and the caller would throw the prototype away
        // as one the argument did not fit. Wrapping in `Function0` is `adapt`'s
        // job, and it still runs on the parameter itself.
        match out {
            Type::ByName(inner) => *inner,
            other => other,
        }
    }

    /// nsc's *lenient* `protoTypeArgs`: the formal with what the expected type
    /// settles substituted in and every parameter it leaves open read as a
    /// wildcard. [`Self::proto_arg_type`] declines such a formal outright, and
    /// that is right for most arguments; this is for a nested *call*, whose
    /// own inference has nothing else to go on. cats' `Arrow.second` writes
    /// `compose(swap, compose(first[A, B, C](fa), swap))` at a declared
    /// `F[(C, A), (C, B)]`: the outer `compose` settles its `A` from that and
    /// hands the inner call `F[(C, A), ?]`, and only that `(C, A)` tells the
    /// inner `swap[X, Y]` what `X` and `Y` are. Typed against nothing, the
    /// inner call is ill-typed in nsc too.
    ///
    /// `NoType` unless the callee is one polymorphic method, the formal is not
    /// a bare type parameter, and the expected type settles at least one of
    /// the parameters the formal mentions.
    pub(crate) fn lenient_proto_arg_type(
        &self,
        fun_ty: &Type,
        sym: SymbolId,
        idx: usize,
        pt: &Type,
    ) -> Type {
        if sym.is_none() || pt.is_no_type() || pt.is_error() {
            return Type::NoType;
        }
        let Type::Method { paramss, ret } = fun_ty else {
            return Type::NoType;
        };
        let tps = self.st.get(sym).tparams.clone();
        let Some(param) = paramss.first().and_then(|ps| param_at(ps, idx)) else {
            return Type::NoType;
        };
        let param = match param {
            Type::ByName(inner) => inner.as_ref(),
            other => other,
        };
        if tps.is_empty() || matches!(param, Type::TypeParam(_)) || !mentions_tparam(param, &tps) {
            return Type::NoType;
        }
        let solved: Vec<(SymbolId, Type)> = self
            .add_expected_constraints_in(sym, ret, pt, Vec::new(), false)
            .into_iter()
            .filter(|(tp, t)| {
                type_mentions_tparam(param, *tp)
                    && !t.is_no_type()
                    && !t.is_error()
                    && !matches!(t, Type::Nothing | Type::Any | Type::Wildcard)
                    && !type_has_wildcard(t)
                    && !mentions_tparam(t, &tps)
            })
            .collect();
        if solved.is_empty() {
            return Type::NoType;
        }
        let ids: Vec<SymbolId> = solved.iter().map(|(id, _)| *id).collect();
        let vals: Vec<Type> = solved.iter().map(|(_, t)| t.clone()).collect();
        let out = crate::symbol::subst_tparams_slice(&ids, &vals, param);
        let rest: Vec<SymbolId> = tps
            .iter()
            .copied()
            .filter(|tp| type_mentions_tparam(&out, *tp))
            .collect();
        // A type *constructor* left open is not a wildcard's kind.
        if rest.iter().any(|tp| !self.st.get(*tp).tparams.is_empty()) {
            return Type::NoType;
        }
        crate::symbol::subst_tparams_slice(&rest, &vec![Type::Wildcard; rest.len()], &out)
    }

    /// Explicit type arguments for a *Java* method, with `Any` read as the
    /// `Object` its type parameter is really bounded by (nsc's
    /// `ObjectTpeJava`). Anything else, and any Scala-defined method, is left
    /// exactly as written.
    pub(crate) fn java_object_targs(&self, sym: SymbolId, targs: Vec<Type>) -> Vec<Type> {
        if sym.is_none()
            || !self.st.get(sym).flags.contains(Flags::JAVA)
            || !targs.iter().any(|t| matches!(t, Type::Any))
        {
            return targs;
        }
        let tps = self.st.get(sym).tparams.clone();
        targs
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let object_bound = match tps.get(i).map(|tp| self.st.get(*tp).bound_hi.clone()) {
                    Some(None) | Some(Some(Type::AnyRef)) => true,
                    Some(Some(other)) => matches!(&other, Type::Class { sym, .. }
                        if self.st.get(*sym).jvm_name == "java/lang/Object"),
                    None => false,
                };
                if matches!(t, Type::Any) && object_bound {
                    Type::AnyRef
                } else {
                    t
                }
            })
            .collect()
    }

    /// A parameter a function literal can inhabit: a function type, a pickled
    /// `FunctionN` class, or a single-abstract-method trait.
    pub(crate) fn is_function_shaped(&self, p: &Type) -> bool {
        if is_function_pt(p) {
            return true;
        }
        match crate::prefix::strip_view(p) {
            Type::Class { sym, args } => {
                self.st.function_class_shape(*sym, args).is_some() || self.st.sam_sig(p).is_some()
            }
            _ => false,
        }
    }

    /// The one alternative whose parameter at `idx` is already a concrete
    /// type, when every alternative asks for the same class there.
    ///
    /// 2.13 overloads `++` on a set as `SetOps.++(IterableOnce[A])` beside
    /// `IterableOps.++[B >: A](IterableOnce[B])` (`prelude_setmap.rs`), and
    /// with two alternatives in play [`Self::proto_arg_type`] used to hand the
    /// argument no prototype at all. slick's
    /// `oldDiscCandidates ++ (tree match { … case _ => Set.empty })` then typed
    /// its `match` with nothing to go on, lubbed the arms to the existential
    /// `Set[_ <: AnyRef]` -- which is what scalac produces there too, given no
    /// expected type -- and neither alternative could take it. nsc never gets
    /// there: it types the argument against `IterableOnce[A]`, and the arms
    /// adapt to it one by one.
    ///
    /// The monomorphic alternative is the one that can say what it wants, so
    /// its parameter is the prototype. Still only a hint: the caller re-types
    /// the argument with none when it does not fit.
    fn only_concrete_param(&self, alts: &[Type], idx: usize) -> Type {
        if alts.len() < 2 {
            return Type::NoType;
        }
        let mut params: Vec<&Type> = Vec::new();
        for a in alts {
            let Type::Method { paramss, .. } = a else {
                return Type::NoType;
            };
            let Some(p) = paramss.first().and_then(|c| param_at(c, idx)) else {
                return Type::NoType;
            };
            if p.is_no_type() || p.is_error() {
                return Type::NoType;
            }
            params.push(p);
        }
        let head = self.st.class_sym_of(params[0]);
        if head.is_none() || !params.iter().all(|p| self.st.class_sym_of(p) == head) {
            return Type::NoType;
        }
        let mut concrete = params
            .iter()
            .filter(|p| !mentions_any_tparam(p) && !type_mentions_wildcard(p));
        match (concrete.next(), concrete.next()) {
            (Some(p), None) => (*p).clone(),
            _ => Type::NoType,
        }
    }

    /// The parameter type at `idx` every alternative agrees on, when it is a
    /// fully determined function-shaped one. `Infer.pretypeArgs` again, for an
    /// argument that is not itself the function literal
    /// (`agreed_lambda_params` covers the literal). An alternative's own type
    /// parameter must stay open until the real candidate is known -- that is
    /// `agreed_lambda_params`'s measured note about cats' `uncancelable[A]`.
    fn agreed_function_param(&self, alts: &[Type], idx: usize) -> Type {
        let mut agreed: Option<&Type> = None;
        for a in alts {
            let Type::Method { paramss, .. } = a else {
                return Type::NoType;
            };
            let Some(p) = paramss.first().and_then(|c| param_at(c, idx)) else {
                return Type::NoType;
            };
            match agreed {
                None => agreed = Some(p),
                Some(prev) if prev == p => {}
                Some(_) => return Type::NoType,
            }
        }
        match agreed {
            Some(p)
                if !p.is_no_type()
                    && !p.is_error()
                    && !type_mentions_wildcard(p)
                    && !mentions_any_tparam(p)
                    && self.is_function_shaped(p) =>
            {
                p.clone()
            }
            _ => Type::NoType,
        }
    }

    /// The parameter type at `idx` every alternative agrees on, when it names
    /// no type parameter of its own. Unlike `agreed_function_param` this does
    /// not insist on a function shape, and unlike `only_concrete_param` it
    /// does not reject a wildcard.
    ///
    /// `rewrite_receiver_apply` leaves `m.Column(name = …, options = Set() ++
    /// …)` as a reference to the *module*, so the monomorphic branch below
    /// never sees it; the case class's `apply` and the `AbstractFunctionN.apply`
    /// it inherits agree on `Set[ColumnOption[_]]`, and that declared type is
    /// the only thing that says what `++`'s element type is
    /// (`jdbc/JdbcModelBuilder.scala:279`).
    fn agreed_value_param(&self, alts: &[Type], idx: usize) -> Type {
        let mut agreed: Option<&Type> = None;
        for a in alts {
            let Type::Method { paramss, .. } = a else {
                return Type::NoType;
            };
            let Some(p) = paramss.first().and_then(|c| param_at(c, idx)) else {
                return Type::NoType;
            };
            match agreed {
                None => agreed = Some(p),
                Some(prev) if prev == p => {}
                Some(_) => return Type::NoType,
            }
        }
        match agreed {
            Some(p) if !p.is_no_type() && !p.is_error() && !mentions_any_tparam(p) => match p {
                Type::ByName(inner) | Type::Repeated(inner) => (**inner).clone(),
                other => other.clone(),
            },
            _ => Type::NoType,
        }
    }

    /// nsc `Infer.pretypeArgs`. The parameter types every overload alternative
    /// wants for the function literal at `idx`, when they all agree and are
    /// fully determined. `None` leaves the literal untyped, as before.
    pub(crate) fn agreed_lambda_params(
        &self,
        fun_ty: &Type,
        idx: usize,
        arity: usize,
    ) -> Option<Vec<Type>> {
        // Pre-typing is only for narrowing an *overload*: a lambda argument
        // to a single-candidate callee is untyped (every parameter
        // `NoType`) at the scoring stage regardless, and `arg_score`'s
        // "shapes agree while a literal's parameters are still open" rule
        // already treats that as a match against any function-shaped (or
        // SAM-shaped, see its own comment) parameter -- scoring does not
        // need real parameter types, only arity-shaped compatibility.
        // Actually pre-typing the literal here for a single candidate was
        // tried and measured against slick end-to-end: it fixed
        // `SQLActionBuilder(sql, (u, pp) => ...)` (a case class apply, whose
        // sole "overload" has no type parameters of its own) but also
        // pre-typed cats-effect's `Async[F].uncancelable[A](body: Poll[F]
        // => F[A]): F[A]` against `A` before the call's own usage-driven
        // inference had solved it, locking in the wrong type and turning
        // 155 baseline slick errors into 232. The lambda still ends up
        // correctly typed either way -- `adapt_args_to_params`, run once
        // the real winning candidate is known, retypes every argument
        // against its actual parameter type, `Unit`/`PositionedParameters`
        // included -- so restricting this pre-typing back to true overloads
        // costs nothing here, it just moves the SAM-parameter case's real
        // typing to the adapt step instead of the scoring step.
        let alts: Vec<Type> = match fun_ty {
            Type::Overload(alts) if alts.len() >= 2 => alts.clone(),
            _ => return None,
        };
        let mut agreed: Option<Vec<Type>> = None;
        for a in &alts {
            let Type::Method { paramss, .. } = a else {
                return None;
            };
            let p = paramss.first()?.get(idx)?;
            // A pickled signature spells a function parameter as `Function1`.
            // A parameter that is merely SAM-shaped (`trait SetParameter[-T]
            // extends ((T, PositionedParameters) => Unit)`, not literally a
            // `scala.FunctionN`) needs the SAM search instead -- exactly what
            // `type_function` itself falls back to once the literal is
            // typed, just needed here too so it is typed with real
            // parameter types in the first place.
            let p = match p {
                Type::Class { sym, args } => self
                    .st
                    .function_class_shape(*sym, args)
                    .or_else(|| {
                        self.st.sam_sig(p).map(|sam| Type::Function {
                            params: sam.param_tys,
                            ret: Box::new(sam.ret_ty),
                        })
                    })
                    .unwrap_or_else(|| p.clone()),
                other => other.clone(),
            };
            let Type::Function { params, .. } = p else {
                return None;
            };
            if params.len() != arity || params.iter().any(mentions_no_type) {
                return None;
            }
            match &agreed {
                None => agreed = Some(params),
                Some(prev) if *prev == params => {}
                Some(_) => return None,
            }
        }
        agreed
    }

    /// `agreed_lambda_params` for `PartialFunction` parameters.
    ///
    /// When every alternative wants a `PartialFunction[A, _]` at `idx` with
    /// the same `A`, a `{ case … }` literal there can be typed before the
    /// alternatives are weighed. The result is left open by asking for
    /// `PartialFunction[A, Any]`: the case-block path in `type_function`
    /// turns an `Any` result into `NoType`, so the case bodies supply it and
    /// the literal comes back as the `PartialFunction` it really is -- which
    /// is what then picks the alternative.
    pub(crate) fn agreed_pf_param(&self, fun_ty: &Type, idx: usize) -> Option<Type> {
        let Type::Overload(alts) = fun_ty else {
            return None;
        };
        if alts.len() < 2 {
            return None;
        }
        let mut agreed: Option<(SymbolId, Type)> = None;
        for a in alts {
            let Type::Method { paramss, .. } = a else {
                return None;
            };
            let p = paramss.first()?.get(idx)?;
            let Type::Class { sym, args } = p else {
                return None;
            };
            if !is_partial_function_sym(&self.st, *sym) || args.len() != 2 {
                return None;
            }
            if mentions_no_type(&args[0]) {
                return None;
            }
            match &agreed {
                None => agreed = Some((*sym, args[0].clone())),
                Some((_, prev)) if *prev == args[0] => {}
                Some(_) => return None,
            }
        }
        let (sym, from) = agreed?;
        Some(Type::Class {
            sym,
            args: vec![from, Type::Any],
        })
    }

    /// Replace a method's own type parameters by their upper bounds, so that
    /// a specificity test cannot instantiate them.
    fn rigidify_own_tparams(&self, sym: SymbolId, ps: &[Type]) -> Vec<Type> {
        if sym.is_none() {
            return ps.to_vec();
        }
        let tps = self.st.get(sym).tparams.clone();
        if tps.is_empty() {
            return ps.to_vec();
        }
        let his: Vec<Type> = tps
            .iter()
            .map(|t| self.st.get(*t).bound_hi.clone().unwrap_or(Type::Any))
            .collect();
        ps.iter()
            .map(|p| crate::symbol::subst_tparams_slice(&tps, &his, p))
            .collect()
    }

    /// The arity nsc's `Infer.shapeType` would give a parameter: how many
    /// arguments a function literal filling it is written with.
    ///
    /// `None` means "no opinion" -- the parameter is not function-shaped, so a
    /// literal's arity says nothing about it.
    fn shape_arity(&self, param: &Type) -> Option<usize> {
        match crate::prefix::strip_view(param) {
            Type::ByName(inner) | Type::Repeated(inner) => self.shape_arity(inner),
            Type::Function { params, .. } => Some(params.len()),
            Type::Class { sym, args } => {
                if let Some(Type::Function { params, .. }) =
                    self.st.function_class_shape(*sym, args)
                {
                    return Some(params.len());
                }
                // nsc's shape for a `{ case … }` literal is
                // `PartialFunction[Any, Nothing]`, i.e. a `Function1`.
                partial_function_type(&self.st, param).map(|_| 1)
            }
            _ => None,
        }
    }

    /// The other half of nsc's `preSelectOverloaded`: the shape of a function
    /// literal says *what* it is, not only how many parameters it takes.
    ///
    /// `x => e` has shape `FunctionN[Any, …, Nothing]`, and that is not a
    /// `PartialFunction` -- nor SAM-convertible to one, since
    /// `PartialFunction` declares two abstract members -- so an alternative
    /// whose formal at that position is a `PartialFunction` is thrown away
    /// before specificity is weighed. `{ case … }` has shape
    /// `PartialFunction[Any, Nothing]`, which fits both kinds of formal, and
    /// is left to specificity (where the `PartialFunction` one is the more
    /// specific and wins).
    ///
    /// This is a *pre-selection*, so it only ever narrows a choice that
    /// already exists: with the `PartialFunction` alternative the only one,
    /// nothing is dropped and the literal is adapted as usual
    /// (`def f(p: PartialFunction[Int, Int]); f((x: Int) => x)` compiles for
    /// scalac 2.13.16, and the same call against an overload set does not
    /// choose that alternative).
    ///
    /// `PartialFunction.andThen` is the pair that needs it:
    /// `andThen[C](k: B => C)` and `andThen[C](k: PartialFunction[B, C])` are
    /// both applicable to an un-inferred literal, and the second is strictly
    /// the more specific, so without the shape every `pf.andThen(s => …)`
    /// would silently compose *domains* -- a wrong `isDefinedAt` at run time,
    /// not a compile error.
    fn narrow_by_partial_function_shape(
        &self,
        applicable: Vec<(SymbolId, Vec<Type>, Type)>,
        shapes: &[ArgShape],
    ) -> Vec<(SymbolId, Vec<Type>, Type)> {
        if !shapes.contains(&ArgShape::Function) {
            return applicable;
        }
        let kept: Vec<(SymbolId, Vec<Type>, Type)> = applicable
            .iter()
            .filter(|(_, ps, _)| {
                shapes.iter().enumerate().all(|(i, shape)| {
                    *shape != ArgShape::Function
                        || match param_at(ps, i) {
                            Some(p) => {
                                partial_function_type(&self.st, strip_param_wrappers(p)).is_none()
                            }
                            None => true,
                        }
                })
            })
            .cloned()
            .collect();
        if kept.is_empty() {
            applicable
        } else {
            kept
        }
    }

    /// nsc's shape-type pass (`Infer.shapeType`, used by
    /// `inferMethodAlternative` before the arguments are typed): a function
    /// literal whose parameter *types* are still unknown already has a fixed
    /// **arity**, and that alone throws out alternatives.
    ///
    /// ```scala
    /// def only(action: Repo => Any): Int
    /// def only[T](action: (T, Repo) => Any): T => Int
    /// only { r => r.nm }        // Function1: only the first alternative
    /// only[T] { (f, r) => … }   // Function2: only the second
    /// ```
    ///
    /// [`Self::arg_score`] deliberately lets an un-inferred literal match a
    /// function parameter of *any* arity, because a `{ case … }` literal is
    /// written with one parameter and still inhabits a `(A, B) => C` by
    /// tupling. So the arity is applied here instead, and only as a filter
    /// that **narrows an already ambiguous set**: a lone applicable
    /// alternative is never rejected, and neither is a set the shape has no
    /// opinion about.
    fn narrow_by_lambda_shape(
        &self,
        applicable: Vec<(SymbolId, Vec<Type>, Type)>,
        args: &[Type],
        shapes: &[ArgShape],
    ) -> Vec<(SymbolId, Vec<Type>, Type)> {
        if applicable.len() < 2 {
            return applicable;
        }
        let applicable = self.narrow_by_partial_function_shape(applicable, shapes);
        if applicable.len() < 2 {
            return applicable;
        }
        // The literal placeholder `type_apply` pushes for an argument it has
        // not been able to type yet: every parameter and the result unknown.
        let shapes: Vec<Option<usize>> = args
            .iter()
            .map(|a| match a {
                Type::Function { params, ret }
                    if ret.is_no_type()
                        && !params.is_empty()
                        && params.iter().all(|p| p.is_no_type()) =>
                {
                    Some(params.len())
                }
                _ => None,
            })
            .collect();
        if shapes.iter().all(|s| s.is_none()) {
            return applicable;
        }
        let kept: Vec<(SymbolId, Vec<Type>, Type)> = applicable
            .iter()
            .filter(|(_, ps, _)| {
                shapes.iter().enumerate().all(|(i, shape)| {
                    let Some(n) = shape else { return true };
                    match param_at(ps, i).and_then(|p| self.shape_arity(p)) {
                        Some(m) => m == *n,
                        None => true,
                    }
                })
            })
            .cloned()
            .collect();
        if kept.is_empty() {
            applicable
        } else {
            kept
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn is_applicable(
        &self,
        sym: SymbolId,
        clause: usize,
        params: &[Type],
        args: &[Type],
        allow_widen: bool,
        targs: &[Type],
        expected: Option<(&Type, &Type)>,
    ) -> bool {
        // Context may rescue a singleton, but must not discard an ordinarily
        // applicable call before adaptation can diagnose its precise mismatch.
        if expected.is_some()
            && self.is_applicable(sym, clause, params, args, allow_widen, targs, None)
        {
            return true;
        }
        let instantiated;
        let params = if !sym.is_none() && !self.st.get(sym).tparams.is_empty() {
            // Explicit type arguments *are* the instantiation (SLS 6.26.3);
            // nsc applies them to every alternative in `inferPolyAlternatives`
            // before applicability is weighed. Only when the alternative takes
            // exactly as many as were written -- an alternative of some other
            // arity is not the one they were written for, and nsc drops it
            // from the set entirely.
            let tps = self.st.get(sym).tparams.clone();
            let inst: Vec<(SymbolId, Type)> = if targs.len() == tps.len() {
                tps.iter().copied().zip(targs.iter().cloned()).collect()
            } else {
                self.infer_method_tparams(sym, params, args)
            };
            let inst = if targs.len() != tps.len() {
                match expected {
                    Some((ret, pt)) => self.add_expected_constraints(sym, ret, pt, inst),
                    None => inst,
                }
            } else {
                inst
            };
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
        let open: Vec<SymbolId> = if sym.is_none() {
            Vec::new()
        } else {
            self.st.get(sym).tparams.clone()
        };
        let open = &open[..];
        let conforms = |a: &Type, p: &Type| {
            self.arg_conforms(a, p, allow_widen, open)
                // Unit value discarding belongs to a resolved call, not to
                // choosing among overloads. expected is supplied only for a
                // singleton, after the ordinary applicability attempt.
                || (expected.is_some()
                    && matches!(p, Type::ByName(t) if matches!(t.as_ref(), Type::Unit)))
        };
        let (fixed, repeated) = split_repeated(params);
        if let Some(elem) = repeated {
            if args.len() < fixed.len() {
                return false;
            }
            return args.iter().zip(fixed).all(|(a, p)| conforms(a, p))
                && args[fixed.len()..].iter().all(|a| conforms(a, elem));
        }
        // Only a repeated parameter list takes a repeated *argument*. nsc's
        // `<repeated>[T]` is a type of its own, and it conforms to no ordinary
        // formal -- not to `T`, not to `Any`, not through a view. Two
        // different things reach here as one:
        //
        //  * an `f(xs: _*)` splice, which must not be accepted by a
        //    fixed-arity alternative and silently passed as a single element;
        //  * a declared `T*` weighed as an *argument* type by
        //    `is_as_specific_method`, which is what makes a varargs
        //    alternative lose to its fixed-arity sibling. See `spec_argtpes`
        //    for the six scalac 2.13.16 measurements that pin this, including
        //    the two ties it must *not* break.
        if args.iter().any(|a| matches!(a, Type::Repeated(_))) {
            return false;
        }
        if args.len() > params.len() {
            return false;
        }
        if args.len() < params.len()
            && !self.trailing_omissible(sym, clause, args.len(), params.len())
        {
            return false;
        }
        args.iter().zip(params).all(|(a, p)| conforms(a, p))
    }

    /// `open` are the callee's type parameters this call has not settled;
    /// a parameter type that mentions one can still be reached by a view whose
    /// own result says what it is (`xs.to(Vector)`).
    pub(crate) fn arg_conforms(
        &self,
        arg: &Type,
        param: &Type,
        allow_widen: bool,
        open: &[SymbolId],
    ) -> bool {
        // Numeric widening is weak conformance, which nsc's applicability
        // (`isWeaklyCompatible`) uses from the first try: `f(3)` against
        // `f(x: AnyVal)` and `f(x: Double)` has both applicable, and the more
        // specific `Double` wins (run/t12560). Holding widening back to the
        // view round made the `AnyVal` one the only candidate.
        match self.arg_score(arg, param) {
            Some(_) => true,
            None if allow_widen => {
                // Narrowing an `Int` literal (`take(3)` on a `Byte` parameter)
                // is a fallback, like a view: without that, `sb.append(42)`
                // would match `append(Char)` as readily as `append(Int)`.
                self.st.narrows_to(arg, param)
                    // nsc view: `wrapString` makes String applicable to Seq.
                    || !matches!(self.search_conversion(arg, param), ImplicitSearch::None)
                    || (mentions_tparam(param, open)
                        && self.search_conversion_open(arg, param, open).is_some())
                    // `Predef`'s array wrappings are views too, but they are
                    // not `implicit` in this prelude (`seqfn_view.rs` says
                    // why), so `search_conversion` cannot find them.
                    || self.array_wrap_conforms(arg, param, open)
            }
            None => false,
        }
    }

    /// Whether this alternative is applicable only because a **default** would
    /// complete it.
    ///
    /// nsc's `Infer.inferMethodAlternative` weighs the alternatives applicable
    /// to the arguments *as written* first, and reaches for the ones a default
    /// would complete only when that set is empty. Weighing both at once made
    /// `new Prefer(1)` on
    ///
    /// ```text
    /// class Prefer(n: Int) { def this(k: Int, bump: Int = 5) = this(k * 100 + bump) }
    /// ```
    ///
    /// `ambiguous overload for constructor`: the primary and the secondary are
    /// each applicable to one `Int`, the secondary only because `bump` has a
    /// default. nsc picks the primary and prints `1`.
    ///
    /// An *implicit* parameter is not a default. It is filled by a search that
    /// runs after the alternative has been chosen, and nsc's applicability
    /// ignores the implicit clause outright -- which is why this asks for
    /// `DEFAULTPARAM` and not for `trailing_omissible`'s weaker
    /// "default *or* implicit".
    pub(crate) fn needs_a_default(
        &self,
        sym: SymbolId,
        clause: usize,
        given: usize,
        total: usize,
    ) -> bool {
        if sym.is_none() || given >= total {
            return false;
        }
        let s = self.st.get(sym);
        // The same reading `trailing_omissible` does: a residual clause has to
        // come out of `paramss`, while `pick_ctor` hands over a constructor
        // whose clauses are already flattened.
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
        ids[given..total]
            .iter()
            .any(|p| self.st.get(*p).flags.contains(Flags::DEFAULTPARAM))
    }

    pub(crate) fn trailing_omissible(
        &self,
        sym: SymbolId,
        clause: usize,
        given: usize,
        total: usize,
    ) -> bool {
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

    /// `Seq[T]` for a repeated parameter's element type — the type nsc gives
    /// `xs: T*` *inside* the body.
    ///
    /// `None` when this run has no `scala.collection.immutable.Seq` at all,
    /// which `--no-scala-library` mode has not: the private runtime
    /// (`backend::runtime`) ships no `Seq`, so a repeated parameter cannot be
    /// used as a value there, and the caller leaves the `T*` in place and lets
    /// the member lookup report it.
    pub(crate) fn seq_of(&self, elem: &Type) -> Option<Type> {
        self.the_seq_class().map(|sym| Type::Class {
            sym,
            args: vec![elem.clone()],
        })
    }

    /// nsc's `definitions.SeqClass`: the class whose binary name is
    /// `scala/collection/immutable/Seq`, wherever this run gets it from — the
    /// prelude (which owns its `Seq` by package `scala`, the jar's `scala.Seq`
    /// alias already collapsed), or a source `package scala.collection.
    /// immutable`, which is what `tests/scalalib_measure.sh` compiles.
    ///
    /// It is deliberately **not** a scope lookup. nsc's is a fixed symbol, and
    /// asking the scope was wrong in both directions:
    ///
    /// * it found the wrong `Seq`. A program may bind the name to something of
    ///   its own, and `object Main { class Seq[A] { def tag = "MINE" };
    ///   def f(xs: Int*) = xs.tag }` then *compiled* — `gen_desc` writes
    ///   `Lscala/collection/immutable/Seq;` for a repeated parameter whatever
    ///   the typer decided, so the emitted `invokevirtual Main$Seq.tag` met an
    ///   `ArraySeq$ofInt` and threw `ClassCastException` with no diagnostic
    ///   anywhere. scalac 2.13.16 reports `value tag is not a member of
    ///   Seq[Int]`, and so does this now
    ///   (`tests/fixtures/varargsrecv_shadow_bad.scala`).
    /// * it found no `Seq` where there was one. A run that compiles the
    ///   standard library from source binds the name only as
    ///   `scala/package.scala`'s `type Seq[+A] = scala.collection.immutable.
    ///   Seq[A]`, an alias — and in a file that writes a single qualified
    ///   package clause (`package scala.jdk`) not even that, because a source
    ///   `type` alias in `scala/package.scala` is not entered into the
    ///   `scala._` auto-import scope the way a source class or object is
    ///   (`Typer::auto_import_scala_member`). Every repeated parameter in the
    ///   library was left as the bare `T*`: `value length is not a member of
    ///   Short*`, `Unit*`, `T*`, `Array[T]*`, and 48 more.
    fn the_seq_class(&self) -> Option<SymbolId> {
        const BINARY: &str = "scala/collection/immutable/Seq";
        let is_it = |s: &SymbolId| {
            let info = self.st.get(*s);
            info.kind == SymKind::Class && info.jvm_name == BINARY
        };
        if self.st.scala_pkg.is_none() {
            return None;
        }
        if let Some(s) = self
            .st
            .lookup_member(self.st.scala_pkg, "Seq")
            .iter()
            .find(|s| is_it(s))
        {
            return Some(*s);
        }
        let mut pkg = self.st.scala_pkg;
        for seg in ["collection", "immutable"] {
            pkg = self
                .st
                .lookup_member(pkg, seg)
                .into_iter()
                .find(|s| self.st.get(*s).kind == SymKind::Package)?;
        }
        self.st
            .lookup_member(pkg, "Seq")
            .iter()
            .find(|s| is_it(s))
            .copied()
    }

    /// `arg` seen as the structural function type it inherits, if it does.
    /// `Map[K, V]`, a `PartialFunction`, and a user class extending `A => B`
    /// all have one; a plain class has none.
    pub(crate) fn function_view(&self, arg: &Type) -> Option<Type> {
        // A `scala.FunctionN` *class* already is a function everywhere it
        // matters; rewriting it structurally here only made both alternatives
        // of slick's `map` calls applicable at once. This is about a type that
        // is a function by *inheritance*.
        if matches!(arg, Type::Function { .. }) {
            return None;
        }
        if let Type::Class { sym, args } = arg {
            if self.st.function_class_shape(*sym, args).is_some() {
                return None;
            }
        }
        // `base_type_seq` lists the *ancestors*; a `PartialFunction[A, B]`
        // named outright is its own view.
        let bases = std::iter::once(arg.clone()).chain(self.st.base_type_seq(arg));
        for base in bases {
            // `class Conv[-A, +B] extends (A => B)` — and `<:<` itself — record
            // that parent as a structural `Type::Function`, not as an applied
            // `Function1` *class*. Skipping it left `flatMap(ev)` on slick's
            // `DBIOAction.flatten` with nothing to read `R2` out of: the
            // conversion conformed (`val g: R => Act[R2] = ev` typed fine) but
            // the callee's type parameters stayed open, and the call was
            // reported as `no matching overload`.
            if matches!(base, Type::Function { .. }) {
                return Some(base);
            }
            if let Type::Class { sym, args } = &base {
                if let Some(f) = self.st.function_class_shape(*sym, args) {
                    return Some(f);
                }
            }
            // `PartialFunction[A, B]` is a `A => B` without being a
            // `Function1` *class* in the prelude's parent chain.
            if let Some((from, to)) = partial_function_type(&self.st, &base) {
                return Some(Type::Function {
                    params: vec![from],
                    ret: Box::new(to),
                });
            }
            // `MapOps[K, +V, …] extends IterableOps[…] with
            // PartialFunction[K, V]`, so a `Map` *is* the function that looks a
            // key up. The prelude has no `MapOps`, and giving `Map` a
            // `PartialFunction` parent outright reorders the inherited-member
            // walk enough to break `toMap`'s `A <:< (K, V)`, so the fact is
            // stated here instead of as an edge.
            if let Type::Class { sym, args } = &base {
                if args.len() == 2 && self.st.get(*sym).jvm_name.ends_with("/Map") {
                    return Some(Type::Function {
                        params: vec![args[0].clone()],
                        ret: Box::new(args[1].clone()),
                    });
                }
            }
        }
        None
    }

    /// Insert the views nsc's `inferView` inserts *before* it solves a call's
    /// type parameters.
    ///
    /// `xs.to(Vector)` is `to[C1](f: Factory[A, C1]): C1`: the argument is
    /// `Vector.type`, the parameter mentions the still-open `C1`, and only
    /// `IterableFactory.toFactory` makes the two meet -- and says that `C1` is
    /// `Vector[A]`. Rewriting the argument here lets ordinary inference see
    /// `Factory[Int, Vector[Int]]` and solve `C1` from it, with no separate
    /// path for `to`.
    pub(crate) fn apply_open_views(
        &mut self,
        sym: SymbolId,
        param_tys: &[Type],
        args: &mut [Tree],
        arg_tys: &mut [Type],
    ) {
        if sym.is_none() {
            return;
        }
        let open = self.st.get(sym).tparams.clone();
        if open.is_empty() {
            return;
        }
        for i in 0..args.len() {
            let Some(p) = param_at(param_tys, i).cloned() else {
                continue;
            };
            if !mentions_tparam(&p, &open) {
                continue;
            }
            let a_ty = args[i].ty.clone();
            if a_ty.is_no_type() || a_ty.is_error() || self.arg_score(&a_ty, &p).is_some() {
                continue;
            }
            if self.open_param_already_fits(&a_ty, &p, &open) {
                continue;
            }
            let Some((id, solved, _)) = self.search_conversion_open(&a_ty, &p, &open) else {
                continue;
            };
            let span = args[i].span;
            let mut arg = std::mem::replace(&mut args[i], Tree::dummy(TreeKind::Empty));
            let from = arg.ty.clone();
            // A by-name view parameter takes the argument as its thunk.
            if let Some(bn @ Type::ByName(_)) = self.conv_first_param(id) {
                self.adapt(&mut arg, &bn);
            }
            let fun = self.ref_implicit(id, span);
            let applied = Tree {
                id: arg.id,
                span,
                kind: TreeKind::Apply {
                    fun: Box::new(fun),
                    args: vec![arg],
                },
                ty: solved.clone(),
                sym: id,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
                byname_thunk: false,
                byname_type_marker: false,
            };
            let mut filled = self.fill_conv_implicits(id, &from, applied, span);
            filled.ty = solved.clone();
            args[i] = filled;
            if let Some(t) = arg_tys.get_mut(i) {
                *t = solved;
            }
        }
    }

    /// nsc's `isCompatible` with the callee's own type parameters still
    /// undetermined: does the argument fill this parameter *as it is*, for
    /// some instantiation of the parameters the call has not settled yet?
    ///
    /// `arg_score` cannot answer that -- it reads an unsolved type parameter
    /// as a rigid type -- so a call whose parameter mentions one looked
    /// inapplicable and `apply_open_views` reached for a view. The view then
    /// wrapped an argument that already conformed, and the wrapping is what
    /// the call's inference read afterwards:
    ///
    /// ```scala
    /// class Rep[T]; class Lit[T](v: T) extends Rep[T]
    /// implicit def toRep[T](v: T): Rep[T] = new Lit[T](v)
    /// def ===[P2, R](e: Rep[P2])(implicit om: OM[B1, P2, R]): String
    /// col === new Lit[Long](9L)
    /// ```
    ///
    /// `Lit[Long]` *is* a `Rep[Long]`, so nsc solves `P2 = Long` and finds
    /// `OM[Long, Long, Boolean]`. We converted it to a `Rep[Lit[Long]]` first
    /// and then asked for `OM[Long, Lit[Long], R]`, which nothing supplies --
    /// and, being the wrong answer rather than a missing one, it took every
    /// implicit downstream of it with it.
    ///
    /// The argument is read at the parameter's own class first
    /// (`align_arg_to_param`), because `unify_one` zips type arguments
    /// positionally and has no symbol table to walk base types with.
    fn open_param_already_fits(&self, arg: &Type, param: &Type, open: &[SymbolId]) -> bool {
        let mentioned: Vec<SymbolId> = open
            .iter()
            .copied()
            .filter(|tp| type_mentions_tparam(param, *tp))
            .collect();
        if mentioned.is_empty() {
            return false;
        }
        let aligned = self.align_arg_to_param(param, arg);
        match self.solve_open_from_arg(&aligned, param, &mentioned) {
            Some(solved) => self.arg_score(arg, &solved).is_some(),
            None => false,
        }
    }

    fn arg_score(&self, arg: &Type, param: &Type) -> Option<i32> {
        if let Type::ByName(inner) = param {
            if let Some(s) = self.arg_score(arg, inner) {
                return Some(s);
            }
            return None;
        }
        // `adapt` keeps an existing by-name identifier as `ByName(T)` so the
        // erasure pass can forward its thunk. Overload applicability still
        // compares the value yielded by that thunk with the formal value type
        // (or with another by-name formal), just as it does for the
        // `Function0[T]` shape produced for a fresh by-name argument.
        if let Type::ByName(inner) = arg {
            return self.arg_score(inner, param);
        }
        // A `xs: _*` argument is already the sequence the parameter wants.
        // Whether the *parameter list* takes one at all is `is_applicable`'s
        // question, not this one.
        if let Type::Repeated(inner) = arg {
            return self.arg_score(inner, param);
        }
        if let Type::Repeated(inner) = param {
            return self.arg_score(arg, inner);
        }
        // `scala.FunctionN[T1, …, R]` *is* the function type. A signature read
        // back from a pickle spells a function parameter as the class
        // (`reduceLeft[B >: A](op: Function2[B, A, B])`), and the
        // function-against-function rule below has to see it: a literal whose
        // parameters are not inferred yet would otherwise be inapplicable to
        // every such method.
        if let Type::Class { sym, args } = crate::prefix::strip_view(param) {
            if let Some(f) = self.st.function_class_shape(*sym, args) {
                return self.arg_score(arg, &f);
            }
            // A parameter that is merely SAM-shaped (`trait SetParameter[-T]
            // extends ((T, PositionedParameters) => Unit)`, not literally a
            // `scala.FunctionN`) still accepts a function literal, same as
            // real scalac's SAM conversion. Gated on `arg` already being a
            // `Type::Function` -- an ordinary class-typed value is never SAM-
            // convertible, only a literal (or an already-function-typed
            // argument) is. Without this, `SQLActionBuilder(sql, (u, pp) =>
            // ...)` against `case class SQLActionBuilder(sql: String,
            // setParameter: SetParameter[Unit])` scored no match at all even
            // after `agreed_lambda_params` gave the literal real parameter
            // types, because the *parameter* side was still only compared as
            // a plain class.
            if matches!(arg, Type::Function { .. }) {
                if let Some(sam) = self.st.sam_sig(param) {
                    let f = Type::Function {
                        params: sam.param_tys,
                        ret: Box::new(sam.ret_ty),
                    };
                    return self.arg_score(arg, &f);
                }
            }
        }
        if let Type::Method { paramss, ret } = arg {
            // A curried method eta-expands to a curried function:
            // `def ite(b: Boolean)(x: A, y: A): A` is `Boolean => (A, A) => A`
            // (cats' `Apply.ifA` passes it to `map(fcond)(ite)`), not the
            // `(Boolean, A, A) => A` flattening every clause gave, which no
            // one-parameter function parameter could take.
            let f = paramss
                .iter()
                .rev()
                .fold((**ret).clone(), |acc, clause| Type::Function {
                    params: clause.clone(),
                    ret: Box::new(acc),
                });
            let f = if paramss.is_empty() {
                Type::Function {
                    params: Vec::new(),
                    ret: ret.clone(),
                }
            } else {
                f
            };
            return self.arg_score(&f, param);
        }
        // An overloaded method named as an argument (`constOp[Long]("min")(math.min)`)
        // stands for whichever alternative the parameter takes; `adapt` picks
        // that one once the callee is settled.
        if let Type::Overload(alts) = arg {
            return alts.iter().filter_map(|a| self.arg_score(a, param)).max();
        }
        if self.st.is_sub_type(arg, param) {
            return Some(if arg == param { 10 } else { 5 });
        }
        // Two function types score as a match while their parameter types are
        // still open (an un-inferred lambda literal), but not when they
        // plainly disagree: `scala.Function.untupled` overloads on nothing but
        // the arity of its argument's tuple parameter, so `((Int, Int)) => Int`
        // has to reject `((T1, T2, T3)) => R`.
        if let (
            Type::Function {
                params: ap,
                ret: ar,
            },
            Type::Function {
                params: pp,
                ret: pr,
            },
        ) = (arg, param)
        {
            // A literal whose parameters are not inferred yet keeps matching
            // on any shape: `apply2 { case (n, s) => … }` arrives as one
            // unknown parameter and only becomes a `(Int, String) => String`
            // once the parameter it fills is known.
            let open = ap.iter().any(|t| t.is_no_type());
            let shapes_agree = open
                || (ap.len() == pp.len()
                    && ap.iter().zip(pp).all(|(a, p)| {
                        match (tuple_arity(&self.st, a), tuple_arity(&self.st, p)) {
                            (Some(x), Some(y)) => x == y,
                            _ => true,
                        }
                    }));
            if shapes_agree {
                // A literal that is *already* typed has to return what the
                // parameter asks for. `StringOps` overloads `map` on nothing
                // but the function's result type, so `Char => String` must not
                // score as a `Char => Char`. A `Unit` (or `Any`) parameter
                // still takes any result -- that is value discarding, which
                // nsc allows for function literals too -- and an undetermined
                // result constrains nothing. Preserve the surrounding shape:
                // Tuple2[K, V] cannot accept Int just because K and V are open.
                let mut variables = Vec::new();
                collect_tparams(pr, &mut variables);
                let result_shape = crate::symbol::subst_tparams_slice(
                    &variables,
                    &vec![Type::Wildcard; variables.len()],
                    pr,
                );
                let strict = !open
                    && is_rigid_type(ar)
                    && !matches!(**pr, Type::Unit | Type::Any | Type::AnyRef);
                if strict
                    && !self.st.is_sub_type(ar, &result_shape)
                    && numeric_widen(ar, &result_shape).is_none()
                {
                    return None;
                }
                return Some(8);
            }
        }
        // A `{ case … }` literal reaches overload resolution as a one-parameter
        // function; it inhabits a `PartialFunction[A, B]` parameter
        // (`Try.recover`, `Option.collect`, `List.collect`). A literal that is
        // not typed yet scores better, so `collect` can infer `B` from the
        // case bodies rather than from an open type parameter.
        //
        // This is the *literal* being adapted, which is `typedFunction`'s job
        // in nsc, and it has no business in a specificity comparison. There
        // both sides are declared signatures -- never trees -- and nsc weighs
        // them with `isCompatible`, which has no function-to-`PartialFunction`
        // coercion at all (`PartialFunction` declares two abstract members, so
        // it is not a SAM type either; a `val g: Int => Int` passed to a
        // `PartialFunction[Int, Int]` parameter is a `type mismatch` for
        // scalac 2.13.16). Scoring a match here made
        // `PartialFunction.andThen[C](k: B => C)` as specific as
        // `andThen[C](k: PartialFunction[B, C])` -- each was as specific as
        // the other and every `pf.andThen(…)` was `ambiguous overload`.
        if let Type::Function { params, ret } = arg {
            if params.len() == 1
                && !self.spec_probe.get()
                && partial_function_type(&self.st, param).is_some()
            {
                return if params[0].is_no_type() && ret.is_no_type() {
                    Some(6)
                } else {
                    Some(7)
                };
            }
        }
        // An argument that is still carrying undetermined type variables
        // (`Map.empty` typed with no expected type is `Map[K, V]`) is
        // applicable to whatever those variables can be made into. nsc keeps
        // them as `TypeVar`s until the call is solved; they are solved here
        // against the parameter and the instantiation is what is compared.
        if self.undet_compatible(arg, param) {
            return Some(4);
        }
        // The *parameter* can be the side that is still open. `Set()` is
        // `Set[?A]` -- nothing has said what `?A` is yet -- so the `++`
        // selected on it takes an `IterableOnce[?A]`, and the argument is what
        // settles it (`-Xprint:typer` on `Set() ++ o` shows nsc typing the
        // receiver as `Set.apply[String]()`, read off that same argument).
        // `undet_compatible` above only looks at the variables the *argument*
        // carries. The `OverloadPick::Found` path already substitutes the
        // solution into the parameters, the result and the receiver; without
        // this the alternative never got that far and `Set() ++ o` was `no
        // matching overload`.
        if !self.undet_tvars.is_empty() && !self.spec_probe.get() {
            let open: Vec<SymbolId> = self
                .undet_tvars
                .iter()
                .copied()
                .filter(|tp| type_mentions_tparam(param, *tp))
                .collect();
            if !open.is_empty() {
                if let Some(solved) = self.solve_open_from_arg(arg, param, &open) {
                    if let Some(s) = self.arg_score(arg, &solved) {
                        return Some(s.min(4));
                    }
                }
            }
        }
        // A *parameter* that is a bare type variable of the alternative takes
        // anything -- that is what `def f[T](x: T)` means, and the variable is
        // not in `undet_tvars` while alternatives are being scored.
        //
        // The *argument* being a bare type parameter is a different matter: a
        // rigid `R` is only what its bounds say it is, and `is_sub_type`
        // already widens it to its upper bound above. Scoring it as applicable
        // to every parameter made every alternative of every Java overload set
        // match: `String.valueOf(value)` for a `value: R` was `ambiguous
        // overload` rather than the `valueOf(Object)` nsc picks.
        if matches!(param, Type::TypeParam(_)) {
            return Some(2);
        }
        // A rigid type parameter *argument* is what its upper bound is, and no
        // more. `is_sub_type` already widened it above; this is the second
        // chance, for a parameter that mentions the alternative's own still
        // open variables. slick's `Comprehension[+Fetch <: Option[Node]]`
        // passes its `fetch: Fetch` to `mapOrNone[A](Option[A])(A => A)` and to
        // `orElse[B >: A](=> Option[B])`, and only `Option[Node]` conforms to
        // either.
        if let Type::TypeParam(id) = arg {
            let hi = self.st.get(*id).bound_hi.clone();
            if let Some(hi) = hi {
                if !matches!(hi, Type::TypeParam(_)) {
                    if let Some(s) = self.arg_score(&hi, param) {
                        return Some(s.min(2));
                    }
                }
            }
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
        // AnyRef and AnyVal are disjoint domains. Subtyping, inference and
        // conversions above decide applicability; neither is a catch-all.
        // Last: a class that *is* a function inhabits a function parameter.
        // `MapOps[K, V, …] extends PartialFunction[K, V]`, so `xs.map(aMap)`
        // hands `map` a `K => V`. Only as a fallback -- weighed earlier it
        // made alternatives applicable that nothing else had matched, and
        // slick's `map` calls went from resolved to ambiguous.
        if !matches!(arg, Type::Function { .. }) && is_function_pt(param) {
            if let Some(view) = self.function_view(arg) {
                return self.arg_score(&view, param);
            }
            // `List(0, 2).filter(anArrayOfBoolean)`: the argument itself is
            // not a function, but `Predef.wrapBooleanArray` turns it into one
            // (`mutable.ArraySeq[Boolean] <: Seq[Boolean] <:
            // PartialFunction[Int, Boolean] <: Int => Boolean`,
            // `seqfn_view.rs`). Scored, not adapted -- `adapt` inserts the
            // real call once this alternative is picked.
            if let Type::Array(elem) = arg {
                if let Some((_, view)) = self.array_seq_wrap(elem) {
                    return self.arg_score(&view, param);
                }
            }
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
        if matches!(probe.ty, Type::Overload(_)) {
            return none;
        }
        // A function *value* is a callee too: nsc types `meth` against a
        // method expectation, and `f: (S, A) => (S, B)` answers with its
        // `apply`. `s => f(s, a)` inside `State(…)` is cats' `mapAccumulate`,
        // where the expected parameter type is a variable nothing has fixed
        // yet, and `f`'s own parameter is the only thing that says `s: S`. A
        // rigid type parameter of the enclosing method is a fixed type there;
        // one this call is still solving (`undet_tvars`) is not.
        if let Type::Function { params, .. } = &probe.ty {
            if params.len() != args.len() {
                return none;
            }
            let mut out = none;
            for (pi, ai) in at.iter().enumerate() {
                let t = params[*ai].clone();
                let undet = matches!(&t, Type::TypeParam(id) if self.undet_tvars.contains(id));
                if !t.is_no_type() && !t.is_error() && !undet && !type_has_wildcard(&t) {
                    out[pi] = t;
                }
            }
            return out;
        }
        if probe.sym.is_none() || !self.st.get(probe.sym).tparams.is_empty() {
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

    /// nsc's `typedFunctionUndoingEtaExpansion` for a literal whose expected
    /// parameter types this call has not decided: the parameter type
    /// `params[i]` mentions one of `open`, so the argument is about to be typed
    /// against that variable's bound, and the literal's body -- an application
    /// that passes the parameter straight on -- may say more. Returns the
    /// expected parameter types with those positions replaced by what the
    /// callee states, or `None` when there is nothing to replace.
    pub(crate) fn undo_eta_param_types(
        &mut self,
        a: &Tree,
        params: &[Type],
        open: &[SymbolId],
    ) -> Option<Vec<Type>> {
        if !is_bare_lambda(a) {
            return None;
        }
        let TreeKind::Function { vparams, body } = &a.kind else {
            return None;
        };
        if vparams.len() != params.len() || !params.iter().any(|p| mentions_tparam(p, open)) {
            return None;
        }
        let (vparams, body) = (vparams.clone(), (**body).clone());
        let from_section = self.section_param_types(&vparams, &body);
        let mut out = params.to_vec();
        let mut changed = false;
        for (i, p) in params.iter().enumerate() {
            if mentions_tparam(p, open) && !from_section[i].is_no_type() {
                out[i] = from_section[i].clone();
                changed = true;
            }
        }
        changed.then_some(out)
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
        fun.scala_ref = true;
        let mut tup = Tree::dummy(TreeKind::Apply {
            fun: Box::new(fun),
            args,
        });
        tup.span = span;
        **selector = tup;
        *vparams = params;
    }

    pub(crate) fn type_function(
        &mut self,
        vparams: &mut Vec<Tree>,
        body: &mut Tree,
        pt: &Type,
    ) -> Type {
        // A `scala.FunctionN[T1, …, R]` *class* is the function type. The
        // prelude writes function parameters structurally, but a signature read
        // back from a pickle spells them as the class
        // (`IterableOnceOps.reduceLeft[B >: A](op: Function2[B, A, B]): B`), and
        // a literal passed to one of those was left with no parameter types at
        // all: `xs.reduceLeft[Node]((a, b) => …)` reported
        // `no matching overload … with arguments ((<notype>, <notype>) => <notype>)`.
        let pt = match pt {
            Type::ByName(inner) => inner.as_ref(),
            other => other,
        };
        let as_fn = match pt {
            Type::Class { sym, args } => self.st.function_class_shape(*sym, args),
            _ => None,
        };
        let pt = as_fn.as_ref().unwrap_or(pt);
        // Only a `{ case … }` literal inhabits a `PartialFunction`; the parser
        // encodes one as `x$pf => x$pf match { … }`. A total function literal
        // must still be rejected, the way nsc rejects
        // `t.recover((x: Int) => x + 1)`.
        // nsc: `{ case (a, b) => … }` where a `FunctionN` is expected takes N
        // parameters and matches the N-tuple of them, not one parameter.
        if is_case_block_literal(vparams, body) {
            // A SAM is expanded the same way: cats-kernel's
            // `implicit def catsStdEqForTry[A](…): Eq[Try[A]] = { case
            // (Success(a), Success(b)) => … }` is a two-parameter literal
            // matching the pair, because `Eq`'s single abstract method takes
            // two. Reading only `FunctionN` here left the one parameter the
            // parser wrote with no type: `missing parameter type for expanded
            // function`.
            let arity = expected_function_arity(pt)
                .or_else(|| self.st.sam_sig(pt).map(|s| s.param_tys.len()));
            if let Some(n) = arity {
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
            let to = if matches!(to, Type::Any) || self.pt_says_nothing(to) {
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
        // `Wildcard` is what `pretypeArgs` writes: the parameters are fixed,
        // the result is whatever the body turns out to be.
        let ret_pt = if matches!(ret_pt, Type::TypeParam(_) | Type::Wildcard)
            || self.pt_says_nothing(&ret_pt)
        {
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
        // Synthesized bodies must not all share the dummy node identity.
        if body.id.0 == 0 {
            body.id = scala_rs_parser::NodeId(self.macro_next_node);
            self.macro_next_node = self
                .macro_next_node
                .checked_add(1)
                .expect("function node identity exhausted");
        }
        let function = self.macro_function_symbol(body.id);
        let saved_macro_owner = self.macro_lexical_owner;
        self.macro_lexical_owner = function;
        // Parameters and block locals belong to the function even when its
        // expression is a field initializer. Nested classes must capture them.
        let saved_owner = self.st.owner;
        self.st.owner = function;
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
            if matches!(p.ty, Type::ByName(_)) {
                if let TreeKind::ValDef { mods, .. } = &mut p.kind {
                    mods.flags = mods.flags.with(Flags::BYNAME);
                }
                if !p.sym.is_none() {
                    self.st.get_mut(p.sym).flags = self.st.get(p.sym).flags.with(Flags::BYNAME);
                }
            }
            if !p.sym.is_none() {
                self.macro_mirror_owners.insert(p.sym, function);
                let previous = self.st.get(p.sym).owner;
                if previous != function {
                    if !previous.is_none() {
                        self.st.get_mut(previous).members.retain(|s| *s != p.sym);
                    }
                    self.st.get_mut(p.sym).owner = function;
                    if !self.st.get(function).members.contains(&p.sym) {
                        self.st.get_mut(function).members.push(p.sym);
                    }
                }
                self.st.get_mut(p.sym).ty = p.ty.clone();
                self.st.enter_in_current(p.name().unwrap_or("_"), p.sym);
            }
            param_tys.push(p.ty.clone());
        }
        self.type_expr(body, &ret_pt);
        // A body with nothing expected of it is still a value: `x => add` is
        // nsc's "missing argument list" (eta-expanded under `-Xsource:3`).
        // With an expectation, `adapt` below applies the same rule.
        if ret_pt.is_no_type() {
            self.adapt_method_value(body);
        }
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
            // A function-valued result is already a reference value. Preserve
            // its inferred type below the prototype's upper bound: replacing
            // () => A with the provisional () => Any loses A in Deferred.
            if is_function_pt(&body.ty)
                && is_function_pt(&ret_pt)
                && self.st.is_sub_type(&body.ty, &ret_pt)
            {
                body.ty.clone()
            } else {
                ret_pt
            }
        };
        let body_ty = body.ty.widen_constant();
        self.st.pop_scope();
        self.macro_function_symbols
            .insert((self.file_index, body.id), function);
        self.st.owner = saved_owner;
        self.macro_lexical_owner = saved_macro_owner;
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
        } else if sam.as_ref().is_some_and(|s| {
            s.param_tys.len() == param_tys.len()
                && s.param_tys
                    .iter()
                    .zip(&param_tys)
                    .all(|(want, have)| self.st.is_sub_type(want, have))
        }) {
            // The literal really does have the SAM's arity, so it *is* one.
            //
            // This used to read `param_tys.len() == pts.len()`, which cannot
            // fail: the arity guard forty lines up has already replaced `pts`
            // with `vec![NoType; vparams.len()]` whenever the two disagreed,
            // so a literal of the wrong arity claimed the SAM type anyway and
            // `val e: Equiv[Int] = (x: Int) => x > 0` compiled -- real scalac
            // 2.13.16 says `found: Int => Boolean  required:
            // scala.math.Equiv[Int]`. Comparing against the SAM's own arity
            // is the question that was meant. Pre-existing, and reachable on
            // the branch point through any source-declared SAM trait.
            pt.clone()
        } else {
            Type::Function {
                params: param_tys,
                ret: Box::new(ret),
            }
        }
    }

    pub(crate) fn f_arg_ok(&self, ty: &Type, kind: scala_rs_parser::finterp::FConvKind) -> bool {
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
}

/// Whether `m` is fine to use as an "apply" candidate at the receiver
/// `recv_cls` -- true for everything but a *static* member reached only by
/// inheriting from an ancestor.
///
/// A class file's `ACC_STATIC` method is never a real member in nsc's own
/// model (Scala has no static members): it is either a genuine Java static,
/// which nsc does not inherit into a subclass either ("static Java members
/// belong to companion objects in Scala; they are not inherited"), or a
/// mirror forwarder scalac writes for a same-named companion object's public
/// members, so Java code can write `C.member(...)` instead of
/// `C$.MODULE$.member(...)`. Selecting one through the *exact* class/module
/// it is declared on is what it is for -- `cats.effect.IO.apply(...)`, read
/// this way before `Checker::retry_module_apply_from_pickle` gets a chance to
/// correct the erased by-name signature the class file can only spell as
/// `Function0`. Reaching the same symbol only because a *subclass* inherited
/// it from that ancestor is not: `object datetimeago extends
/// BaseScalaTemplate[...](fmt)` (a case class) inherited its companion's
/// static `apply` alongside its own written `apply(date, recentOnly =
/// true)`, and every Twirl template with a defaulted trailing parameter had
/// the two competing -- "ambiguous overload for apply/datetimeago with
/// arguments (Date)", `docs/gitbucket.md` root 26.
pub(crate) fn not_inherited_static(
    st: &crate::symbol::SymbolTable,
    m: SymbolId,
    recv_cls: SymbolId,
) -> bool {
    !st.get(m).flags.contains(Flags::STATIC) || st.get(m).owner == recv_cls
}

/// The parameter type at `idx` of the only alternative in `alts` whose first
/// parameter list can take `nargs` arguments, when that type is fully
/// determined. An alternative with *more* parameters might be completed by
/// defaults, which its type does not record, so it counts as a match.
fn sole_arity_match_param(alts: &[Type], idx: usize, nargs: usize) -> Option<Type> {
    let mut found: Option<&[Type]> = None;
    for a in alts {
        let Type::Method { paramss, .. } = a else {
            return None;
        };
        let ps: &[Type] = paramss.first().map(|p| p.as_slice()).unwrap_or(&[]);
        let repeated = ps.last().is_some_and(|p| matches!(p, Type::Repeated(_)));
        let fits = ps.len() == nargs || (repeated && nargs + 1 >= ps.len()) || nargs < ps.len();
        if fits {
            if found.is_some() {
                return None;
            }
            found = Some(ps);
        }
    }
    let p = param_at(found?, idx)?;
    let p = match p {
        Type::ByName(inner) => inner.as_ref(),
        other => other,
    };
    if p.is_no_type()
        || p.is_error()
        || matches!(p, Type::Repeated(_))
        || mentions_any_tparam(p)
        || type_mentions_wildcard(p)
    {
        return None;
    }
    Some(p.clone())
}
