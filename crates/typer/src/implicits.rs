//! In-scope implicit vals/defs (including those inherited from parent
//! class/trait), companions of the parts of the target type (type constructor,
//! type arguments, nested prefixes, and their base classes), imported
//! implicits, and package objects of the enclosing package.
//! `no implicit` / `ambiguous implicit` / `diverging implicit expansion` are
//! hard errors.
//!
//! A candidate with type parameters of its own
//! (`implicit def showList[A](implicit s: Show[A]): Show[List[A]]`) is fitted
//! by unifying its result type with the wanted type ([`Unify`],
//! [`Typer::implicit_solve`]). The unifier is two-sided: it solves the
//! candidate's parameters *and* the call-site parameters only the witness can
//! pin down (nsc's undetermined tparams, as in
//! `toMap[K, V](implicit ev: A <:< (K, V))`), widening the candidate to a base
//! type when the wanted class is a supertype of the candidate's
//! (`<:<.refl[A]: A =:= A` fitted to `From <:< To`). A candidate that leaves a
//! type parameter undetermined is dropped, never filled with `Any`.
//!
//! Once fitted, a candidate's own implicit arguments are resolved recursively.
//! Two cut-offs bound that: [`MAX_IMPLICIT_DEPTH`], and nsc's diverging
//! implicit expansion — re-entering the same implicit for a target with the
//! same head symbol and no smaller complexity
//! (`implicit def loop[A](implicit a: A): A`).

use scala_rs_parser::{Flags, RefineDecl, SymbolId, Tree, TreeKind, Type};
use scala_rs_span::Span;

use crate::check::Typer;
use crate::symbol::SymKind;

/// The conversion a view search settled on, the target type with the callee's
/// undetermined parameters filled in, and those bindings.
pub(crate) type OpenView = (SymbolId, Type, Vec<(SymbolId, Type)>);

/// Hard cap on nested derivations, on top of the divergence check.
pub(crate) const MAX_IMPLICIT_DEPTH: usize = 8;

#[derive(Debug)]
pub enum ImplicitSearch {
    Found(SymbolId),
    None,
    Ambiguous(Vec<SymbolId>),
}

impl ImplicitSearch {
    pub(crate) fn is_found(&self) -> bool {
        matches!(self, ImplicitSearch::Found(_))
    }
}

/// How a candidate was fitted to the wanted type.
#[derive(Debug, Default, Clone)]
pub(crate) struct ImplicitFit {
    /// The candidate's own type arguments, in `tparams` order.
    pub(crate) targs: Vec<Type>,
    /// Call-site type parameters the search pinned down.
    pub(crate) undet: Vec<(SymbolId, Type)>,
}

