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

use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_span::Span;

use crate::check::Typer;
use crate::symbol::SymKind;

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
        let mut seen = std::collections::HashSet::new();
        for sc in self.st.scopes.iter().rev() {
            for name in sc.names() {
                for id in sc.lookup(name) {
                    if self.st.get(*id).flags.contains(Flags::IMPLICIT) && seen.insert(id.0) {
                        out.push(*id);
                    }
                }
            }
        }
        if !self.st.this_class.is_none() {
            // Instance implicits on this class/module, walking parents (nsc
            // linearization is not reproduced; inheritance is).
            let mut work = vec![self.st.this_class];
            let mut walked = std::collections::HashSet::new();
            while let Some(id) = work.pop() {
                if id.is_none() || !walked.insert(id.0) {
                    continue;
                }
                for m in self.st.get(id).members.clone() {
                    if self.st.get(m).flags.contains(Flags::IMPLICIT) && seen.insert(m.0) {
                        out.push(m);
                    }
                }
                for p in self.st.get(id).parents.clone() {
                    if let Some(ps) = self.st.class_sym_of(&p) {
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
                    for m in o.members.clone() {
                        if self.st.get(m).flags.contains(Flags::IMPLICIT) && seen.insert(m.0) {
                            out.push(m);
                        }
                        if self.st.get(m).name == "package" {
                            let mcls = self.st.module_class_of(m);
                            for mem in self.st.get(mcls).members.clone() {
                                if self.st.get(mem).flags.contains(Flags::IMPLICIT)
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
        out
    }

    /// Implicit members of the companion module of `class_id` (or the module
    /// class itself when `class_id` is already a module / module class).
    fn companion_implicits_of_class(&self, class_id: SymbolId) -> Vec<SymbolId> {
        let mut out = Vec::new();
        if class_id.is_none() {
            return out;
        }
        let mcls = match self.st.get(class_id).kind {
            SymKind::Module => self.st.module_class_of(class_id),
            SymKind::ModuleClass => class_id,
            _ => {
                let Some(module) = self.st.companion_module(class_id) else {
                    return out;
                };
                self.st.module_class_of(module)
            }
        };
        for mem in &self.st.get(mcls).members {
            if self.st.get(*mem).flags.contains(Flags::IMPLICIT) {
                out.push(*mem);
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
        seen: &mut std::collections::HashSet<u32>,
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
        seen: &mut std::collections::HashSet<u32>,
    ) {
        if id.is_none() || !seen.insert(id.0) {
            return;
        }
        out.push(id);
        // SLS 7.2: the implicit scope of `T` also holds the companions of `T`'s
        // base classes. `=:=` has no companion object of its own, so its only
        // witness (`<:<.refl`) is reachable only through the `<:<` it extends.
        for p in self.st.get(id).parents.clone() {
            if let Some(ps) = self.st.class_sym_of(&p) {
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

    fn companion_implicits(&self, ty: &Type) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut parts = Vec::new();
        self.collect_type_parts(ty, &mut parts, &mut std::collections::HashSet::new());
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
        let s = self.st.get(id);
        if !s.flags.contains(Flags::IMPLICIT) {
            return None;
        }
        match &s.ty {
            Type::Method { paramss, ret } => {
                let ret = ret.clone();
                if paramss.iter().all(|c| c.is_empty()) {
                    return self.implicit_solve(id, &ret, pt, undet);
                }
                // A derivation rule: usable when its own implicits resolve.
                if depth >= MAX_IMPLICIT_DEPTH || !self.only_implicit_clauses(id) {
                    return None;
                }
                let paramss = paramss.clone();
                let fit = self.implicit_solve(id, &ret, pt, undet)?;
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
                });
                self.open_implicits.borrow_mut().pop();
                ok.then_some(fit)
            }
            Type::Function { params, ret } if params.is_empty() => {
                let ret = (**ret).clone();
                self.implicit_solve(id, &ret, pt, undet)
            }
            t => {
                let t = t.clone();
                self.implicit_solve(id, &t, pt, undet)
            }
        }
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
        let mut u = Unify::new(self, tps.iter().copied().chain(undet.iter().copied()));
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
                Some(t) => targs.push(t),
                // Not pinned down by the result type; the one-sided guess is
                // the last chance before the candidate is dropped.
                None => targs.push(crate::check::unify_one(*tp, ret, pt)?),
            }
        }
        let undet_out: Vec<(SymbolId, Type)> = undet
            .iter()
            .filter_map(|d| u.solved(*d).map(|t| (*d, t)))
            .collect();
        let inst = crate::symbol::subst_tparams_slice(&tps, &targs, ret);
        let want = self.subst_undet(pt, &undet_out);
        self.implicit_result_conforms(&inst, &want)
            .then_some(ImplicitFit {
                targs,
                undet: undet_out,
            })
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

    fn conversion_provides(&self, id: SymbolId, from: &Type, to: &Type) -> bool {
        let s = self.st.get(id);
        if !s.flags.contains(Flags::IMPLICIT) {
            return false;
        }
        match &s.ty {
            Type::Method { paramss, ret } => {
                let ps = paramss.first().cloned().unwrap_or_default();
                if ps.len() != 1 {
                    return false;
                }
                self.st.is_sub_type(from, &ps[0]) && self.st.is_sub_type(ret, to)
            }
            Type::Function { params, ret } if params.len() == 1 => {
                self.st.is_sub_type(from, &params[0]) && self.st.is_sub_type(ret, to)
            }
            _ => false,
        }
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

    /// nsc-style: `a` is as specific as `b` when `a`'s result type is a subtype
    /// of `b`'s, and (for conversions) `a`'s argument type is a subtype of `b`'s,
    /// **or** `a`'s defining class is a subclass of `b`'s (origin).
    /// Type and origin can disagree (inherited more-specific vs local less-specific)
    /// and then `most_specific` reports ambiguous, matching nsc.
    fn is_as_specific(&self, a: SymbolId, b: SymbolId) -> bool {
        self.is_as_specific_type(a, b) || self.is_as_specific_origin(a, b)
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

    fn strictly_more_specific(&self, a: SymbolId, b: SymbolId) -> bool {
        a != b && self.is_as_specific(a, b) && !self.is_as_specific(b, a)
    }

    fn most_specific(&self, cands: Vec<SymbolId>) -> ImplicitSearch {
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

    /// The result type with the candidate's own type parameters erased to
    /// wildcards, nsc's `isAsSpecific` on a `PolyType`: `Show[Int]` conforms to
    /// `implicit def anyShow[A]: Show[A]`'s `Show[_]` but not the other way
    /// round, so the monomorphic instance wins.
    fn implicit_result_ty(&self, id: SymbolId) -> Type {
        let ret = match &self.st.get(id).ty {
            Type::Method { ret, .. } => (**ret).clone(),
            Type::Function { ret, .. } => (**ret).clone(),
            t => t.clone(),
        };
        self.erase_method_tparams(id, &ret)
    }

    fn conversion_arg_ty(&self, id: SymbolId) -> Option<Type> {
        match &self.st.get(id).ty {
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
            let members = self.st.lookup_member(cls, name);
            if let Some(m) = members.first() {
                hits.push((id, *m, to));
            }
        }
        hits.sort_by_key(|(c, m, _)| (c.0, m.0));
        hits.dedup_by_key(|(c, m, _)| (c.0, m.0));
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
                let pool = if declared.is_empty() { hits } else { declared };
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
        let s = self.st.get(id);
        if !s.flags.contains(Flags::IMPLICIT) {
            return None;
        }
        match &s.ty {
            Type::Method { paramss, ret } => {
                let ps = paramss.first().cloned().unwrap_or_default();
                if ps.len() != 1 {
                    return None;
                }
                if self.conv_param_matches(id, from, &ps[0]) {
                    Some(self.instantiate_conv_type(id, from, &ps[0], (**ret).clone()))
                } else {
                    None
                }
            }
            Type::Function { params, ret } if params.len() == 1 => {
                if self.conv_param_matches(id, from, &params[0]) {
                    Some(self.instantiate_conv_type(id, from, &params[0], (**ret).clone()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn instantiate_conv_type(&self, id: SymbolId, from: &Type, param: &Type, ty: Type) -> Type {
        let tps = self.st.get(id).tparams.clone();
        if tps.is_empty() {
            return ty;
        }
        let args_t: Vec<Type> = tps
            .iter()
            .map(|tp| unify_conv_tparam(*tp, param, from).unwrap_or(Type::AnyRef))
            .collect();
        crate::symbol::subst_tparams_slice(&tps, &args_t, &ty)
    }

    fn conv_param_matches(&self, id: SymbolId, from: &Type, param: &Type) -> bool {
        let param = self.erase_method_tparams(id, param);
        self.st.is_sub_type(from, &param) || matches!(param, Type::Any | Type::Wildcard)
    }

    pub(crate) fn ref_implicit(&self, id: SymbolId, span: Span) -> Tree {
        let s = self.st.get(id);
        let ty = match &s.ty {
            Type::Method { paramss, ret }
                if paramss.is_empty() || paramss.iter().all(|c| c.is_empty()) =>
            {
                (**ret).clone()
            }
            t => t.clone(),
        };
        Tree {
            id: scala_rs_parser::NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: s.name.clone(),
            },
            ty,
            sym: id,
            postfix: false,
        }
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
    unknowns: std::collections::HashSet<u32>,
    bound: std::collections::HashMap<u32, Type>,
}

impl<'a> Unify<'a> {
    fn new(typer: &'a Typer, unknowns: impl IntoIterator<Item = SymbolId>) -> Self {
        Unify {
            typer,
            unknowns: unknowns.into_iter().map(|s| s.0).collect(),
            bound: std::collections::HashMap::new(),
        }
    }

    fn unknown_of(&self, ty: &Type) -> Option<u32> {
        match ty {
            Type::TypeParam(id) if self.unknowns.contains(&id.0) => Some(id.0),
            _ => None,
        }
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
                let Some(base) = self.typer.base_type_instance(a, *s2, 0) else {
                    return false;
                };
                match &base {
                    Type::Class { args, .. } if args.len() == a2.len() => args
                        .iter()
                        .zip(a2.iter())
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
            _ => a == b,
        }
    }
}

fn strip_annot(ty: &Type) -> &Type {
    match ty {
        Type::Annotated { tpe, .. } => strip_annot(tpe),
        t => t,
    }
}

fn mentions_unknown(ty: &Type, unknowns: &std::collections::HashSet<u32>) -> bool {
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
        _ => false,
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
        _ => None,
    }
}