/// Number of nodes in a type, nsc's `complexity`.
fn complexity(ty: &Type) -> usize {
    match ty {
        Type::Class { args, .. } | Type::Named { args, .. } => {
            1 + args.iter().map(complexity).sum::<usize>()
        }
        Type::Tuple(ts) => 1 + ts.iter().map(complexity).sum::<usize>(),
        Type::Applied { ctor, args } => {
            complexity(ctor) + args.iter().map(complexity).sum::<usize>()
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => 1 + complexity(t),
        Type::Function { params, ret } => {
            1 + params.iter().map(complexity).sum::<usize>() + complexity(ret)
        }
        _ => 1,
    }
}

fn head_sym(typer: &Typer, ty: &Type) -> Option<SymbolId> {
    match ty {
        Type::Class { sym, .. } => Some(*sym),
        Type::Applied { ctor, .. } => head_sym(typer, ctor),
        _ => typer.st.class_sym_of(ty),
    }
}

/// nsc `Types#dominates`: same head symbol and no simpler than the open one.
fn dominates(typer: &Typer, new_pt: &Type, open_pt: &Type) -> bool {
    match (head_sym(typer, new_pt), head_sym(typer, open_pt)) {
        (Some(a), Some(b)) => a == b && complexity(new_pt) >= complexity(open_pt),
        // A bare type parameter target (`implicit def loop[A](implicit a: A): A`)
        // has no head symbol; fall back to plain complexity.
        _ => complexity(new_pt) >= complexity(open_pt),
    }
}

impl Typer {
    pub(crate) fn implicits_in_scope(&self) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        // SLS 7.2: a candidate is an identifier "that can be accessed ...
        // without a prefix", i.e. ordinary unqualified name resolution, which
        // shadows: a name bound in a nearer scope hides every binding of that
        // same name in an enclosing one, implicit or not (`implicit def i2s`
        // in a method body hides an outer `implicit def i2s`, even though
        // both are visible to `implicits_in_scope`'s underlying scope walk).
        // `shadowed_names` tracks every name a nearer scope has already bound
        // so an outer scope's same-named symbol is never even considered --
        // matching, rather than merely deduplicating, what a bare reference
        // to that name would resolve to at each point in the walk.
        // Borrowed keys: this runs on every implicit search and copying every
        // name in every enclosing scope was its largest single cost.
        // Sized up front: every name of every enclosing scope goes in, and
        // growing the table from empty on each implicit search cost more than
        // the inserts themselves.
        let mut shadowed_names: rustc_hash::FxHashSet<&str> =
            rustc_hash::FxHashSet::with_capacity_and_hasher(
                self.st.scopes.iter().map(|sc| sc.len()).sum(),
                rustc_hash::FxBuildHasher,
            );
        for sc in self.st.scopes.iter().rev() {
            for (name, ids) in sc.entries() {
                if !shadowed_names.insert(name.as_str()) {
                    continue;
                }
                for id in ids {
                    if self.st.get(*id).flags.contains(Flags::IMPLICIT) && seen.insert(id.0) {
                        out.push(*id);
                    }
                }
            }
        }
        if !self.st.this_class.is_none() && !self.parent_ctor_scope {
            // Instance implicits on this class/module, walking parents (nsc
            // linearization is not reproduced; inheritance is).
            let mut work = vec![self.st.this_class];
            let mut walked = rustc_hash::FxHashSet::default();
            while let Some(id) = work.pop() {
                if id.is_none() || !walked.insert(id.0) {
                    continue;
                }
                for &m in &self.st.get(id).members {
                    // A local declaration shadows a same-named instance
                    // member too -- an unqualified reference to that name
                    // resolves to the local one, not `this.name`.
                    if self.st.get(m).flags.contains(Flags::IMPLICIT)
                        && !shadowed_names.contains(self.st.get(m).name.as_str())
                        && seen.insert(m.0)
                    {
                        out.push(m);
                    }
                }
                for p in &self.st.get(id).parents {
                    if let Some(ps) = self.st.class_sym_of(p) {
                        work.push(ps);
                    }
                }
            }
            // Package object of the enclosing package (members copied onto the
            // package symbol, plus the `package` module itself).
            let mut owner = self.st.get(self.st.this_class).owner;
            while !owner.is_none() {
                let o = self.st.get(owner);
                if o.kind == crate::symbol::SymKind::Package {
                    for &m in &o.members {
                        if self.st.get(m).flags.contains(Flags::IMPLICIT)
                            && !shadowed_names.contains(self.st.get(m).name.as_str())
                            && seen.insert(m.0)
                        {
                            out.push(m);
                        }
                        if self.st.get(m).name == "package" {
                            let mcls = self.st.module_class_of(m);
                            for &mem in &self.st.get(mcls).members {
                                if self.st.get(mem).flags.contains(Flags::IMPLICIT)
                                    && !shadowed_names.contains(self.st.get(mem).name.as_str())
                                    && seen.insert(mem.0)
                                {
                                    out.push(mem);
                                }
                            }
                        }
                    }
                    break;
                }
                owner = o.owner;
            }
        }
        self.shadow_inherited_implicits(out)
    }

    /// nsc's `findMember`, applied to the implicit members `this` inherits.
    ///
    /// Only inherited ones: a candidate whose owner is not in `this`'s
    /// linearization is never dropped, so an import, an enclosing scope and a
    /// package object are all left exactly as the walk found them.
    ///
    /// Two members of the same name whose types are the same are **one**
    /// member, not two candidates: an unqualified reference to that name means
    /// whichever of them the linearization reaches first, and the other is not
    /// in scope at all. The walk above collects every base class's members
    /// separately, so an inherited *declaration* was offered next to the
    /// definition that implements it, and both fitted every search equally.
    ///
    /// scalatra is what showed it. `ScalatraContext` declares
    ///
    /// ```scala
    /// implicit def request: HttpServletRequest
    /// ```
    ///
    /// and `DynamicScope` defines it; neither trait is a base of the other, so
    /// the "an override replaces what it overrides" rule in
    /// `Check::drop_overridden` -- which asks for one owner to be below the
    /// other -- has nothing to compare. `ScalatraFilter` mixes in both, and
    /// every `params("id")` in a gitbucket controller was `ambiguous implicit:
    /// request, request`.
    ///
    /// Keeping the one the linearization reaches first is also the only choice
    /// that can be *called*: a declaration has no body, and the class that
    /// implements it is the one the JVM resolves to.
    fn shadow_inherited_implicits(&self, cands: Vec<SymbolId>) -> Vec<SymbolId> {
        if cands.len() < 2 {
            return cands;
        }
        let lin = crate::lin::linearize(&self.st, self.st.this_class);
        let rank = |owner: SymbolId| lin.iter().position(|&b| b == owner);
        cands
            .iter()
            .copied()
            .filter(|&c| {
                let s = self.st.get(c);
                let Some(here) = rank(s.owner) else {
                    return true;
                };
                !cands.iter().any(|&other| {
                    if other == c {
                        return false;
                    }
                    let o = self.st.get(other);
                    if o.name != s.name || o.owner == s.owner || o.ty != s.ty {
                        return false;
                    }
                    rank(o.owner).is_some_and(|there| there < here)
                })
            })
            .collect()
    }

    /// Implicit members of the companion module of `class_id` (or the module
    /// class itself when `class_id` is already a module / module class).
    fn companion_implicits_of_class(&self, class_id: SymbolId) -> Vec<SymbolId> {
        let mut out = Vec::new();
        if class_id.is_none() {
            return out;
        }
        let (module_sym, mcls) = match self.st.get(class_id).kind {
            SymKind::Module => (class_id, self.st.module_class_of(class_id)),
            // Reached as a module *class*; there is no back-pointer to the
            // module symbol, so nothing is recorded for it.
            SymKind::ModuleClass => (SymbolId::NONE, class_id),
            _ => {
                let Some(module) = self.st.companion_module(class_id) else {
                    return out;
                };
                (module, self.st.module_class_of(module))
            }
        };
        // SLS 7.2 names the companion *object*, and an object's members
        // include the ones it inherits. slick declares every `Shape` instance
        // in `trait RepShapeImplicits` / `ConstColumnShapeImplicits` /
        // `TupleShapeImplicits` and writes
        // `object Shape extends ConstColumnShapeImplicits with …`, so stopping
        // at the module class's own members found none of them at all.
        let mut work = vec![mcls];
        let mut walked = rustc_hash::FxHashSet::default();
        let mut seen = rustc_hash::FxHashSet::default();
        while let Some(id) = work.pop() {
            if id.is_none() || !walked.insert(id.0) {
                continue;
            }
            for &mem in &self.st.get(id).members {
                if self.st.get(mem).flags.contains(Flags::IMPLICIT) && seen.insert(mem.0) {
                    if id != mcls && !module_sym.is_none() {
                        // Declared by a trait the object mixes in; the
                        // reference has to name the object (see
                        // `wildcard_module_for`).
                        self.implicit_via_module
                            .borrow_mut()
                            .insert(mem.0, module_sym);
                    }
                    out.push(mem);
                }
            }
            for p in &self.st.get(id).parents {
                if let Some(ps) = self.st.class_sym_of(p) {
                    work.push(ps);
                }
            }
        }
        out
    }

    /// nsc-style parts of a type: the type constructor, type arguments, and
    /// enclosing class/module prefixes of nested types.
    fn collect_type_parts(
        &self,
        ty: &Type,
        out: &mut Vec<SymbolId>,
        seen: &mut rustc_hash::FxHashSet<u32>,
    ) {
        match ty {
            Type::Class { sym, args } => {
                self.collect_class_and_enclosing(*sym, out, seen);
                for a in args {
                    self.collect_type_parts(a, out, seen);
                }
            }
            Type::Applied { ctor, args } => {
                self.collect_type_parts(ctor, out, seen);
                for a in args {
                    self.collect_type_parts(a, out, seen);
                }
            }
            Type::Named { args, .. } => {
                if let Some(id) = self.st.class_sym_of(ty) {
                    self.collect_class_and_enclosing(id, out, seen);
                }
                for a in args {
                    self.collect_type_parts(a, out, seen);
                }
            }
            Type::ModuleRef(s) => self.collect_class_and_enclosing(*s, out, seen),
            // An existential's bound is part of the type, the way nsc's
            // `companionImplicitMap` follows an abstract type's `bounds.hi`.
            // Without it `Shape[_ <: FlatShapeLevel, Rep[String], T, G]`
            // (slick's `Query.map`) named no class the typer could warm, so
            // `FlatShapeLevel` still had an empty parent list when
            // `candidate_bounds_hold` asked whether it is a `ShapeLevel`, and
            // `repColumnShape` was dropped for a bound it does satisfy.
            Type::BoundedWildcard { lo, hi } => {
                for b in [lo, hi].into_iter().flatten() {
                    self.collect_type_parts(b, out, seen);
                }
            }
            Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => {
                self.collect_type_parts(t, out, seen);
            }
            Type::Function { params, ret } => {
                for p in params {
                    self.collect_type_parts(p, out, seen);
                }
                self.collect_type_parts(ret, out, seen);
            }
            Type::Tuple(ts) => {
                for t in ts {
                    self.collect_type_parts(t, out, seen);
                }
            }
            Type::Method { paramss, ret } => {
                for c in paramss {
                    for p in c {
                        self.collect_type_parts(p, out, seen);
                    }
                }
                self.collect_type_parts(ret, out, seen);
            }
            // SLS 7.2's parts of a compound type are the parts of every
            // parent, not just the first (the existing fallback below finds
            // only the first parent's class, through `class_sym_of`'s own
            // `Type::Refined` arm). Purely additive: subtyping, display and
            // dealiasing read a *view* refinement (`as_seen_from_view`, used
            // for a `Type::Class` projection prefix) as its bare first
            // parent and never reach here, so this only ever adds
            // implicit-scope candidates to what they already saw.
            Type::Refined { parents, .. } => {
                for p in parents {
                    self.collect_type_parts(p, out, seen);
                }
            }
            // A still-abstract type member offers only its upper bound's
            // class as a part (see `SymbolTable::class_sym_of`), which is
            // not where cats' `Newtype` encoding declares its conversions --
            // `object NonEmptySetImpl extends Newtype` never overrides
            // `Newtype`'s abstract `type Type[A]`, so `Type`'s only
            // class-side answer is `Base`'s, and `catsNonEmptySetOps` lives
            // on `NonEmptySetImpl` itself. `Type::TypeMember` has no room to
            // carry the prefix a qualified `p.T` selected it through --
            // `Checker::with_prefix_if_type_member` records it in
            // `Typer::type_member_prefixes` instead, keyed by `T`'s own
            // symbol, and this is the one place that reads it back. See
            // `docs/cats.md`'s `Newtype` note.
            Type::TypeMember(id) => {
                if let Some(id) = self.st.class_sym_of(ty) {
                    self.collect_class_and_enclosing(id, out, seen);
                }
                if let Some(owners) = self.type_member_prefixes.borrow().get(&id.0) {
                    for &owner in owners {
                        self.collect_class_and_enclosing(owner, out, seen);
                    }
                }
            }
            _ => {
                if let Some(id) = self.st.class_sym_of(ty) {
                    self.collect_class_and_enclosing(id, out, seen);
                }
            }
        }
    }

    fn collect_class_and_enclosing(
        &self,
        id: SymbolId,
        out: &mut Vec<SymbolId>,
        seen: &mut rustc_hash::FxHashSet<u32>,
    ) {
        if id.is_none() || !seen.insert(id.0) {
            return;
        }
        out.push(id);
        // SLS 7.2: the implicit scope of `T` also holds the companions of `T`'s
        // base classes. `=:=` has no companion object of its own, so its only
        // witness (`<:<.refl`) is reachable only through the `<:<` it extends.
        for p in &self.st.get(id).parents {
            if let Some(ps) = self.st.class_sym_of(p) {
                self.collect_class_and_enclosing(ps, out, seen);
            }
        }
        let owner = self.st.get(id).owner;
        if owner.is_none() {
            return;
        }
        match self.st.get(owner).kind {
            SymKind::Class | SymKind::ModuleClass | SymKind::Module => {
                self.collect_class_and_enclosing(owner, out, seen);
            }
            _ => {}
        }
    }

    /// The classes whose companions form `ty`'s implicit scope (SLS 7.2):
    /// the type constructor, its arguments, their base classes, and the
    /// enclosing prefixes. Exposed so the typer can make sure each one's
    /// companion object is actually *loaded* before a search runs — the search
    /// itself holds an immutable borrow and cannot read a class file.
    pub(crate) fn implicit_scope_classes(&self, ty: &Type) -> Vec<SymbolId> {
        let mut parts = Vec::new();
        self.collect_type_parts(ty, &mut parts, &mut rustc_hash::FxHashSet::default());
        parts
    }

    fn companion_implicits(&self, ty: &Type) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut parts = Vec::new();
        self.collect_type_parts(ty, &mut parts, &mut rustc_hash::FxHashSet::default());
        for cls in parts {
            for mem in self.companion_implicits_of_class(cls) {
                if seen.insert(mem.0) {
                    out.push(mem);
                }
            }
        }
        out
    }

    /// Every parameter clause is implicit, so the candidate is usable as long
    /// as its own implicits resolve (`implicit def listShow[A](implicit s:
    /// Show[A]): Show[List[A]]`).
    fn only_implicit_clauses(&self, id: SymbolId) -> bool {
        let s = self.st.get(id);
        s.paramss.iter().all(|c| {
            c.iter()
                .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
        }) && (!s.paramss.is_empty() || s.params.is_empty())
    }

    /// A type read through the `import <a value>._` prefix that made `id`
    /// visible, when there is one.
    ///
    /// The value's class is where `id` is declared, and only the value says
    /// what that class's type parameters are: `import b._` with `b: Box[Int]`
    /// reads `class Box[T] { implicit def mkOps(lhs: T): Ops[T] }` as
    /// `Int => Ops[Int]`. Left as `Box`'s own `T` the candidate matched
    /// nothing, which is what made `import seq.integral._; increment < zero`
    /// report `value < is not a member of T`.
    ///
    /// It is also what reads a member of a class *nested* in that one:
    /// `Ordering[T]#OrderingOps` declares `def <(rhs: T)` at `Ordering`'s
    /// parameter, and `subst_as_seen_from` replaces that symbol wherever it
    /// occurs, however deeply the member is nested.
    pub(crate) fn at_import_prefix_of(&self, id: SymbolId, ty: &Type) -> Option<Type> {
        let owner = self.st.get(id).owner;
        if owner.is_none()
            || !self.st.get(owner).is_class_like()
            || self.st.get(owner).tparams.is_empty()
        {
            return None;
        }
        let prefix = self.term_import_prefix_for(owner).map(|q| q.ty.clone())?;
        if prefix.is_no_type() || prefix.is_error() {
            return None;
        }
        Some(self.st.subst_as_seen_from(&prefix, ty))
    }

    /// An inherited implicit is declared in terms of its *owner's* type
    /// parameters. Seen from the class we are typing, those are the parent's
    /// arguments: `implicit def p1Type: TT[P1]` on `trait Base[P1]` is
    /// `TT[P1]` of `Mid` inside `trait Mid[P1] extends Base[P1]`. Without this
    /// the candidate carries `Base`'s `P1`, which never matches the wanted
    /// `TT[P1]` of `Mid`.
    pub(crate) fn implicit_candidate_ty(&self, id: SymbolId) -> std::borrow::Cow<'_, Type> {
        // Borrowed in the common case. This runs once per candidate per
        // implicit search, and deep-cloning the declared type of every
        // candidate was the single largest source of `Type::clone` in a slick
        // build; almost all of those clones were then only read.
        let ty = &self.st.get(id).ty;
        let this = self.st.this_class;
        let owner = self.st.get(id).owner;
        // An implicit brought in by `import <a value>._` is declared in terms
        // of that value's class parameters, and the value is what says what
        // they are: `import b._` with `b: Box[Int]` reads
        // `class Box[T] { implicit def mkOps(lhs: T): Ops[T] }` as
        // `Int => Ops[Int]`. Left as `Box`'s own `T` the candidate matched
        // nothing, which is why `import seq.integral._; increment < zero`
        // (`Numeric[T]#mkOrderingOps`, reached through `Integral[T]`) reported
        // `value < is not a member of T`.
        if let Some(seen) = self.at_import_prefix_of(id, ty) {
            return std::borrow::Cow::Owned(seen);
        }
        if this.is_none()
            || owner.is_none()
            || owner == this
            || !self.st.get(owner).is_class_like()
            || self.st.get(owner).tparams.is_empty()
        {
            return std::borrow::Cow::Borrowed(ty);
        }
        let this_ty = Type::Class {
            sym: this,
            args: self
                .st
                .get(this)
                .tparams
                .iter()
                .map(|t| Type::TypeParam(*t))
                .collect(),
        };
        std::borrow::Cow::Owned(self.st.subst_as_seen_from(&this_ty, ty))
    }

    /// Whether `id` can inhabit `pt`, and with which type arguments.
    ///
    /// `undet` are call-site type parameters the search itself has to solve —
    /// nsc infers `K`/`V` of `toMap[K, V](implicit ev: A <:< (K, V))` from the
    /// witness it finds, since they appear nowhere else in the call.
    pub(crate) fn implicit_fit_at(
        &self,
        id: SymbolId,
        pt: &Type,
        depth: usize,
        undet: &[SymbolId],
    ) -> Option<ImplicitFit> {
        if !self.st.get(id).flags.contains(Flags::IMPLICIT) {
            return None;
        }
        let cand_ty = self.implicit_candidate_ty(id);
        match &*cand_ty {
            Type::Method { paramss, ret } => {
                if paramss.iter().all(|c| c.is_empty()) {
                    return self.implicit_solve(id, ret, pt, undet);
                }
                // A derivation rule: usable when its own implicits resolve.
                if depth >= MAX_IMPLICIT_DEPTH || !self.only_implicit_clauses(id) {
                    return None;
                }
                let Some(fit) = self.implicit_solve(id, ret, pt, undet) else {
                    return self.implicit_fit_open(id, ret, pt, undet, paramss, depth);
                };
                if self.implicit_diverges(id, pt) {
                    return None;
                }
                let tps = self.st.get(id).tparams.clone();
                self.open_implicits
                    .borrow_mut()
                    .push((id, self.subst_undet(pt, &fit.undet)));
                let ok = paramss.iter().flatten().all(|p| {
                    let want = crate::symbol::subst_tparams_slice(&tps, &fit.targs, p);
                    self.search_implicit_at(&want, depth + 1).is_found()
                        || self.built_not_found(&want)
                });
                self.open_implicits.borrow_mut().pop();
                ok.then_some(fit)
            }
            Type::Function { params, ret } if params.is_empty() => {
                self.implicit_solve(id, ret, pt, undet)
            }
            t => self.implicit_solve(id, t, pt, undet),
        }
    }

    /// An implicit that is *built* rather than found, so `search_implicit_at`
    /// answering `None` says nothing about whether the parameter can be filled.
    ///
    /// `fill_implicit_params` has always had these fallbacks; the viability
    /// check for a derivation rule did not, so a rule with a `ClassTag`
    /// parameter of its own was judged unusable and never even tried.
    /// `implicit def forColl[C[X] <: Iterable[X]](implicit cbf: Factory[Any,
    /// C[Any]], tag: ClassTag[C[Any]]): TypedCollectionTypeConstructor[C]`
    /// (slick's `ast/Type.scala`) is exactly that shape, and `q.to[Seq]` was
    /// reported as a missing `TypedCollectionTypeConstructor[Seq]` while
    /// `implicitly[ClassTag[Seq[Any]]]` on its own compiled fine.
    ///
    /// Deliberately only the tags: the view fallbacks (`identity_view`,
    /// `array_wrap_view`, `conversion_view`) run their own searches and would
    /// make a function-typed parameter look satisfiable without saying which
    /// conversion answers it.
    fn built_not_found(&self, want: &Type) -> bool {
        if crate::materialize::tag_request(&self.st, want).is_some() {
            return true;
        }
        matches!(want, Type::Class { sym, args }
            if !args.is_empty()
                && self.st.get(*sym).name == "ClassTag"
                && self.st.companion_module(*sym).is_some())
    }

    /// nsc's "diverging implicit expansion": the same implicit is already being
    /// expanded for a target with the same head symbol and no smaller
    /// complexity (`implicit def loop[A](implicit a: A): A`).
    fn implicit_diverges(&self, id: SymbolId, pt: &Type) -> bool {
        let open = self.open_implicits.borrow();
        let hit = open
            .iter()
            .any(|(sid, spt)| *sid == id && dominates(self, pt, spt));
        drop(open);
        if hit && self.diverged_implicit.borrow().is_none() {
            *self.diverged_implicit.borrow_mut() = Some((id, pt.clone()));
        }
        hit
    }

    fn subst_undet(&self, ty: &Type, undet: &[(SymbolId, Type)]) -> Type {
        if undet.is_empty() {
            return ty.clone();
        }
        let ids: Vec<SymbolId> = undet.iter().map(|(id, _)| *id).collect();
        let tys: Vec<Type> = undet.iter().map(|(_, t)| t.clone()).collect();
        crate::symbol::subst_tparams_slice(&ids, &tys, ty)
    }

    /// Solve a candidate's type parameters from its result against the wanted
    /// type: `Show[List[A]]` against `Show[List[Int]]` gives `A = Int`.
    pub(crate) fn implicit_targs(&self, id: SymbolId, ret: &Type, pt: &Type) -> Option<Vec<Type>> {
        let tps = self.st.get(id).tparams.clone();
        let mut args = Vec::with_capacity(tps.len());
        for tp in &tps {
            args.push(crate::check::unify_one(*tp, ret, pt)?);
        }
        Some(args)
    }

    /// Unify the candidate's result type with the wanted type, solving both the
    /// candidate's own type parameters and `undet`, then check the instantiated
    /// result really conforms. A candidate with a type parameter left
    /// undetermined is dropped (never silently filled with `Any`).
    fn implicit_solve(
        &self,
        id: SymbolId,
        ret: &Type,
        pt: &Type,
        undet: &[SymbolId],
    ) -> Option<ImplicitFit> {
        let tps = self.st.get(id).tparams.clone();
        if tps.is_empty() && undet.is_empty() {
            return self
                .implicit_result_conforms(ret, pt)
                .then(ImplicitFit::default);
        }
        let mut u = Unify::new(self, tps.iter().copied(), undet.iter().copied());
        if !u.unify(ret, pt) {
            // Fall back to the one-sided guess. It cannot solve `undet`, so a
            // call whose type parameters only the witness can pin down fails
            // here rather than guessing.
            if !undet.is_empty() {
                return None;
            }
            let targs = self.implicit_targs(id, ret, pt)?;
            let inst = crate::symbol::subst_tparams_slice(&tps, &targs, ret);
            return self
                .implicit_result_conforms(&inst, pt)
                .then(|| ImplicitFit {
                    targs,
                    undet: Vec::new(),
                });
        }
        let mut targs = Vec::with_capacity(tps.len());
        for tp in &tps {
            match u.solved(*tp) {
                Some(t) => targs.push(self.simplify_solved(&t)),
                // Not pinned down by the result type; the one-sided guess is
                // the last chance before the candidate is dropped.
                None => targs.push(crate::check::unify_one(*tp, ret, pt)?),
            }
        }
        // A solution read off a higher-kinded position is an `Applied` whose
        // constructor is now a class (`CC[A]` with `CC := List`); nothing
        // downstream prints or erases that as `List[String]`.
        let undet_out: Vec<(SymbolId, Type)> = undet
            .iter()
            .filter_map(|d| u.solved(*d).map(|t| (*d, self.simplify_solved(&t))))
            .collect();
        if !self.candidate_bounds_hold(&tps, &targs) {
            return None;
        }
        let inst = self.simplify_solved(&crate::symbol::subst_tparams_slice(&tps, &targs, ret));
        let want = self.subst_undet(pt, &undet_out);
        self.implicit_result_conforms(&inst, &want)
            .then_some(ImplicitFit {
                targs,
                undet: undet_out,
            })
    }

    /// [`Self::implicit_solve`] for a derivation rule whose own type parameters
    /// the wanted type cannot pin down, because the *call site* left a type
    /// parameter undetermined and that is where they show through.
    ///
    /// slick's `Compiled.apply[V, C <: Compiled[V]](raw: V)(implicit c:
    /// Compilable[V, C], …): C` is the case. `V` comes from the argument, but
    /// `C` is undetermined -- it occurs only in the implicit clause and in the
    /// result -- so the search is for `Compilable[Rep[P] => Query[T, U, Seq],
    /// ?C]`. Unifying that with
    ///
    /// ```text
    /// function1IsCompilable[A, B <: Rep[_], P, U]: Compilable[A => B, CompiledFunction[A => B, A, P, B, U]]
    /// ```
    ///
    /// settles `A` and `B` and binds `?C` to `CompiledFunction[A => B, A, P, B,
    /// U]` -- with the candidate's own `P` and `U` still open, because nothing
    /// on the wanted side stands opposite them. Only the candidate's *own*
    /// implicit parameters can say what they are: `aShape: Shape[…, A, P, A]`
    /// gives `P`, `bExe: Executable[B, U]` gives `U`. nsc solves them exactly
    /// there (`Context.undetparams` while the implicit arguments are typed);
    /// [`Self::implicit_solve`] insists on a complete solution from the result
    /// type alone, drops the candidate, and the call was
    /// "Computation of type (Rep[P]) => Query[T, U, Seq] cannot be compiled
    /// (as type C)" -- slick's own `@implicitNotFound`.
    ///
    /// Deliberately a *fallback*: it runs only for a candidate the ordinary
    /// solve rejected, and only when the wanted type pinned down at least one
    /// of the candidate's parameters. A rule that matched with everything open
    /// would be tried against every implicit in scope.
    fn implicit_fit_open(
        &self,
        id: SymbolId,
        ret: &Type,
        pt: &Type,
        undet: &[SymbolId],
        paramss: &[Vec<Type>],
        depth: usize,
    ) -> Option<ImplicitFit> {
        if undet.is_empty() {
            return None;
        }
        let tps = self.st.get(id).tparams.clone();
        if tps.is_empty() {
            return None;
        }
        let mut u = Unify::new(self, tps.iter().copied(), undet.iter().copied());
        if !u.unify(ret, pt) {
            return None;
        }
        let mut targs: Vec<Type> = Vec::with_capacity(tps.len());
        let mut open: Vec<SymbolId> = Vec::new();
        for tp in &tps {
            match u.solved(*tp) {
                Some(t) => targs.push(self.simplify_solved(&t)),
                None => {
                    open.push(*tp);
                    targs.push(Type::TypeParam(*tp));
                }
            }
        }
        // Nothing left open: the ordinary solve already had its say, and
        // failed on conformance or bounds. Everything left open: the wanted
        // type says nothing about this candidate at all.
        if open.is_empty() || open.len() == tps.len() {
            return None;
        }
        if self.implicit_diverges(id, pt) {
            return None;
        }
        self.open_implicits.borrow_mut().push((id, pt.clone()));
        let mut ok = true;
        for p in paramss.iter().flatten() {
            let want = crate::symbol::subst_tparams_slice(&tps, &targs, p);
            if open.is_empty() {
                if !self.search_implicit_at(&want, depth + 1).is_found() {
                    ok = false;
                    break;
                }
                continue;
            }
            let (found, binds) = self.search_implicit_undet(&want, &open, depth + 1);
            if !found.is_found() {
                ok = false;
                break;
            }
            for (bid, bt) in binds {
                if let Some(pos) = tps.iter().position(|x| *x == bid) {
                    targs[pos] = self.simplify_solved(&bt);
                    open.retain(|x| *x != bid);
                }
            }
        }
        self.open_implicits.borrow_mut().pop();
        if !ok || !open.is_empty() {
            return None;
        }
        if !self.candidate_bounds_hold(&tps, &targs) {
            return None;
        }
        // The call site's own parameters were bound to types that still
        // mentioned the candidate's -- `?C := CompiledFunction[A => B, A, P, B,
        // U]`. Now that those are known, they are ordinary types.
        let undet_out: Vec<(SymbolId, Type)> = undet
            .iter()
            .filter_map(|d| {
                let t = u.solved_open(*d)?;
                let t = crate::symbol::subst_tparams_slice(&tps, &targs, &t);
                (!tps
                    .iter()
                    .any(|tp| crate::check::type_mentions_tparam(&t, *tp)))
                .then(|| (*d, self.simplify_solved(&t)))
            })
            .collect();
        let inst = self.simplify_solved(&crate::symbol::subst_tparams_slice(&tps, &targs, ret));
        let want = self.subst_undet(pt, &undet_out);
        self.implicit_result_conforms(&inst, &want)
            .then_some(ImplicitFit {
                targs,
                undet: undet_out,
            })
    }

    /// nsc checks a candidate's own type parameter bounds before deciding it
    /// applies (`Infer#checkBounds`), and among the `BuildFrom` witnesses that
    /// check is sometimes the *only* thing telling two of them apart.
    /// `object BuildFrom` declares
    ///
    /// ```text
    /// implicit def buildFromBitSet[C <: BitSet with BitSetOps[C]]: BuildFrom[C, Int, C]
    /// ```
    ///
    /// whose result type is a bare `C` on both sides. Left unchecked it
    /// answered `BuildFrom[List[Int], Int, ?]` -- it is declared in the
    /// companion itself, so it beats `buildFromIterableOps` on origin -- and
    /// `List(1, 2).lazyZip(…).map(_ + _)` type-checked and then died with
    /// `class ::$ cannot be cast to class scala.collection.BitSet`.
    ///
    /// Only a *first-order* parameter is checked here. A higher-kinded one
    /// arrives with its bound already folded into the type
    /// (`buildFromSortedSetOps` is
    /// `BuildFrom[CC[A0] with SortedSet[A0], A, CC[A] with SortedSet[A]]`),
    /// where the unifier enforces it directly; re-deriving that from
    /// `CC`'s own F-bounded, higher-kinded `bound_hi` would only risk
    /// dropping a candidate nsc accepts.
    ///
    /// The test is deliberately the permissive one -- conformance, or failing
    /// that merely having the bound's class among the solution's base classes.
    /// Rejecting less than nsc leaves an existing diagnostic in place;
    /// rejecting more turns working code into an error.
    fn candidate_bounds_hold(&self, tps: &[SymbolId], targs: &[Type]) -> bool {
        if tps.len() != targs.len() {
            return true;
        }
        for (tp, targ) in tps.iter().zip(targs) {
            if !self.st.get(*tp).tparams.is_empty() {
                continue;
            }
            let Some(hi) = self.st.get(*tp).bound_hi.clone() else {
                continue;
            };
            if targ.is_no_type() || targ.is_error() || matches!(targ, Type::TypeParam(_)) {
                continue;
            }
            let hi = crate::symbol::subst_tparams_slice(tps, targs, &hi);
            let parents: Vec<Type> = match &hi {
                Type::Refined { parents, .. } => parents.clone(),
                other => vec![other.clone()],
            };
            for parent in &parents {
                if self.st.is_sub_type(targ, parent) {
                    continue;
                }
                let Some(psym) = self.st.class_sym_of(parent) else {
                    continue;
                };
                if self.st.class_sym_of(targ) == Some(psym) {
                    continue;
                }
                if self.base_type_instance(targ, psym, 0).is_none() {
                    return false;
                }
            }
        }
        true
    }

    /// A solved type, tidied.
    ///
    /// Two things need it, both introduced by matching at a higher kind. An
    /// `Applied` whose constructor is now a class is collapsed (`CC[A]` with
    /// `CC := List` is `List[String]`; nothing downstream prints or erases the
    /// open form). And an intersection whose parents form a subtype chain
    /// becomes its most specific member: the F-bound reaches the typer folded
    /// into the type, so `buildFromSortedSetOps` answers a `TreeSet` receiver
    /// with `CC[A] with SortedSet[A]` = `TreeSet[Int] with SortedSet[Int]`,
    /// where nsc infers plain `TreeSet[Int]`.
    fn simplify_solved(&self, ty: &Type) -> Type {
        self.collapse_refinements(&fold_applied(ty))
    }

    fn collapse_refinements(&self, ty: &Type) -> Type {
        match ty {
            Type::Refined { parents, decls } if decls.is_empty() && !parents.is_empty() => {
                let parents: Vec<Type> = parents
                    .iter()
                    .map(|p| self.collapse_refinements(p))
                    .collect();
                let mut keep: Vec<Type> = Vec::new();
                for p in &parents {
                    // Dropped when another parent is strictly below it, and
                    // when an equal one is already kept.
                    let redundant = parents
                        .iter()
                        .any(|q| q != p && self.st.is_sub_type(q, p) && !self.st.is_sub_type(p, q));
                    if redundant || keep.contains(p) {
                        continue;
                    }
                    keep.push(p.clone());
                }
                match keep.len() {
                    1 => keep.remove(0),
                    _ => Type::Refined {
                        parents: keep,
                        decls: decls.clone(),
                    },
                }
            }
            Type::Class { sym, args } => Type::Class {
                sym: *sym,
                args: args.iter().map(|a| self.collapse_refinements(a)).collect(),
            },
            Type::Tuple(ts) => {
                Type::Tuple(ts.iter().map(|t| self.collapse_refinements(t)).collect())
            }
            other => other.clone(),
        }
    }

    /// ClassTag is invariant. Covariant `is_sub_type` would let
    /// `ClassTag[Nothing]` inhabit `ClassTag[Int]` (`Nothing <: Int`) and
    /// `newArray` would then allocate `Object[]`.
    /// `Releasable[-R]` is contravariant: `Releasable[AutoCloseable]` inhabits
    /// `Releasable[Box]` when `Box <: AutoCloseable` (nsc `Using.resource`).
    fn implicit_result_conforms(&self, have: &Type, pt: &Type) -> bool {
        match (have, pt) {
            (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, args: a2 })
                if s1 == s2 && !a1.is_empty() && !a2.is_empty() && a1.len() == a2.len() =>
            {
                let tparams = self.st.get(*s1).tparams.clone();
                a1.iter().zip(a2.iter()).enumerate().all(|(i, (x, y))| {
                    if x == y {
                        return true;
                    }
                    // An invariant position the wanted type left as `_` is not
                    // a constraint at all, in either direction.
                    if matches!(x, Type::Wildcard | Type::BoundedWildcard { .. })
                        || matches!(y, Type::Wildcard | Type::BoundedWildcard { .. })
                    {
                        return self.st.is_sub_type(x, y) || self.st.is_sub_type(y, x);
                    }
                    let flags = tparams
                        .get(i)
                        .map(|&tp| self.st.get(tp).flags)
                        .unwrap_or(Flags::EMPTY);
                    if flags.contains(Flags::CONTRAVARIANT) {
                        self.st.is_sub_type(y, x)
                    } else if flags.contains(Flags::COVARIANT) {
                        self.st.is_sub_type(x, y)
                    } else {
                        self.st.is_sub_type(x, y) && self.st.is_sub_type(y, x)
                    }
                })
            }
            _ => self.st.is_sub_type(have, pt),
        }
    }

    /// nsc `weak_<:<`: a view's argument only has to *weakly* conform, so the
    /// numeric widenings the JVM performs count. `Predef.long2Long` therefore
    /// applies to an `Int` (`xs.add(7)` on a `java.util.ArrayList[Long]`) and
    /// `int2Integer` to a `Char` (`val i: java.lang.Integer = 'c'`) — both
    /// compile in scalac. Only the numeric primitives take part; `Boolean` and
    /// `Unit` have no weak conformances.
    fn weak_conforms(&self, from: &Type, to: &Type) -> bool {
        if self.st.is_sub_type(from, to) {
            return true;
        }
        matches!(
            (from.widen_constant(), to.widen_constant()),
            (
                Type::Byte,
                Type::Short | Type::Int | Type::Long | Type::Float | Type::Double
            ) | (
                Type::Short,
                Type::Int | Type::Long | Type::Float | Type::Double
            ) | (
                Type::Char,
                Type::Int | Type::Long | Type::Float | Type::Double
            ) | (Type::Int, Type::Long | Type::Float | Type::Double)
                | (Type::Long, Type::Float | Type::Double)
                | (Type::Float, Type::Double)
        )
    }

    /// Whether `id` is an implicit conversion that turns a `from` into a `to`.
    ///
    /// A *polymorphic* candidate is solved from the argument type first, the
    /// way the member-directed search already does it
    /// ([`Self::conversion_result`]): `orderingToOrdered[T](x: T)(implicit
    /// ord: Ordering[T]): Ordered[T]` binds `T = String` from `from` and only
    /// then compares `Ordered[String]` with the wanted `Ordered[String]`.
    /// Comparing the *declared* types instead, as this used to, meant every
    /// conversion with a type parameter of its own was invisible to
    /// `search_conversion` — `implicit def boxit[T](x: T): Box[T]` did not make
    /// `val b: Box[Int] = 3` compile, and neither did `orderingToOrdered`
    /// satisfy an `A => Ordered[A]` view.
    ///
    /// A conversion whose own implicit clauses have no witness is not
    /// applicable, exactly as in nsc — otherwise `orderingToOrdered` would
    /// claim `Box[Int] => Ordered[Box[Int]]` and fail later, at the point where
    /// its `Ordering[Box[Int]]` argument has to be produced.
    /// SLS 7.3: a view is an implicit method with *one explicit* parameter.
    /// `implicit def Option[T](implicit ord: Ordering[T]): Ordering[Option[T]]`
    /// is a derivation rule, not a conversion from `Ordering[T]`; reading it as
    /// one accepted `val o: Ordering[Option[Int]] = Ordering.Int` (real scalac
    /// rejects it) and re-typed the receiver of every failed selection on an
    /// `Ordering`. The method *type* cannot say which clause is implicit, so
    /// the parameter symbols are what we ask.
    fn first_clause_is_implicit(&self, id: SymbolId) -> bool {
        let s = self.st.get(id);
        let first = match s.paramss.first() {
            Some(c) => c,
            None => return false,
        };
        !first.is_empty()
            && first
                .iter()
                .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
    }

    /// What a candidate looks like *as a view*: its one parameter and its
    /// result.
    ///
    /// nsc asks whether the candidate's type conforms to `From => To`, so a
    /// one-parameter method, a `Function1`-typed value, and a value of any
    /// class that *inherits* `Function1` all qualify. The last of those is
    /// how `implicit ev: P <:< Rp[Option[QO]]` converts a `P`
    /// (`sealed abstract class <:<[-From, +To] extends (From => To)`), which
    /// is what slick's `flatten[QO](implicit ev: P <:< Rp[Option[QO]]) =
    /// flatMap[QO](identity(_))` leans on entirely.
    ///
    /// A parameterless implicit method is *not* a view: `<:<.refl[A]: A =:= A`
    /// would otherwise convert every type to itself.
    fn view_shape(&self, ty: &Type) -> Option<(Type, Type)> {
        match ty {
            Type::Method { paramss, ret } => {
                let ps = paramss.first()?;
                (ps.len() == 1).then(|| (ps[0].clone(), (**ret).clone()))
            }
            Type::Function { params, ret } if params.len() == 1 => {
                Some((params[0].clone(), (**ret).clone()))
            }
            Type::Class { .. } => self.st.base_type_seq(ty).into_iter().find_map(|b| {
                let f = match &b {
                    Type::Function { .. } => Some(b.clone()),
                    Type::Class { sym, args } => self.st.function_class_shape(*sym, args),
                    _ => None,
                };
                match f {
                    Some(Type::Function { params, ret }) if params.len() == 1 => {
                        Some((params[0].clone(), *ret))
                    }
                    _ => None,
                }
            }),
            _ => None,
        }
    }

    fn conversion_provides(&self, id: SymbolId, from: &Type, to: &Type) -> bool {
        let s = self.st.get(id);
        if !s.flags.contains(Flags::IMPLICIT) {
            return false;
        }
        if self.first_clause_is_implicit(id) {
            return false;
        }
        let Some((param, ret)) = self.view_shape(&self.implicit_candidate_ty(id)) else {
            return false;
        };
        let tps = s.tparams.clone();
        if tps.is_empty() {
            return self.weak_conforms(from, &param)
                && self.st.is_sub_type(&ret, to)
                && self.conv_implicits_resolve(id, from);
        }
        // Read the argument at the parameter's own class before solving:
        // `IterableFactory.toFactory(factory: IterableFactory[CC])` given
        // `ArrayBuffer.type` has to see `IterableFactory[ArrayBuffer]`, or `CC`
        // falls through to `AnyRef`.
        let targs = self.conv_targs(id, &self.align_to_param_class(&param, from));
        let param_s = crate::symbol::subst_tparams_slice(&tps, &targs, &param);
        let ret_s = crate::symbol::subst_tparams_slice(&tps, &targs, &ret);
        if self.weak_conforms(from, &param_s)
            && self.st.is_sub_type(&ret_s, to)
            && self.conv_implicits_resolve(id, from)
        {
            return true;
        }
        // A parameter the *argument* cannot pin down is the wanted type's to
        // fix: `toFactory[A, CC](f: IterableFactory[CC]): Factory[A, CC[A]]`
        // gets `CC` from `ArrayBuffer.type` and `A` only from the wanted
        // `Factory[Int, ArrayBuffer[Int]]`. `conv_targs` fills such a parameter
        // with `AnyRef`, and `Factory[AnyRef, ArrayBuffer[AnyRef]]` conforms to
        // nothing.
        self.open_conversion_fit(id, from, to, &[]).is_some()
    }

    /// Call-site type parameters solved from the *view* that will fill a
    /// function-typed implicit parameter.
    ///
    /// `List[Option[Int]].flatten` is
    /// `flatten[B](implicit asIterable: Option[Int] => IterableOnce[B])`: `B`
    /// appears nowhere else, so only the witness can pin it, and the witness
    /// is a conversion rather than a value —
    /// `Option.option2Iterable[Int]: Option[Int] => Iterable[Int]`. Unifying
    /// its result `Iterable[Int]` against the wanted `IterableOnce[B]` (the
    /// candidate side widens to the wanted class, as everywhere else in
    /// [`Unify`]) gives `B = Int`.
    ///
    /// Without this the search returned nothing, `adapt_implicit_apply` left
    /// the method type standing, and the whole selection was eta-expanded into
    /// a function value — `println(List(Some(1), None, Some(3)).flatten)`
    /// printed `Main$$$anonfun$0@…`. (`Typer::reject_unapplied_implicit_clause`
    /// is the backstop that turns any remaining case of that into an error;
    /// this is what makes the case at hand *compile* instead.)
    ///
    /// The source type has to be known already: a view out of a type the
    /// search itself is still solving would let any conversion in scope claim
    /// the parameter.
    pub(crate) fn view_undet_bindings(
        &self,
        pt: &Type,
        undet: &[SymbolId],
    ) -> Option<Vec<(SymbolId, Type)>> {
        let Type::Function { params, ret } = pt else {
            return None;
        };
        if params.len() != 1 || undet.is_empty() {
            return None;
        }
        let from = &params[0];
        let unknowns: rustc_hash::FxHashSet<u32> = undet.iter().map(|s| s.0).collect();
        if mentions_unknown(from, &unknowns) {
            return None;
        }
        let mut cands: Vec<SymbolId> = self.implicits_in_scope();
        cands.extend(self.companion_implicits(from));
        cands.extend(self.companion_implicits(ret));
        let mut hits: Vec<(SymbolId, Vec<(SymbolId, Type)>)> = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        for id in cands {
            if !seen.insert(id.0) {
                continue;
            }
            let Some(res) = self.conversion_result(id, from) else {
                continue;
            };
            if !self.conv_implicits_resolve(id, from) {
                continue;
            }
            let mut u = Unify::new(self, std::iter::empty(), undet.iter().copied());
            if !u.unify(&res, ret) {
                continue;
            }
            let sol: Vec<(SymbolId, Type)> = undet
                .iter()
                .filter_map(|d| u.solved(*d).map(|t| (*d, t)))
                .collect();
            if sol.len() != undet.len() {
                continue;
            }
            hits.push((id, sol));
        }
        match self.most_specific(hits.iter().map(|(id, _)| *id).collect()) {
            ImplicitSearch::Found(w) => hits
                .into_iter()
                .find(|(id, _)| *id == w)
                .map(|(_, sol)| sol),
            // Two conversions that disagree about the type argument are an
            // ambiguity, not a guess.
            _ => None,
        }
    }

    /// Every implicit clause of a conversion has a witness.
    fn conv_implicits_resolve(&self, id: SymbolId, from: &Type) -> bool {
        self.conv_implicit_params(id, from)
            .iter()
            .flatten()
            .all(|want| self.search_implicit_at(want, 1).is_found())
    }

    pub(crate) fn search_implicit(&self, pt: &Type) -> ImplicitSearch {
        *self.diverged_implicit.borrow_mut() = None;
        self.search_implicit_at(pt, 0)
    }

    pub(crate) fn search_implicit_at(&self, pt: &Type, depth: usize) -> ImplicitSearch {
        self.search_implicit_undet(pt, &[], depth).0
    }

    /// Implicit search that also solves `undet`, the call-site type parameters
    /// that only the witness can pin down (`xs.toMap` infers `K`/`V` from the
    /// `A <:< (K, V)` it finds). The returned bindings are those of the winner.
    pub(crate) fn search_implicit_undet(
        &self,
        pt: &Type,
        undet: &[SymbolId],
        depth: usize,
    ) -> (ImplicitSearch, Vec<(SymbolId, Type)>) {
        let mut fits: Vec<(SymbolId, ImplicitFit)> = self
            .implicits_in_scope()
            .into_iter()
            .filter_map(|id| self.implicit_fit_at(id, pt, depth, undet).map(|f| (id, f)))
            .collect();
        if fits.is_empty() {
            fits = self
                .companion_implicits(pt)
                .into_iter()
                .filter_map(|id| self.implicit_fit_at(id, pt, depth, undet).map(|f| (id, f)))
                .collect();
            fits.sort_by_key(|(id, _)| id.0);
            fits.dedup_by_key(|(id, _)| id.0);
        }
        if fits.is_empty() {
            if let Some(c) = self.conforms_witness(pt) {
                fits = self
                    .implicit_fit_at(c, pt, depth, undet)
                    .map(|f| vec![(c, f)])
                    .unwrap_or_default();
            }
        }
        let cands: Vec<SymbolId> = fits.iter().map(|(id, _)| *id).collect();
        let found = self.most_specific(cands);
        let bindings = match &found {
            ImplicitSearch::Found(w) => fits
                .iter()
                .find(|(id, _)| id == w)
                .map(|(_, f)| f.undet.clone())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        (found, bindings)
    }

    /// `Predef.$conforms[A]: A => A`, when the wanted type is a one-argument
    /// function type that it could satisfy.
    ///
    /// nsc has `$conforms` in scope everywhere (it is a `Predef` member and
    /// `Predef._` is imported into every compilation unit), which is how
    /// `implicitly[String => CharSequence]` and, through it,
    /// `Ordering.ordered[A](implicit asComparable: A => Comparable[A])`
    /// resolve -- slick's `ScalaBaseType[Null]` needs exactly that chain for
    /// its `Ordering[Null]`. Our `Predef` members are entered into the base
    /// scope before `prelude_conform` adds `$conforms`, so the ordinary scope
    /// walk never sees it. Offering it here, only after every real candidate
    /// has failed and only against a function type, keeps it from displacing
    /// anything: an identity view is what nsc falls back on too.
    fn conforms_witness(&self, pt: &Type) -> Option<SymbolId> {
        let Type::Function { params, .. } = pt else {
            return None;
        };
        if params.len() != 1 {
            return None;
        }
        let predef = self.st.predef;
        if predef.is_none() {
            return None;
        }
        self.st
            .get(predef)
            .members
            .iter()
            .copied()
            .find(|&m| self.st.get(m).name == "$conforms")
    }

    pub(crate) fn search_conversion(&self, from: &Type, to: &Type) -> ImplicitSearch {
        let local: Vec<SymbolId> = self
            .implicits_in_scope()
            .into_iter()
            .filter(|id| self.conversion_provides(*id, from, to))
            .collect();
        if !local.is_empty() {
            return self.most_specific(local);
        }
        let mut comps: Vec<SymbolId> = self
            .companion_implicits(to)
            .into_iter()
            .chain(self.companion_implicits(from))
            .filter(|id| self.conversion_provides(*id, from, to))
            .collect();
        comps.sort_by_key(|id| id.0);
        comps.dedup();
        self.most_specific(comps)
    }

    /// A view from `from` to `to` where `to` still mentions the *callee's*
    /// undetermined type parameters, and the view is what settles them.
    ///
    /// nsc runs `inferView` with `Context.undetparams` in play. `xs.to(Vector)`
    /// is `to[C1](f: Factory[A, C1]): C1`, and the only thing that can say what
    /// `C1` is is the conversion itself —
    /// `IterableFactory.toFactory[A, CC](f: IterableFactory[CC]): Factory[A, CC[A]]`
    /// gives `C1 = Vector[A]`. Comparing the *declared* result with the wanted
    /// type, as [`Self::conversion_provides`] does, leaves `C1` unbound and no
    /// conversion ever applies.
    ///
    /// Returns the conversion, the target with those parameters filled in, and
    /// the bindings. Every one of `open` has to come out solved: a view that
    /// settles only some of them is no better than none.
    pub(crate) fn search_conversion_open(
        &self,
        from: &Type,
        to: &Type,
        open: &[SymbolId],
    ) -> Option<OpenView> {
        if open.is_empty() || from.is_no_type() || from.is_error() {
            return None;
        }
        let mut cands: Vec<SymbolId> = self.implicits_in_scope();
        cands.extend(self.companion_implicits(to));
        cands.extend(self.companion_implicits(from));
        cands.sort_by_key(|id| id.0);
        cands.dedup();
        let mut hits: Vec<OpenView> = Vec::new();
        for id in cands {
            if let Some((solved, binds)) = self.open_conversion_fit(id, from, to, open) {
                hits.push((id, solved, binds));
            }
        }
        // Distinct conversions that agree on the answer are not a conflict;
        // ones that disagree are, and nsc would report them ambiguous. Stay
        // out of the way there and let the ordinary diagnostic stand.
        let first = hits.first()?.clone();
        hits.iter().all(|(_, t, _)| *t == first.1).then_some(first)
    }

    fn open_conversion_fit(
        &self,
        id: SymbolId,
        from: &Type,
        to: &Type,
        open: &[SymbolId],
    ) -> Option<(Type, Vec<(SymbolId, Type)>)> {
        let s = self.st.get(id);
        if !s.flags.contains(Flags::IMPLICIT) {
            return None;
        }
        if self.first_clause_is_implicit(id) {
            return None;
        }
        let cand_ty = self.implicit_candidate_ty(id);
        let (param, ret) = match &*cand_ty {
            Type::Method { paramss, ret } => {
                let ps = paramss.first().cloned().unwrap_or_default();
                if ps.len() != 1 {
                    return None;
                }
                (ps[0].clone(), (**ret).clone())
            }
            Type::Function { params, ret } if params.len() == 1 => {
                (params[0].clone(), (**ret).clone())
            }
            _ => return None,
        };
        let tps = s.tparams.clone();
        // What the *argument* pins down first (`CC = Vector` from
        // `IterableFactory[CC]`), the way the member-directed search does it.
        // The argument is matched at the parameter's own class:
        // `IterableFactory[CC]` against a companion whose *base type* there is
        // `IterableFactory[ArrayBuffer]`, not against `ArrayBuffer.type`.
        let from_at_param = self.align_to_param_class(&param, from);
        let solved_from_arg: Vec<Option<Type>> = tps
            .iter()
            .map(|tp| unify_conv_tparam(*tp, &param, &from_at_param))
            .collect();
        let (known_ids, known_tys): (Vec<SymbolId>, Vec<Type>) = tps
            .iter()
            .zip(solved_from_arg.iter())
            .filter_map(|(tp, t)| t.clone().map(|t| (*tp, t)))
            .unzip();
        let param_k = crate::symbol::subst_tparams_slice(&known_ids, &known_tys, &param);
        if !self.weak_conforms(from, &param_k) {
            return None;
        }
        let ret_k = crate::symbol::subst_tparams_slice(&known_ids, &known_tys, &ret);
        // What is left of the conversion's own parameters, plus the callee's,
        // are all unknowns of one two-sided unification.
        let rest: Vec<SymbolId> = tps
            .iter()
            .zip(solved_from_arg.iter())
            .filter_map(|(tp, t)| t.is_none().then_some(*tp))
            .collect();
        let mut u = Unify::new(self, rest.iter().copied(), open.iter().copied());
        if !u.unify(&ret_k, to) {
            return None;
        }
        // With nothing left to solve on either side this whole pass is a
        // *shape* check, and a wildcard unifies with anything: `Iterable[_]`
        // was accepted for a wanted `IterableOnce[ColumnOption[Nothing]]`. So
        // `Option.option2Iterable` answered the view out of an
        // `Option[Default[_]]`, which made the monomorphic
        // `Set#++(IterableOnce[A]): Set[A]` applicable and pinned the whole
        // `Set() ++ …` chain at `Set[ColumnOption[Nothing]]` -- against an
        // invariant `Set[ColumnOption[_]]` parameter
        // (slick `jdbc/JdbcModelBuilder.scala:279`; real scalac has no such
        // view here and takes the polymorphic `concat[B >: A]`). Where there
        // is nothing to solve, conformance is the question, and
        // `conversion_provides` has already asked it the honest way.
        if rest.is_empty() && open.is_empty() && !self.st.is_sub_type(&ret_k, to) {
            return None;
        }
        // Only the callee's type parameters the *wanted* type mentions can be
        // settled by this unification; the others are settled by the rest of
        // the call, or by an implicit clause, or not at all. Demanding a
        // solution for every one of them threw away every fit for a method
        // whose signature has a type parameter outside the parameter this
        // view is for -- slick's
        //
        //   def === [P2, R](e: Rep[P2])(implicit om: OptionMapper2[B1, B1, Boolean, P1, P2, R]): Rep[R]
        //
        // is the shape: `Rep[P2]` says nothing about `R`, so `column === 1L`
        // could not reach the `Long => Rep[Long]` view that makes it
        // applicable at all, and came out `no matching overload for
        // (Rep[P2])(OptionMapper2[…])Rep[R] with arguments (1L)`. The answer
        // is unaffected -- a type parameter `to` does not mention cannot
        // appear in the substitution -- and `solved_to` is still required to
        // be free of type parameters below.
        let mut binds = Vec::new();
        for o in open {
            if !crate::check::mentions_tparam(to, std::slice::from_ref(o)) {
                continue;
            }
            binds.push((*o, fold_applied(&u.solved(*o)?)));
        }
        let ids: Vec<SymbolId> = binds.iter().map(|(i, _)| *i).collect();
        let tys: Vec<Type> = binds.iter().map(|(_, t)| t.clone()).collect();
        let solved_to = fold_applied(&crate::symbol::subst_tparams_slice(&ids, &tys, to));
        if crate::check::mentions_any_tparam(&solved_to) {
            return None;
        }
        if !self.conv_implicits_resolve(id, from) {
            return None;
        }
        Some((solved_to, binds))
    }

    fn is_as_specific_type(&self, a: SymbolId, b: SymbolId) -> bool {
        let ra = self.implicit_result_ty(a);
        let rb = self.implicit_result_ty(b);
        if !self.st.is_sub_type(&ra, &rb) {
            return false;
        }
        match (self.conversion_arg_ty(a), self.conversion_arg_ty(b)) {
            (Some(aa), Some(ab)) => self.st.is_sub_type(&aa, &ab),
            (Some(_), None) => false,
            (None, Some(_)) => true,
            (None, None) => true,
        }
    }

    /// Direct owner must be class-like (nsc `owner.isSubClass`). A method-local
    /// implicit's owner is the method, so it does not win on origin against an
    /// inherited class member.
    fn is_as_specific_origin(&self, a: SymbolId, b: SymbolId) -> bool {
        let oa = self.st.get(a).owner;
        let ob = self.st.get(b).owner;
        if oa.is_none() || ob.is_none() || oa == ob {
            return false;
        }
        if !self.st.get(oa).is_class_like() || !self.st.get(ob).is_class_like() {
            return false;
        }
        self.st.is_sub_type(
            &Type::Class {
                sym: oa,
                args: vec![],
            },
            &Type::Class {
                sym: ob,
                args: vec![],
            },
        )
    }

    /// nsc `Infer#isStrictlyMoreSpecific`: the *sum* of the specificity
    /// comparison and the owner-subclass comparison has to come out positive.
    /// Two candidates of the same type are told apart by their owner
    /// (`ConstColumn`'s own context-bound evidence beats the `tpe` it inherits
    /// from `Rep.TypedRep`), and a type/origin disagreement cancels out to
    /// ambiguous — both as in nsc.
    fn strictly_more_specific(&self, a: SymbolId, b: SymbolId) -> bool {
        if a == b {
            return false;
        }
        let spec =
            i32::from(self.is_as_specific_type(a, b)) - i32::from(self.is_as_specific_type(b, a));
        let sub = i32::from(self.is_as_specific_origin(a, b))
            - i32::from(self.is_as_specific_origin(b, a));
        spec + sub > 0
    }

    fn most_specific(&self, cands: Vec<SymbolId>) -> ImplicitSearch {
        let cands = self.drop_module_classes(cands);
        match cands.len() {
            0 => ImplicitSearch::None,
            1 => ImplicitSearch::Found(cands[0]),
            _ => {
                let winners: Vec<SymbolId> = cands
                    .iter()
                    .copied()
                    .filter(|&a| !cands.iter().any(|&b| self.strictly_more_specific(b, a)))
                    .collect();
                match winners.len() {
                    0 => ImplicitSearch::Ambiguous(cands),
                    1 => ImplicitSearch::Found(winners[0]),
                    _ => ImplicitSearch::Ambiguous(winners),
                }
            }
        }
    }

    /// `implicit object GetString` is one implicit value, not two. Both the
    /// module and its module class carry the flag and have the same type, so a
    /// search that reaches both would report them as ambiguous with
    /// themselves. Keep the module: that is what a reference to the name means.
    fn drop_module_classes(&self, cands: Vec<SymbolId>) -> Vec<SymbolId> {
        if cands.len() < 2 {
            return cands;
        }
        let modules: Vec<SymbolId> = cands
            .iter()
            .copied()
            .filter(|&c| self.st.get(c).kind == SymKind::Module)
            .collect();
        if modules.is_empty() {
            return cands;
        }
        cands
            .iter()
            .copied()
            .filter(|&c| {
                self.st.get(c).kind != SymKind::ModuleClass
                    || !modules.iter().any(|&m| self.st.module_class_of(m) == c)
            })
            .collect()
    }

    /// The result type with the candidate's own type parameters erased to
    /// wildcards, nsc's `isAsSpecific` on a `PolyType`: `Show[Int]` conforms to
    /// `implicit def anyShow[A]: Show[A]`'s `Show[_]` but not the other way
    /// round, so the monomorphic instance wins.
    fn implicit_result_ty(&self, id: SymbolId) -> Type {
        let cand_ty = self.implicit_candidate_ty(id);
        let ret = match &*cand_ty {
            Type::Method { ret, .. } => (**ret).clone(),
            Type::Function { ret, .. } => (**ret).clone(),
            t => t.clone(),
        };
        self.erase_method_tparams(id, &ret)
    }

    fn conversion_arg_ty(&self, id: SymbolId) -> Option<Type> {
        let cand_ty = self.implicit_candidate_ty(id);
        match &*cand_ty {
            Type::Method { paramss, .. } => {
                let ps = paramss.first()?;
                if ps.len() == 1 {
                    Some(ps[0].clone())
                } else {
                    None
                }
            }
            Type::Function { params, .. } if params.len() == 1 => Some(params[0].clone()),
            _ => None,
        }
    }

    /// Implicit conversion from `from` whose result type has member `name`.
    pub(crate) fn search_extension(
        &mut self,
        from: &Type,
        name: &str,
        span: Span,
    ) -> Option<(SymbolId, SymbolId, Type)> {
        // A higher-kinded conversion is applicable only if its own implicit
        // clause has a witness (`conv_param_matches`), and that search cannot
        // load anything -- it takes `&self`. The witness for `FlatMap[Box]`
        // lives on `Box`'s companion, which is a class file nothing else asks
        // for, so warm the receiver's implicit scope here, where the mutable
        // borrow still exists.
        self.warm_implicit_scope(from);
        let mut hits: Vec<(SymbolId, SymbolId, Type)> = Vec::new();
        let mut ids = self.implicits_in_scope();
        ids.extend(
            self.companion_implicits(from)
                .into_iter()
                .chain(self.companion_implicits(&Type::Any)),
        );
        ids.sort_by_key(|id| id.0);
        ids.dedup();
        for id in ids {
            let Some(to) = self.conversion_result(id, from) else {
                continue;
            };
            let Some(cls) = self.st.class_sym_of(&to) else {
                continue;
            };
            // Load the conversion *result* (e.g. ListHasAsScala) so `asScala`
            // is visible. Do not complete the *argument* type: that would
            // install `java.lang.String#toUpperCase(Locale)` onto Predef
            // String and shadow StringOps.
            self.ensure_java_loaded(cls, span);
            let mut members: Vec<SymbolId> = self
                .st
                .lookup_member(cls, name)
                .into_iter()
                .filter(|&m| !self.st.get(m).flags.contains(Flags::STATIC))
                .collect();
            // The hand-written prelude is not a complete `StringOps` (or
            // `ArrayOps`, …), and until now nothing asked the library pickle
            // about the conversion *result*: `supply_from_pickle` only ever
            // saw the receiver, `java.lang.String`, which has no
            // `ScalaSignature` at all. Ask the result's own pickle when the
            // prelude has nothing, so `"abc".groupBy(f)` resolves the same way
            // `List(1).groupBy(f)` already does. The prelude still wins
            // whenever it declares the member.
            if members.is_empty() {
                members = self
                    .supply_from_pickle(&to, name)
                    .into_iter()
                    .filter(|&m| !self.st.get(m).flags.contains(Flags::STATIC))
                    .collect();
            }
            if let Some(m) = members.first() {
                hits.push((id, *m, to));
            }
        }
        hits.sort_by_key(|(c, m, _)| (c.0, m.0));
        hits.dedup_by_key(|(c, m, _)| (c.0, m.0));
        self.drop_inherited_duplicates(&mut hits);
        self.drop_overridden_conversions(&mut hits);
        self.drop_inapplicable_conversions(&mut hits, name);
        match hits.len() {
            1 => Some(hits.pop().unwrap()),
            0 => None,
            _ => {
                // nsc Predef: `augmentString` (StringOps) wins over `wrapString`
                // (WrappedString / Seq) because wrapString is lower priority.
                // Prefer the conversion whose result *declares* the member.
                let declared: Vec<(SymbolId, SymbolId, Type)> = hits
                    .iter()
                    .filter(|(_, m, to)| self.conversion_declares_member(to, *m))
                    .cloned()
                    .collect();
                if declared.len() == 1 {
                    return Some(declared.into_iter().next().unwrap());
                }
                let mut pool = if declared.is_empty() { hits } else { declared };
                // nsc priority: a conversion `Predef` declares itself beats one
                // it inherits from `LowPriorityImplicits`. `0.5.isNaN` is
                // `double2Double(0.5).isNaN()` in scalac, not `RichDouble`.
                if pool.iter().any(|(c, _, _)| !self.st.get(*c).low_priority) {
                    pool.retain(|(c, _, _)| !self.st.get(*c).low_priority);
                }
                if pool.len() == 1 {
                    return Some(pool.into_iter().next().unwrap());
                }
                let convs: Vec<SymbolId> = pool.iter().map(|(c, _, _)| *c).collect();
                let winners: Vec<SymbolId> = convs
                    .iter()
                    .copied()
                    .filter(|&a| {
                        !convs
                            .iter()
                            .any(|&b| self.conv_arg_strictly_more_specific(b, a))
                    })
                    .collect();
                if winners.len() != 1 {
                    if let Some(hit) = self.pick_array_ops_conv(from, &pool) {
                        return Some(hit);
                    }
                    return None;
                }
                pool.into_iter().find(|(c, _, _)| *c == winners[0])
            }
        }
    }

    /// A conversion whose member cannot take the arguments the call site
    /// writes is not a candidate for that call.
    ///
    /// nsc's `adaptToArguments` asks for a view whose result has a member
    /// *applicable to these arguments*, so two conversions that merely share
    /// the name do not make an ambiguity. gitbucket's
    ///
    /// ```scala
    /// implicit class RichColumn(c1: Rep[Boolean]) {
    ///   def &&(c2: => Rep[Boolean], guard: => Boolean): Rep[Boolean] = …
    /// }
    /// ```
    ///
    /// sits in scope beside slick's `booleanColumnExtensionMethods`, whose
    /// `&&` takes one argument. Every `a && b` in the project tied between the
    /// two and was reported as `value && is not a member of Rep[Boolean]`.
    ///
    /// Runs only when there is a tie to break and only when the call site's
    /// argument count is known, and only ever *narrows* a set of two or more:
    /// a member this cannot read the shape of stays a candidate.
    fn drop_inapplicable_conversions(
        &self,
        hits: &mut Vec<(SymbolId, SymbolId, Type)>,
        name: &str,
    ) {
        if hits.len() < 2 {
            return;
        }
        let Some(n) = self.callee_arity else {
            return;
        };
        let keep: Vec<bool> = hits
            .iter()
            .map(|(_, m, to)| {
                let alts = match self.st.class_sym_of(to) {
                    Some(cls) => self.st.lookup_member(cls, name),
                    None => vec![*m],
                };
                let alts = if alts.is_empty() { vec![*m] } else { alts };
                alts.iter().any(|&a| self.member_accepts_arity(a, n))
            })
            .collect();
        if !keep.iter().any(|k| *k) {
            return;
        }
        let mut it = keep.into_iter();
        hits.retain(|_| it.next().unwrap_or(true));
    }

    /// Whether `m` could be applied to `n` explicit arguments. Deliberately
    /// permissive: a shape this cannot read (a `val` of function type, a
    /// nullary member that is applied through its own `apply`) answers yes,
    /// because the only caller uses this to *drop* alternatives.
    fn member_accepts_arity(&self, m: SymbolId, n: usize) -> bool {
        let Type::Method { paramss, .. } = &self.st.get(m).ty else {
            return true;
        };
        let Some(first) = paramss.first() else {
            return true;
        };
        if first.len() == n {
            return true;
        }
        if n + 1 >= first.len() && matches!(first.last(), Some(Type::Repeated(_))) {
            return true;
        }
        if n > first.len() {
            return false;
        }
        // Short of the clause: legal when every parameter left over is
        // implicit or has a default.
        let params = self.st.get(m).params.clone();
        if params.len() != first.len() {
            return true;
        }
        params[n..].iter().all(|p| {
            let f = self.st.get(*p).flags;
            f.contains(Flags::IMPLICIT) || f.contains(Flags::DEFAULTPARAM)
        })
    }

    /// One conversion reached by two routes is one candidate, not an ambiguity.
    ///
    /// `object CollectionConverters extends AsScalaExtensions` has
    /// `ListHasAsScala` both as its own member (the prelude declares it there)
    /// and as the trait's, and both are in scope after
    /// `import scala.jdk.CollectionConverters._`. They are the same conversion
    /// -- same name, same result type, same member -- so the one declared
    /// further away is dropped rather than left to tie with itself.
    fn drop_inherited_duplicates(&self, hits: &mut Vec<(SymbolId, SymbolId, Type)>) {
        if hits.len() < 2 {
            return;
        }
        let shadowed: Vec<bool> = hits
            .iter()
            .map(|(c, m, to)| {
                let owner = self.st.get(*c).owner;
                let name = &self.st.get(*c).name;
                hits.iter().any(|(c2, m2, to2)| {
                    let owner2 = self.st.get(*c2).owner;
                    c2 != c
                        && m2 == m
                        && to2 == to
                        && &self.st.get(*c2).name == name
                        && owner != owner2
                        && self.st.is_ancestor_of(owner, owner2)
                })
            })
            .collect();
        let mut keep = shadowed.iter().map(|s| !s);
        hits.retain(|_| keep.next().unwrap_or(true));
    }

    /// A conversion a subclass *overrides* is not a second candidate.
    ///
    /// `trait Integral[T] extends Numeric[T]` narrows the result:
    /// `mkNumericOps(lhs: T): IntegralOps` over `Numeric`'s `: NumericOps`.
    /// `import seq.integral._` brings both names into scope, and because the
    /// two results are different classes declaring different `unary_-`
    /// symbols, [`Self::drop_inherited_duplicates`] -- which asks for the
    /// *same* member and the *same* result -- saw two unrelated candidates and
    /// the search gave up. In nsc there is one member, the derived one.
    fn drop_overridden_conversions(&self, hits: &mut Vec<(SymbolId, SymbolId, Type)>) {
        if hits.len() < 2 {
            return;
        }
        let overridden: Vec<bool> = hits
            .iter()
            .map(|(c, _, _)| {
                let name = &self.st.get(*c).name;
                let owner = self.st.get(*c).owner;
                !owner.is_none()
                    && hits.iter().any(|(c2, _, _)| {
                        let owner2 = self.st.get(*c2).owner;
                        c2 != c
                            && &self.st.get(*c2).name == name
                            && owner2 != owner
                            && !owner2.is_none()
                            && self.st.is_ancestor_of(owner, owner2)
                    })
            })
            .collect();
        let mut keep = overridden.iter().map(|s| !s);
        hits.retain(|_| keep.next().unwrap_or(true));
    }

    fn conversion_declares_member(&self, to: &Type, member: SymbolId) -> bool {
        let Some(cls) = self.st.class_sym_of(to) else {
            return false;
        };
        self.st.get(cls).members.contains(&member)
    }

    /// nsc: primitive `intArrayOps` wins over `genericArrayOps`; `refArrayOps`
    /// wins for `Array[AnyRef]` / `Array[String]`; unconstrained `Array[T]` uses
    /// `genericArrayOps`. After erasing method tparams both generic and ref look
    /// like `Array[_]`, so pick by the source element.
    fn pick_array_ops_conv(
        &self,
        from: &Type,
        hits: &[(SymbolId, SymbolId, Type)],
    ) -> Option<(SymbolId, SymbolId, Type)> {
        if hits.is_empty() {
            return None;
        }
        if !hits.iter().all(|(_, _, to)| {
            self.st
                .class_sym_of(to)
                .is_some_and(|c| self.st.get(c).name == "ArrayOps")
        }) {
            return None;
        }
        let elem = match from {
            Type::Array(e) => e.as_ref(),
            _ => return None,
        };
        let prefer = match elem {
            Type::Int => "intArrayOps",
            Type::Long => "longArrayOps",
            Type::Byte => "byteArrayOps",
            Type::Short => "shortArrayOps",
            Type::Char => "charArrayOps",
            Type::Float => "floatArrayOps",
            Type::Double => "doubleArrayOps",
            Type::Boolean => "booleanArrayOps",
            Type::Unit | Type::NoType => "unitArrayOps",
            Type::TypeParam(_) | Type::Any | Type::AnyVal => "genericArrayOps",
            _ => "refArrayOps",
        };
        let named: Vec<_> = hits
            .iter()
            .filter(|(c, _, _)| self.st.get(*c).name == prefer)
            .cloned()
            .collect();
        if named.len() == 1 {
            return named.into_iter().next();
        }
        let gen: Vec<_> = hits
            .iter()
            .filter(|(c, _, _)| self.st.get(*c).name == "genericArrayOps")
            .cloned()
            .collect();
        if gen.len() == 1 {
            return gen.into_iter().next();
        }
        None
    }

    fn conv_arg_strictly_more_specific(&self, a: SymbolId, b: SymbolId) -> bool {
        a != b
            && match (self.conversion_arg_ty(a), self.conversion_arg_ty(b)) {
                (Some(aa), Some(ab)) => {
                    let aa = self.erase_method_tparams(a, &aa);
                    let ab = self.erase_method_tparams(b, &ab);
                    self.st.is_sub_type(&aa, &ab) && !self.st.is_sub_type(&ab, &aa)
                }
                _ => false,
            }
    }

    fn erase_method_tparams(&self, id: SymbolId, ty: &Type) -> Type {
        let tps = self.st.get(id).tparams.clone();
        if tps.is_empty() {
            return ty.clone();
        }
        let wilds = vec![Type::Wildcard; tps.len()];
        crate::symbol::subst_tparams_slice(&tps, &wilds, ty)
    }

    fn conversion_result(&self, id: SymbolId, from: &Type) -> Option<Type> {
        if !self.st.get(id).flags.contains(Flags::IMPLICIT) {
            return None;
        }
        let cand_ty = self.implicit_candidate_ty(id);
        match &*cand_ty {
            Type::Method { paramss, ret } => {
                let ps = paramss.first().filter(|ps| ps.len() == 1)?;
                if self.conv_param_matches(id, from, &ps[0]) {
                    Some(self.instantiate_conv_type(id, from, ret))
                } else {
                    None
                }
            }
            Type::Function { params, ret } if params.len() == 1 => {
                if self.conv_param_matches(id, from, &params[0]) {
                    Some(self.instantiate_conv_type(id, from, ret))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn instantiate_conv_type(&self, id: SymbolId, from: &Type, ty: &Type) -> Type {
        let tps = &self.st.get(id).tparams;
        if tps.is_empty() {
            return ty.clone();
        }
        let args_t = self.conv_targs(id, from);
        crate::symbol::subst_tparams_slice(tps, &args_t, ty)
    }

    /// The conversion's own type arguments, solved from the receiver type.
    fn conv_targs(&self, id: SymbolId, from: &Type) -> Vec<Type> {
        let tps = &self.st.get(id).tparams;
        let cand_ty = self.implicit_candidate_ty(id);
        let param: Option<&Type> = match &*cand_ty {
            Type::Method { paramss, .. } => paramss.first().and_then(|c| c.first()),
            Type::Function { params, .. } => params.first(),
            _ => None,
        };
        let Some(param) = param else {
            return vec![Type::AnyRef; tps.len()];
        };
        // nsc solves the conversion's parameters against the receiver's *base
        // type* at the parameter's class, not against the receiver as written.
        // `implicit def mapAsScalaMapConverter[K, V](m: java.util.Map[K, V])`
        // applied to a `ConfigObject` (which merely *extends*
        // `java.util.Map[String, ConfigValue]`) has nothing to zip argument by
        // argument, so both `K` and `V` fell through to `AnyRef` and
        // `config.root.asScala` came back a `Map[AnyRef, AnyRef]`.
        let seen_as = match param {
            Type::Class { sym, args } if !args.is_empty() => self.base_type_instance(from, *sym, 0),
            _ => None,
        };
        let from = seen_as.as_ref().unwrap_or(from);
        let mut solved: Vec<Option<Type>> = tps
            .iter()
            .map(|tp| unify_conv_tparam(*tp, param, from))
            .collect();
        self.solve_conv_targs_from_implicits(id, tps, &mut solved);
        solved
            .into_iter()
            .map(|t| t.unwrap_or(Type::AnyRef))
            .collect()
    }

    /// A type parameter the receiver does not mention can still be pinned by
    /// the conversion's *own* implicit clause.
    ///
    /// cats writes `catsSyntaxApplicativeError[F[_], E, A](fa: F[A])(implicit
    /// F: ApplicativeError[F, E])`: `E` appears nowhere in `F[A]`, so falling
    /// back to `AnyRef` made `fa.attempt` an `F[Either[AnyRef, A]]` -- a
    /// member that resolves and then fails to conform to anything the caller
    /// wrote. The witness in scope (`Async[F] <: MonadError[F, Throwable]`)
    /// says `E = Throwable`, which is exactly how nsc solves it.
    ///
    /// Only run for a parameter the *result* type mentions: this is on the
    /// path of every candidate conversion, and an implicit search per
    /// candidate is not free.
    fn solve_conv_targs_from_implicits(
        &self,
        id: SymbolId,
        tps: &[SymbolId],
        solved: &mut [Option<Type>],
    ) {
        let cand_ty = self.implicit_candidate_ty(id);
        let Type::Method { paramss, ret } = &*cand_ty else {
            return;
        };
        if paramss.len() < 2 {
            return;
        }
        let undet: Vec<SymbolId> = tps
            .iter()
            .zip(solved.iter())
            .filter(|(tp, s)| s.is_none() && crate::check::mentions_tparam(ret, &[**tp]))
            .map(|(tp, _)| *tp)
            .collect();
        if undet.is_empty() {
            return;
        }
        for clause in &paramss[1..] {
            for p in clause {
                let known: Vec<Type> = tps
                    .iter()
                    .zip(solved.iter())
                    .map(|(tp, s)| s.clone().unwrap_or(Type::TypeParam(*tp)))
                    .collect();
                let pt = crate::symbol::subst_tparams_slice(tps, &known, p);
                let (found, bindings) = self.search_implicit_undet(&pt, &undet, 0);
                if !found.is_found() {
                    continue;
                }
                for (tp, t) in bindings {
                    if let Some(i) = tps.iter().position(|x| *x == tp) {
                        if solved[i].is_none() && !t.is_no_type() && !t.is_error() {
                            solved[i] = Some(t);
                        }
                    }
                }
            }
        }
    }

    /// The conversion's *implicit* parameter clauses, with its type parameters
    /// solved from the receiver.
    ///
    /// cats' syntax layer is
    /// `implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])`:
    /// applying it to the receiver alone leaves the second clause unfilled, and
    /// the call goes out with fewer arguments than its descriptor declares.
    pub(crate) fn conv_implicit_params(&self, id: SymbolId, from: &Type) -> Vec<Vec<Type>> {
        let cand_ty = self.implicit_candidate_ty(id);
        let Type::Method { paramss, .. } = &*cand_ty else {
            return Vec::new();
        };
        if paramss.len() < 2 {
            return Vec::new();
        }
        let tps = self.st.get(id).tparams.clone();
        let targs = self.conv_targs(id, from);
        paramss[1..]
            .iter()
            .map(|c| {
                c.iter()
                    .map(|p| crate::symbol::subst_tparams_slice(&tps, &targs, p))
                    .collect()
            })
            .collect()
    }

    fn conv_param_matches(&self, id: SymbolId, from: &Type, param: &Type) -> bool {
        let erased = self.erase_method_tparams(id, param);
        if self.st.is_sub_type(from, &erased) || matches!(erased, Type::Any | Type::Wildcard) {
            return true;
        }
        // `fa: F[A]`, with `F` and `A` both the conversion's own parameters,
        // erases to `?[?]`. Any type applied to the right number of arguments
        // fits it -- a class (`Box[Int]`) just as much as another higher-kinded
        // parameter (`G[Int]`), which is the only shape `is_sub_type`
        // recognised.
        let fits_ctor = match &erased {
            Type::Applied { ctor, args } if matches!(**ctor, Type::Wildcard) => {
                applied_args(from).is_some_and(|a| a.len() == args.len())
            }
            _ => false,
        };
        if !fits_ctor {
            return false;
        }
        // That widening alone would let a higher-kinded conversion claim every
        // applied type, turning "not a member" into "no implicit". A conversion
        // whose own implicit clause has no witness is not applicable (nsc
        // checks the same), and that is what keeps the widening honest. Only
        // the widened path pays for the search; a conversion that matched by
        // conformance is decided exactly as before.
        self.conv_implicit_params(id, from)
            .iter()
            .flatten()
            .all(|want| self.search_implicit_at(want, 1).is_found())
    }

    pub(crate) fn ref_implicit(&self, id: SymbolId, span: Span) -> Tree {
        let cand_ty = self.implicit_candidate_ty(id);
        let ty = match &*cand_ty {
            Type::Method { paramss, ret }
                if paramss.is_empty() || paramss.iter().all(|c| c.is_empty()) =>
            {
                (**ret).clone()
            }
            t => t.clone(),
        };
        let name = self.st.get(id).name.clone();
        let ident = Tree {
            id: scala_rs_parser::NodeId(0),
            span,
            kind: TreeKind::Ident { name: name.clone() },
            ty: ty.clone(),
            sym: id,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let Some(module) = self.wildcard_module_for(id) else {
            // `import b._` where `b` is a *value*: the conversion is an
            // instance member of `b`'s class, so the call needs `b` as its
            // receiver. Emitted as a bare name, codegen loaded `this` and cast
            // it -- `class Main$ cannot be cast to class NoTp` from a program
            // that typechecked.
            let owner = self.st.get(id).owner;
            if let Some(prefix) = self.term_import_prefix_for(owner) {
                return Tree {
                    id: scala_rs_parser::NodeId(0),
                    span,
                    kind: TreeKind::Select {
                        qual: Box::new(prefix.clone()),
                        name,
                    },
                    ty,
                    sym: id,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
            }
            return ident;
        };
        let mcls = self.st.module_class_of(module);
        let qual = Tree {
            id: scala_rs_parser::NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: self.st.get(module).name.clone(),
            },
            ty: Type::ModuleRef(mcls),
            sym: module,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        Tree {
            id: scala_rs_parser::NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(qual),
                name,
            },
            ty,
            sym: id,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        }
    }

    /// The object a wildcard import brought `id` in through, when `id` is
    /// *inherited* by that object rather than declared by it.
    ///
    /// `import tinycats.syntax.all._` (and `import cats.syntax.all._`) makes
    /// `toFlatMapOps` visible, but it is declared by the trait
    /// `FlatMap.ToFlatMapOps` that the object mixes in. Emitted as a bare
    /// name, codegen loads `this` and casts it to that trait:
    /// `Main$ cannot be cast to tinycats.FlatMap$ToFlatMapOps`. The receiver
    /// is the imported object.
    fn wildcard_module_for(&self, id: SymbolId) -> Option<SymbolId> {
        let owner = self.st.get(id).owner;
        if owner.is_none()
            || matches!(
                self.st.get(owner).kind,
                SymKind::Module | SymKind::ModuleClass | SymKind::Package
            )
        {
            return None;
        }
        // A member of the enclosing class is already reachable through `this`.
        if !self.st.this_class.is_none()
            && (owner == self.st.this_class
                || crate::pickle_supply::inherits_from(&self.st, self.st.this_class, owner))
        {
            return None;
        }
        for sc in self.st.scopes.iter().rev() {
            for w in sc.wildcards() {
                if !w.offers(&self.st.get(id).name) {
                    continue;
                }
                let m = w.owner;
                if !matches!(self.st.get(m).kind, SymKind::Module | SymKind::ModuleClass) {
                    continue;
                }
                if crate::pickle_supply::inherits_from(&self.st, m, owner) {
                    return Some(m);
                }
            }
        }
        // Not imported, but reached through a companion object that only
        // *inherits* it (`object Shape extends ConstColumnShapeImplicits`).
        self.implicit_via_module.borrow().get(&id.0).copied()
    }

    pub(crate) fn describe_implicits(&self, ids: &[SymbolId]) -> String {
        ids.iter()
            .map(|id| self.st.get(*id).name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Two-sided unification for implicit search.
///
/// `unknowns` holds the candidate's own type parameters *and* the call-site
/// parameters the search has to solve. One-sided `check::unify_one` cannot do
/// the latter: fitting `<:<.refl[A0]: A0 =:= A0` to `A <:< (K, V)` binds `A0`
/// from the `From` position and then `K`/`V` from the `To` position, in the
/// same pass.
struct Unify<'a> {
    typer: &'a Typer,
    unknowns: rustc_hash::FxHashSet<u32>,
    /// The subset of `unknowns` allowed to stand for a *type constructor*.
    ///
    /// Only the candidate's own parameters are: solving
    /// `buildFromIterableOps[CC[X], A0, A]` means reading `CC := List` off the
    /// wanted type, which is the whole point. A *call site's* undetermined
    /// constructor is not -- it is what ordinary inference from the argument
    /// settles. Letting a conversion bind it made
    /// `firstLength[A, M[+X] <: Iterable[X]](in: M[A])` accept
    /// `IterableOnce.iterableOnceExtensionMethods` as a way to reach `M[A]`
    /// from a `List[Int]` that already conformed with `M := List`
    /// (`tests/fixtures/mism12_lib.scala`, a `ClassCastException` at run time).
    ctor_unknowns: rustc_hash::FxHashSet<u32>,
    bound: rustc_hash::FxHashMap<u32, Type>,
}

impl<'a> Unify<'a> {
    /// `own` are the candidate's own type parameters, `undet` the call site's.
    fn new(
        typer: &'a Typer,
        own: impl IntoIterator<Item = SymbolId>,
        undet: impl IntoIterator<Item = SymbolId>,
    ) -> Self {
        let ctor_unknowns: rustc_hash::FxHashSet<u32> = own.into_iter().map(|s| s.0).collect();
        let mut unknowns = ctor_unknowns.clone();
        unknowns.extend(undet.into_iter().map(|s| s.0));
        Unify {
            typer,
            unknowns,
            ctor_unknowns,
            bound: rustc_hash::FxHashMap::default(),
        }
    }

    /// Whether `ty` is an unknown this unification may solve to a type
    /// *constructor*; see [`Unify::ctor_unknowns`].
    fn unknown_ctor(&self, ty: &Type) -> bool {
        matches!(ty, Type::TypeParam(id) if self.ctor_unknowns.contains(&id.0))
    }

    fn unknown_of(&self, ty: &Type) -> Option<u32> {
        match ty {
            Type::TypeParam(id) if self.unknowns.contains(&id.0) => Some(id.0),
            _ => None,
        }
    }

    /// The solution for `tp` with nested unknowns expanded as far as they go,
    /// *keeping* one that still mentions an unknown. For a caller that goes on
    /// to solve those separately (`implicit_fit_open`).
    fn solved_open(&self, tp: SymbolId) -> Option<Type> {
        let t = self.bound.get(&tp.0)?.clone();
        Some(self.expand(&t, 0))
    }

    /// The solution for `tp`, with any nested unknowns resolved.
    fn solved(&self, tp: SymbolId) -> Option<Type> {
        let t = self.bound.get(&tp.0)?.clone();
        let t = self.expand(&t, 0);
        (!mentions_unknown(&t, &self.unknowns)).then_some(t)
    }

    fn expand(&self, ty: &Type, depth: usize) -> Type {
        if depth > 8 || self.bound.is_empty() {
            return ty.clone();
        }
        if let Type::TypeParam(id) = ty {
            if let Some(t) = self.bound.get(&id.0) {
                return self.expand(&t.clone(), depth + 1);
            }
        }
        let ids: Vec<SymbolId> = self.bound.keys().map(|k| SymbolId(*k)).collect();
        let tys: Vec<Type> = ids
            .iter()
            .map(|i| self.bound.get(&i.0).cloned().unwrap_or(Type::NoType))
            .collect();
        crate::symbol::subst_tparams_slice(&ids, &tys, ty)
    }

    fn bind(&mut self, id: u32, ty: &Type) -> bool {
        if ty.is_no_type() || ty.is_error() || matches!(ty, Type::Wildcard) {
            return false;
        }
        // Occurs check: `A = List[A]` would make `expand` loop.
        if mentions_unknown(ty, &std::iter::once(id).collect()) {
            return false;
        }
        self.bound.insert(id, ty.widen_constant());
        true
    }

    fn unify(&mut self, a: &Type, b: &Type) -> bool {
        self.unify_at(a, b, 0)
    }

    /// One side is an intersection (`CC[A0] with SortedSet[A0]`): every part
    /// of it has to unify with the other side.
    ///
    /// `Some(_)` only when this shape applies. Two refinements on both sides
    /// fall through to the structural equality at the end, as before.
    fn unify_refinement(&mut self, a: &Type, b: &Type, depth: usize) -> Option<bool> {
        let (parents, other) = match (a, b) {
            (Type::Refined { parents, decls }, other) if decls.is_empty() => (parents, other),
            (other, Type::Refined { parents, decls }) if decls.is_empty() => (parents, other),
            _ => return None,
        };
        if matches!(other, Type::Refined { .. }) {
            return None;
        }
        let parents = parents.clone();
        let other = other.clone();
        Some(!parents.is_empty() && parents.iter().all(|p| self.unify_at(p, &other, depth + 1)))
    }

    /// One side is `?F[X, …]` with `?F` an unknown constructor: match `?F`
    /// against the other side's *constructor* and the arguments positionally.
    ///
    /// `Some(_)` only when this shape applies, so an ordinary pair falls
    /// through to the structural cases.
    fn unify_higher_kinded(&mut self, a: &Type, b: &Type, depth: usize) -> Option<bool> {
        let (hk_ctor, hk_args, other) = match (a, b) {
            (Type::Applied { ctor, args }, other) if self.unknown_ctor(ctor) => (ctor, args, other),
            (other, Type::Applied { ctor, args }) if self.unknown_ctor(ctor) => (ctor, args, other),
            _ => return None,
        };
        // Both sides `Applied` with the same arity is already the structural
        // case below; leave it there so a bound constructor is followed.
        if matches!(other, Type::Applied { .. }) {
            return None;
        }
        let (octor, oargs) = as_application(other)?;
        if oargs.len() != hk_args.len() {
            return None;
        }
        let hk_ctor = (**hk_ctor).clone();
        let hk_args = hk_args.clone();
        let oargs = oargs.to_vec();
        Some(
            self.unify_at(&hk_ctor, &octor, depth + 1)
                && hk_args
                    .iter()
                    .zip(oargs.iter())
                    .all(|(x, y)| self.unify_at(x, y, depth + 1)),
        )
    }

    fn unify_at(&mut self, a: &Type, b: &Type, depth: usize) -> bool {
        if depth > 24 {
            return false;
        }
        let a = strip_annot(a);
        let b = strip_annot(b);
        if let Some(id) = self.unknown_of(a) {
            return match self.bound.get(&id).cloned() {
                Some(prev) => self.unify_at(&prev, b, depth + 1),
                None => self.bind(id, b),
            };
        }
        if let Some(id) = self.unknown_of(b) {
            return match self.bound.get(&id).cloned() {
                Some(prev) => self.unify_at(a, &prev, depth + 1),
                None => self.bind(id, a),
            };
        }
        // `_` in the wanted type is a position the search is not asking about.
        // slick writes `packedValue[R](implicit ev: Shape[? <: Level, T, ?, R])`
        // and the witness in scope is a `Shape[? <: Level, E, U, R]`: matching
        // `U` against `?` structurally said "no", so `R` was never solved and
        // the implicit clause stayed unfilled. A bounded one still has to hold
        // its bound.
        match (a, b) {
            (Type::Wildcard, _) | (_, Type::Wildcard) => return true,
            (Type::BoundedWildcard { hi, .. }, other)
            | (other, Type::BoundedWildcard { hi, .. }) => {
                return match hi {
                    Some(h) => {
                        self.typer.st.is_sub_type(other, h) || self.unify_at(other, h, depth + 1)
                    }
                    None => true,
                };
            }
            _ => {}
        }
        // An F-bounded higher-kinded parameter reaches the typer with its
        // bound folded into the type: `buildFromSortedSetOps` is
        // `BuildFrom[CC[A0] with SortedSet[A0], A, CC[A] with SortedSet[A]]`,
        // and `buildFromMapOps` is `BuildFrom[CC[K0, V0] with Map[K0, V0], …]`.
        // That intersection is what tells the `BuildFrom` witnesses apart --
        // they are otherwise the same type -- so every part of it has to hold:
        // a `List[Int]` matches `CC[A0]` but not `SortedSet[A0]`, a
        // `TreeSet[Int]` matches both.
        if let Some(r) = self.unify_refinement(a, b, depth) {
            return r;
        }
        // An *unknown type constructor* applied to arguments, against a
        // concrete application. `BuildFrom`'s only general witness is
        // `buildFromIterableOps[CC[X] <: Iterable[X] with IterableOps[X, CC, _],
        // A0, A]: BuildFrom[CC[A0], A, CC[A]]`, so fitting it to
        // `BuildFrom[List[String], String, ?C]` means reading `CC := List` and
        // `A0 := String` off the first argument -- and only then is `?C`
        // solvable as `CC[A]`. Structurally an `Applied` never equalled a
        // `Class`, so `xs.lazyZip(ys).map(f)` reported
        // `could not find implicit value of type BuildFrom[…, C]`.
        // One-sided `check::unify_one` already reads a constructor this way;
        // this is the two-sided pass, which is the only one that also solves
        // the call site's `?C`.
        if let Some(r) = self.unify_higher_kinded(a, b, depth) {
            return r;
        }
        // Two *type lambdas*, one of which still carries unknowns.
        // `implicit def readerMonad[R]: Monad[({ type L[X] = Reader[R, X] })#L]`
        // has to answer a wanted `Monad[({ type L[X] = Reader[Int, X] })#L]`.
        // Neither side applies an unknown *constructor* -- both are aliases --
        // so the case above does not fire, and structurally the two refinements
        // are different symbols. Applying both to the same parameters turns the
        // question into `Reader[R, X]` against `Reader[Int, X]`, which is what
        // solves `R`.
        if let Some((ea, eb)) = self.typer.st.eta_expand_pair(a, b) {
            return self.unify_at(&ea, &eb, depth + 1);
        }
        match (a, b) {
            (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, args: a2 }) => {
                if s1 == s2 && a1.len() == a2.len() {
                    return a1
                        .iter()
                        .zip(a2.iter())
                        .all(|(x, y)| self.unify_at(x, y, depth + 1));
                }
                if s1 == s2 {
                    return a1.is_empty() || a2.is_empty();
                }
                // `=:=[A0, A0]` fitted to `<:<[From, To]`: widen the candidate
                // side to the wanted class before matching arguments.
                if let Some(Type::Class { args, .. }) = self.typer.base_type_instance(a, *s2, 0) {
                    if args.len() == a2.len()
                        && args
                            .iter()
                            .zip(a2.iter())
                            .all(|(x, y)| self.unify_at(x, y, depth + 1))
                    {
                        return true;
                    }
                }
                // A *contravariant* position is the other way round: the
                // candidate declares the supertype and the wanted type is the
                // subtype. slick's
                // `constColumnShape[T]: Shape[L, ConstColumn[T], T, ConstColumn[T]]`
                // has to answer a wanted `Shape[FlatShapeLevel, LiteralColumn[Boolean], ?, ?BP]`
                // (`Mixed_` is `-`), and `T` is only reachable by seeing the
                // wanted `LiteralColumn[Boolean]` as a `ConstColumn`.
                match self.typer.base_type_instance(b, *s1, 0) {
                    Some(Type::Class { args, .. }) if args.len() == a1.len() => a1
                        .iter()
                        .zip(args.iter())
                        .all(|(x, y)| self.unify_at(x, y, depth + 1)),
                    _ => false,
                }
            }
            (Type::Tuple(t1), Type::Tuple(t2)) if t1.len() == t2.len() => t1
                .iter()
                .zip(t2.iter())
                .all(|(x, y)| self.unify_at(x, y, depth + 1)),
            (Type::Tuple(ts), Type::Class { args, .. })
            | (Type::Class { args, .. }, Type::Tuple(ts))
                if ts.len() == args.len() =>
            {
                let (l, r): (&Vec<Type>, &Vec<Type>) = match a {
                    Type::Tuple(_) => (ts, args),
                    _ => (args, ts),
                };
                l.iter()
                    .zip(r.iter())
                    .all(|(x, y)| self.unify_at(x, y, depth + 1))
            }
            (Type::Array(x), Type::Array(y))
            | (Type::ByName(x), Type::ByName(y))
            | (Type::Repeated(x), Type::Repeated(y)) => self.unify_at(x, y, depth + 1),
            (
                Type::Function {
                    params: p1,
                    ret: r1,
                },
                Type::Function {
                    params: p2,
                    ret: r2,
                },
            ) if p1.len() == p2.len() => {
                p1.iter()
                    .zip(p2.iter())
                    .all(|(x, y)| self.unify_at(x, y, depth + 1))
                    && self.unify_at(r1, r2, depth + 1)
            }
            (Type::Applied { ctor: c1, args: a1 }, Type::Applied { ctor: c2, args: a2 })
                if a1.len() == a2.len() =>
            {
                self.unify_at(c1, c2, depth + 1)
                    && a1
                        .iter()
                        .zip(a2.iter())
                        .all(|(x, y)| self.unify_at(x, y, depth + 1))
            }
            // Two refinements, matched parent by parent and declaration by
            // name. cats names a type constructor member this way --
            // `type Aux[M[_], F0[_]] = Parallel[M] { type F[x] = F0[x] }` --
            // and fitting an in-scope `Parallel.Aux[M, F]` to a wanted
            // `Parallel.Aux[M, ?F]` is the only way `?F` is ever solved.
            // Structural equality answered this while a member's right-hand
            // side was the same placeholder symbol whatever `F0` was; now that
            // the lambda carries what it captured, the arguments have to be
            // matched.
            (
                Type::Refined {
                    parents: p1,
                    decls: d1,
                },
                Type::Refined {
                    parents: p2,
                    decls: d2,
                },
            ) if p1.len() == p2.len() && d1.len() == d2.len() => {
                let (p1, p2) = (p1.clone(), p2.clone());
                let (d1, d2) = (d1.clone(), d2.clone());
                p1.iter()
                    .zip(p2.iter())
                    .all(|(x, y)| self.unify_at(x, y, depth + 1))
                    && d1.iter().all(|x| {
                        match d2
                            .iter()
                            .find(|y| refine_decl_name(y) == refine_decl_name(x))
                        {
                            Some(y) => self.unify_refine_decl(x, y, depth + 1),
                            None => false,
                        }
                    })
            }
            _ => a == b,
        }
    }

    /// Two refinement declarations of the same name, payload by payload.
    fn unify_refine_decl(&mut self, a: &RefineDecl, b: &RefineDecl, depth: usize) -> bool {
        let opt = |s: &mut Self, x: &Option<Type>, y: &Option<Type>| match (x, y) {
            (None, None) => true,
            (Some(x), Some(y)) => s.unify_at(x, y, depth),
            _ => false,
        };
        match (a, b) {
            (
                RefineDecl::Type {
                    rhs: r1,
                    tparams: t1,
                    lo: lo1,
                    hi: hi1,
                    ..
                },
                RefineDecl::Type {
                    rhs: r2,
                    tparams: t2,
                    lo: lo2,
                    hi: hi2,
                    ..
                },
            ) => t1 == t2 && opt(self, r1, r2) && opt(self, lo1, lo2) && opt(self, hi1, hi2),
            (
                RefineDecl::Def {
                    paramss: p1,
                    ret: r1,
                    ..
                },
                RefineDecl::Def {
                    paramss: p2,
                    ret: r2,
                    ..
                },
            ) => {
                p1.len() == p2.len()
                    && p1.iter().zip(p2.iter()).all(|(x, y)| {
                        x.len() == y.len()
                            && x.iter()
                                .zip(y.iter())
                                .all(|(x, y)| self.unify_at(x, y, depth))
                    })
                    && self.unify_at(r1, r2, depth)
            }
            (RefineDecl::Val { ty: t1, .. }, RefineDecl::Val { ty: t2, .. }) => {
                self.unify_at(t1, t2, depth)
            }
            _ => false,
        }
    }
}

fn refine_decl_name(d: &RefineDecl) -> &str {
    match d {
        RefineDecl::Type { name, .. }
        | RefineDecl::Def { name, .. }
        | RefineDecl::Val { name, .. } => name,
    }
}

fn strip_annot(ty: &Type) -> &Type {
    match ty {
        Type::Annotated { tpe, .. } => strip_annot(tpe),
        t => t,
    }
}

fn mentions_unknown(ty: &Type, unknowns: &rustc_hash::FxHashSet<u32>) -> bool {
    match ty {
        Type::TypeParam(id) => unknowns.contains(&id.0),
        Type::Class { args, .. } | Type::Named { args, .. } | Type::Tuple(args) => {
            args.iter().any(|t| mentions_unknown(t, unknowns))
        }
        Type::Applied { ctor, args } => {
            mentions_unknown(ctor, unknowns) || args.iter().any(|t| mentions_unknown(t, unknowns))
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            mentions_unknown(t, unknowns)
        }
        Type::Function { params, ret } => {
            params.iter().any(|t| mentions_unknown(t, unknowns)) || mentions_unknown(ret, unknowns)
        }
        Type::Refined { parents, .. } => parents.iter().any(|t| mentions_unknown(t, unknowns)),
        _ => false,
    }
}

/// `CC` solved to `Vector` leaves the result as `Applied { Vector, [Int] }`;
/// nothing downstream prints or erases that as `Vector[Int]`. Collapse it.
fn fold_applied(ty: &Type) -> Type {
    match ty {
        Type::Applied { ctor, args } => crate::symbol::apply_type_ctor(
            fold_applied(ctor),
            args.iter().map(fold_applied).collect(),
        ),
        Type::Class { sym, args } => Type::Class {
            sym: *sym,
            args: args.iter().map(fold_applied).collect(),
        },
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(fold_applied).collect()),
        Type::Array(t) => Type::Array(Box::new(fold_applied(t))),
        Type::Refined { parents, decls } => Type::Refined {
            parents: parents.iter().map(fold_applied).collect(),
            decls: decls.clone(),
        },
        other => other.clone(),
    }
}

fn unify_conv_tparam(tp: SymbolId, param: &Type, from: &Type) -> Option<Type> {
    match (param, from) {
        (Type::TypeParam(id), actual) if *id == tp => Some(actual.widen_constant()),
        (Type::Array(p), Type::Array(a)) => unify_conv_tparam(tp, p, a),
        (Type::Class { args: pa, .. }, Type::Class { args: fa, .. }) => pa
            .iter()
            .zip(fa.iter())
            .find_map(|(p, f)| unify_conv_tparam(tp, p, f)),
        // The parameter is an application of a higher-kinded parameter:
        // `implicit def toFlatMapOps[F[_], A](fa: F[A])`. Solving `F` from a
        // receiver `Box[Int]` means taking the receiver's type *constructor*,
        // not one of its arguments; solving `A` means matching argument for
        // argument, whether the receiver is a class application (`Box[Int]`)
        // or another higher-kinded one (`G[Int]` inside `def go[G[_]]`).
        // Without this `F` fell through to `AnyRef` and cats' whole syntax
        // layer resolved to `FlatMap[AnyRef]`.
        (Type::Applied { ctor, args: pa }, actual) => {
            if matches!(**ctor, Type::TypeParam(id) if id == tp) {
                return type_ctor_of(actual);
            }
            let fa = applied_args(actual)?;
            pa.iter()
                .zip(fa.iter())
                .find_map(|(p, f)| unify_conv_tparam(tp, p, f))
        }
        _ => None,
    }
}

/// An applied type split into its constructor and arguments:
/// `List[String]` -> (`List`, `[String]`). A type that is not an application
/// is not one, so a higher-kinded unknown never binds to a proper type.
fn as_application(ty: &Type) -> Option<(Type, &[Type])> {
    match ty {
        Type::Class { sym, args } if !args.is_empty() => Some((
            Type::Class {
                sym: *sym,
                args: Vec::new(),
            },
            args,
        )),
        Type::Applied { ctor, args } if !args.is_empty() => Some(((**ctor).clone(), args)),
        _ => None,
    }
}

/// The type constructor of an applied type: `Box[Int]` -> `Box`, `G[Int]` -> `G`.
fn type_ctor_of(ty: &Type) -> Option<Type> {
    match ty {
        Type::Class { sym, args } if !args.is_empty() => Some(Type::Class {
            sym: *sym,
            args: Vec::new(),
        }),
        Type::Applied { ctor, .. } => Some((**ctor).clone()),
        _ => None,
    }
}

fn applied_args(ty: &Type) -> Option<&[Type]> {
    match ty {
        Type::Class { args, .. } | Type::Applied { args, .. } => Some(args),
        _ => None,
    }
}
