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

#[derive(Debug, Clone)]
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

/// Memo for [`Typer::search_implicit_undet`], keyed by `(wanted type,
/// undetermined call-site parameters, depth)`.
///
/// Without it the search is exponential in the number of candidates: a
/// derivation rule's own implicit clauses are resolved by a fresh search, and
/// the same wanted type is reached again through every path that leads to it,
/// up to [`MAX_IMPLICIT_DEPTH`] times over. Admitting one more cake's worth of
/// implicits into scope took gitbucket's 213 hand-written sources from 14
/// seconds to over 13 minutes, with `sample` putting every one of 5591 samples
/// in a single `search_implicit_at` -> `implicit_fit_at` -> `search_implicit_at`
/// stack (`docs/gitbucket.md`).
///
/// **Why that key is enough.** The whole search runs under `&self`, so
/// `SymbolTable::scopes`, `this_class` and `parent_ctor_scope` -- everything
/// [`Typer::implicits_in_scope`] and [`Typer::companion_implicits`] read --
/// cannot change while it is in flight. The memo is therefore created when the
/// outermost search starts and thrown away when it returns, which makes the
/// context it was computed in a constant rather than part of the key. `depth`
/// rides on the entry instead of in the key: it only decides anything where
/// [`Typer::implicit_fit_at`] actually stopped offering derivation rules
/// because of [`MAX_IMPLICIT_DEPTH`], so an entry records whether that
/// happened and is reused at a shallower depth when it did not
/// (see `MemoEntry::depth`).
///
/// The one piece of mutable state a search *does* read is
/// [`Typer::open_implicits`], nsc's `openImplicits`, which
/// [`Typer::implicit_diverges`] consults to cut off a diverging expansion.
/// That stack is not in the key, so an entry is only usable where it cannot
/// matter: [`Self::probe`] records, as a 64-bit signature over `SymbolId`, the
/// candidates a subtree asked the divergence check about, and an entry is both
/// stored and reused only when that signature is disjoint from the signature
/// of the open stack in force. `implicit_diverges` fires only when the *same*
/// symbol is already open, so a disjoint signature means every divergence
/// decision inside the subtree was made from the subtree's own pushes, which
/// are replayed identically wherever the entry is reused. A signature
/// collision can only cost a cache miss.
///
/// The two other cells a search writes need separate treatment:
/// `diverged_implicit` keeps the *first* divergence and is
/// monotone within one top-level search. `implicit_via_module` can be
/// overwritten when two companions inherit the same member, so entries replay
/// every route their subtree wrote, including writes from failed candidates.
#[derive(Default)]
pub(crate) struct ImplicitMemo {
    /// Re-entrancy count of [`Typer::search_implicit_undet`]. The memo is
    /// alive while this is non-zero and cleared when it falls back to zero.
    depth: usize,
    /// [`Typer::implicits_in_scope`]'s answer for the scope the outermost
    /// search started in. Recomputing it walks every enclosing scope, every
    /// base class of `this` and the enclosing package object, once per node.
    in_scope: Option<std::rc::Rc<Vec<SymbolId>>>,
    entries: rustc_hash::FxHashMap<u64, Vec<MemoEntry>>,
    /// nsc's `improvesCache`: [`Typer::strictly_more_specific`] by candidate
    /// pair. [`Typer::most_specific`] compares every pair of the candidates
    /// that fitted, so it is quadratic in a scope's size, and it runs at every
    /// node of the derivation -- over the same pairs each time. Each answer is
    /// four `is_sub_type` calls over types rebuilt from the candidate's
    /// signature.
    improves: rustc_hash::FxHashMap<(u32, u32), bool>,
    /// Divergence-check signature of the subtree being evaluated right now.
    probe: u64,
    /// Whether that subtree was stopped anywhere by [`MAX_IMPLICIT_DEPTH`].
    cut: bool,
    /// Last companion-route writes made by the subtree currently evaluated.
    routes: rustc_hash::FxHashMap<u32, SymbolId>,
    /// The prefixes the wanted types of this search reached an inner class's
    /// companion through (module class -> `o1` of `o1.Inner`), read by
    /// [`Typer::implicit_candidate_ty`]: a conversion `object Inner {
    /// implicit def fromOther(b: Bridge): Inner }` found for an `o1.Inner`
    /// gives an `o1.Inner` (pos/t4947).
    companion_prefixes: rustc_hash::FxHashMap<u32, Vec<Type>>,
}

struct MemoEntry {
    pt: Type,
    undet: Vec<SymbolId>,
    probe: u64,
    /// The depth this was answered at, and whether the depth limit had a say.
    /// A subtree that never reached the limit gives the same answer at every
    /// *smaller* depth, because every decision in it is depth-independent
    /// until the limit bites; at a larger one it could be cut off sooner, so
    /// the entry does not travel upwards. Without this the same wanted type is
    /// re-derived once per depth it is reached at, up to eight times over.
    depth: usize,
    cut: bool,
    result: ImplicitSearch,
    bindings: Vec<(SymbolId, Type)>,
    routes: rustc_hash::FxHashMap<u32, SymbolId>,
}

/// One bit per `SymbolId`, folded modulo 64.
fn sym_bit(id: SymbolId) -> u64 {
    1u64 << (id.0 % 64)
}

/// Structural hash of a type, for the memo's bucket key only: an entry is
/// still compared with `PartialEq` before it is used, so a variant this misses
/// costs a lookup, never a wrong answer. `Type` cannot derive `Hash` because
/// `Lit` holds `f64`.
fn hash_type<H: std::hash::Hasher>(ty: &Type, h: &mut H) {
    use std::hash::Hash;
    std::mem::discriminant(ty).hash(h);
    let all = |ts: &[Type], h: &mut H| {
        ts.len().hash(h);
        for t in ts {
            hash_type(t, h);
        }
    };
    match ty {
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) | Type::Annotated { tpe: t, .. } => {
            hash_type(t, h)
        }
        Type::Tuple(ts) | Type::Overload(ts) => all(ts, h),
        Type::Function { params, ret } => {
            all(params, h);
            hash_type(ret, h);
        }
        Type::Named { name, args } => {
            name.hash(h);
            all(args, h);
        }
        Type::Class { sym, args } => {
            sym.0.hash(h);
            all(args, h);
        }
        Type::Applied { ctor, args } => {
            hash_type(ctor, h);
            all(args, h);
        }
        Type::Method { paramss, ret } => {
            paramss.len().hash(h);
            for c in paramss {
                all(c, h);
            }
            hash_type(ret, h);
        }
        Type::ModuleRef(s) | Type::TypeParam(s) | Type::TypeMember(s) | Type::ThisType(s) => {
            s.0.hash(h)
        }
        Type::SingleType { prefix, sym } => {
            hash_type(prefix, h);
            sym.0.hash(h);
        }
        Type::BoundedWildcard { lo, hi } => {
            for b in [lo, hi].into_iter().flatten() {
                hash_type(b, h);
            }
        }
        Type::Refined { parents, .. } => all(parents, h),
        // Constants and the nullary variants are settled by the discriminant
        // and the exact comparison that follows.
        _ => {}
    }
}

fn memo_key(pt: &Type, undet: &[SymbolId]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    hash_type(pt, &mut h);
    for u in undet {
        u.0.hash(&mut h);
    }
    h.finish()
}

/// Decrements the memo's re-entrancy count, and clears it when the outermost
/// search returns. A guard rather than a plain decrement because the callers
/// have several early returns.
struct MemoLive<'a> {
    typer: &'a Typer,
    _prefixes: crate::check_name::ImportPrefixLive<'a>,
}

impl Drop for MemoLive<'_> {
    fn drop(&mut self) {
        let mut m = self.typer.implicit_memo.borrow_mut();
        m.depth -= 1;
        if m.depth == 0 {
            m.entries.clear();
            m.improves.clear();
            m.in_scope = None;
            m.probe = 0;
            m.cut = false;
            m.routes.clear();
            m.companion_prefixes.clear();
        }
    }
}

/// Number of nodes in a type, nsc's `complexity`.
pub(crate) fn complexity(ty: &Type) -> usize {
    match ty {
        // An annotation does not hide the structural size of its underlying
        // type. Treating an annotated tail as one node falsely marks shrinking
        // recursive HList derivations as divergent.
        Type::Annotated { tpe, .. } => complexity(tpe),
        // An inner class behind a prefix (`prefix.rs`): nsc counts the
        // prefix as one node and the class's arguments as themselves.
        Type::Refined { .. } if crate::prefix::view_prefix(ty).is_some() => {
            1 + complexity(crate::prefix::strip_view(ty))
        }
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
        Type::Refined { .. } if crate::prefix::view_prefix(ty).is_some() => {
            head_sym(typer, crate::prefix::strip_view(ty))
        }
        _ => typer.st.class_sym_of(ty),
    }
}

/// nsc `Types#dominates`: same head symbol and no simpler than the open one.
pub(crate) fn dominates(typer: &Typer, new_pt: &Type, open_pt: &Type) -> bool {
    match (head_sym(typer, new_pt), head_sym(typer, open_pt)) {
        (Some(a), Some(b)) => a == b && complexity(new_pt) >= complexity(open_pt),
        // A bare type parameter target (`implicit def loop[A](implicit a: A): A`)
        // has no head symbol; fall back to plain complexity.
        _ => complexity(new_pt) >= complexity(open_pt),
    }
}

impl Typer {
    /// The candidates an unqualified reference at this point can reach.
    ///
    /// Answered from [`ImplicitMemo`] while a search is running: the walk
    /// below covers every enclosing scope, every base class of `this` and the
    /// enclosing package object, and it was being redone at every node of the
    /// derivation tree even though nothing it reads can change under `&self`.
    pub(crate) fn implicits_in_scope(&self) -> Vec<SymbolId> {
        let cached = {
            let m = self.implicit_memo.borrow();
            (m.depth > 0).then(|| m.in_scope.clone())
        };
        match cached {
            Some(Some(hit)) => return hit.as_ref().clone(),
            Some(None) => {
                let out = self.implicits_in_scope_uncached();
                self.implicit_memo.borrow_mut().in_scope = Some(std::rc::Rc::new(out.clone()));
                return out;
            }
            None => {}
        }
        self.implicits_in_scope_uncached()
    }

    fn implicits_in_scope_uncached(&self) -> Vec<SymbolId> {
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
                // Types and terms are separate namespaces (SLS 2): a *type*
                // named `F` hides no term `F`. cats' `new Parallel[IorT[F0, E,
                // *]] { type F[x] = IorT[F0, E, x]; … }` sits inside a method
                // whose `implicit F: Monad[F0]` is the `Applicative[F0]` the
                // body's `IorT.pure(a)` needs, and the anonymous class's type
                // member took it out of the search.
                let binds_term = ids.iter().any(|b| {
                    !matches!(
                        self.st.get(b.sym).kind,
                        crate::symbol::SymKind::Class
                            | crate::symbol::SymKind::TypeParam
                            | crate::symbol::SymKind::TypeMember
                    )
                });
                let hidden = if binds_term {
                    !shadowed_names.insert(name.as_str())
                } else {
                    shadowed_names.contains(name.as_str())
                };
                if hidden {
                    continue;
                }
                // Every binding of the name, whatever its SLS 2 precedence:
                // an implicit is found by searching the scope, not by being
                // written down, so a shadowed *name* still offers its
                // implicit. nsc's `ImplicitComputation` shadows by name only
                // across levels, which the `shadowed_names` set above is.
                for b in ids {
                    if self.st.get(b.sym).flags.contains(Flags::IMPLICIT) && seen.insert(b.sym.0) {
                        out.push(b.sym);
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
                // A `private` member of a *strict* ancestor is not inherited
                // (SLS 5.2), so it is not a candidate here at all -- the same
                // rule `SymbolTable::lookup_member` applies to a named
                // reference. gitbucket's `object helpers extends
                // AvatarImageProvider with LinkConverter with RequestCache`
                // reached `RequestCache`'s `private implicit def
                // context2Session` this way, and it then competed with the
                // `request2Session` nsc actually chooses.
                let inherited = id != self.st.this_class;
                for &m in &self.st.get(id).members {
                    // A local declaration shadows a same-named instance
                    // member too -- an unqualified reference to that name
                    // resolves to the local one, not `this.name`.
                    if self.st.get(m).flags.contains(Flags::IMPLICIT)
                        && !(inherited && self.st.private_to_owner(m))
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
    ///
    /// The self type's own linearization counts as well. A `self:` annotation
    /// is not a supertype, but its members *are* visible unqualified from
    /// inside the body -- that is what `SymbolTable::lookup_member` walks it
    /// for -- so an unqualified reference resolves through it and the same
    /// declaration/definition pair has to collapse there too. gitbucket
    /// writes its authenticators that way:
    ///
    /// ```scala
    /// trait ReferrerAuthenticator { self: ControllerBase & RepositoryService & AccountService =>
    ///   private def authenticate(...) = { val userName = params("owner") ... }
    /// }
    /// ```
    ///
    /// and every `params(...)` in one was `ambiguous implicit: request,
    /// request` while the identical line in a trait that *extends*
    /// `ScalatraFilter` type-checked -- the linearization the ranks were read
    /// from held neither `ScalatraContext` nor `DynamicScope`, so no candidate
    /// could shadow the other.
    ///
    /// An *enclosing* class's bases count for the same reason: inside `new
    /// Constraint() { … }` in a controller's body, `params("userName")` still
    /// reads the controller's `request`, and the anonymous class's own
    /// linearization holds neither declaration. Nearer scopes come first, so
    /// the ranks are laid out innermost class outwards -- which is also the
    /// order an unqualified name resolves in.
    fn shadow_inherited_implicits(&self, cands: Vec<SymbolId>) -> Vec<SymbolId> {
        if cands.is_empty() {
            return cands;
        }
        // During a super constructor call, neither this instance's implicit
        // members nor its ordinary declarations are in scope. Otherwise an
        // unavailable inherited declaration hides a usable outer implicit.
        let enclosing = self.this_owner(None);
        let start = if self.parent_ctor_scope
            && enclosing == self.st.this_class
            && !self.st.this_class.is_none()
        {
            self.st.get(self.st.this_class).owner
        } else {
            enclosing
        };
        let lin = self.unqualified_base_order(start);
        // A non-implicit override removes the inherited implicit too. Looking
        // only at implicit candidates left abstract `implicit def algebra`
        // visible behind an ordinary implementing val in an anonymous class.
        let declarations: Vec<SymbolId> = lin
            .iter()
            .flat_map(|id| self.st.get(*id).members.iter().copied())
            .collect();
        let receiver = self.st.self_type_of_class(self.st.this_class);
        let value_type = |id: SymbolId| {
            let ty = self.st.subst_as_seen_from(&receiver, &self.st.get(id).ty);
            match ty {
                Type::Method { paramss, ret } if paramss.is_empty() => *ret,
                other => other,
            }
        };
        let rank = |owner: SymbolId| lin.iter().position(|&b| b == owner);
        cands
            .iter()
            .copied()
            .filter(|&c| {
                let s = self.st.get(c);
                let Some(here) = rank(s.owner) else {
                    return true;
                };
                !declarations.iter().chain(cands.iter()).any(|&other| {
                    if other == c {
                        return false;
                    }
                    let o = self.st.get(other);
                    if o.name != s.name
                        || o.owner == s.owner
                        || self.st.private_to_owner(other)
                        || (value_type(other) != value_type(c)
                            // An inferred val already occupies the term name
                            // while its initializer is being typed. It hides a
                            // parameterless inherited method before its result
                            // type is available; override conformance is checked
                            // after inference.
                            && !(o.ty.is_no_type()
                                && o.kind == SymKind::Term
                                && matches!(&s.ty, Type::Method { paramss, .. } if paramss.is_empty())))
                    {
                        return false;
                    }
                    rank(o.owner).is_some_and(|there| there < here)
                })
            })
            .collect()
    }

    /// The classes an unqualified reference inside `cls`'s body can reach a
    /// member of, nearest first.
    ///
    /// `cls`'s own linearization, then its self type's (a `self:` annotation
    /// makes the annotated type's members visible from inside the body without
    /// being a supertype -- `SymbolTable::lookup_member` walks it for exactly
    /// that), then the same two for each enclosing class outwards. Duplicates
    /// are dropped at the position they were first reached, so a base shared
    /// by an inner and an outer class ranks where the inner one put it.
    fn unqualified_base_order(&self, cls: SymbolId) -> Vec<SymbolId> {
        let mut out: Vec<SymbolId> = Vec::new();
        let mut at = cls;
        let mut guard = 0;
        while !at.is_none() && guard < 64 {
            guard += 1;
            if self.st.get(at).is_class_like() {
                let mut here = vec![at];
                if let Some(sty) = &self.st.get(at).self_type {
                    // `self: A & B & C =>` is a refinement with three parents,
                    // and each contributes its own bases.
                    match sty {
                        Type::Refined { parents, .. } => here.extend(
                            parents
                                .iter()
                                .filter_map(|p| self.st.class_sym_of(p))
                                .collect::<Vec<_>>(),
                        ),
                        other => here.extend(self.st.class_sym_of(other)),
                    }
                }
                for start in here {
                    for b in crate::lin::linearize(&self.st, start) {
                        if !out.contains(&b) {
                            out.push(b);
                        }
                    }
                }
            }
            at = self.st.get(at).owner;
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
                        let mut memo = self.implicit_memo.borrow_mut();
                        if memo.depth > 0 {
                            memo.routes.insert(mem.0, module_sym);
                        }
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
                // SLS 7.2: the parts of `p.T` include the parts of `p.type`,
                // and those of `S#T` the parts of `S`. An inner class behind
                // a prefix (`prefix.rs`) carries exactly that prefix, so
                // `StrTypes.BCT[String]` sees the implicits `object StrTypes`
                // declares.
                if let Some(pre) = crate::prefix::view_prefix(ty) {
                    // The companion of an inner class reached through a
                    // path: its members are read as seen from that path
                    // (`implicit_candidate_ty`).
                    if self.st.is_singleton_prefix(pre) {
                        for p in parents {
                            let Type::Class { sym, .. } = p else { continue };
                            if !self.st.is_inner_class_of_class(*sym) {
                                continue;
                            }
                            let Some(m) = self.st.companion_module(*sym) else {
                                continue;
                            };
                            let mcls = self.st.module_class_of(m);
                            if let Ok(mut memo) = self.implicit_memo.try_borrow_mut() {
                                let v = memo.companion_prefixes.entry(mcls.0).or_default();
                                if !v.contains(pre) {
                                    v.push(pre.clone());
                                }
                            }
                        }
                    }
                    let pre = match pre {
                        Type::ThisType(c) if !c.is_none() => self.st.type_of_class(*c),
                        other => self.st.widen_prefix(other),
                    };
                    if !pre.is_no_type() {
                        self.collect_type_parts(&pre, out, seen);
                    }
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

    pub(crate) fn companion_implicits(&self, ty: &Type) -> Vec<SymbolId> {
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
    pub(crate) fn only_implicit_clauses(&self, id: SymbolId) -> bool {
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
        if owner.is_none() || !self.st.get(owner).is_class_like() {
            return None;
        }
        let prefix = self.term_import_prefix_for(owner).map(|q| q.ty.clone())?;
        if prefix.is_no_type() || prefix.is_error() {
            return None;
        }
        // Even a non-generic API can mention its outer profile's type
        // family. Read aliases through the actual imported API before
        // substituting the receiver's ordinary type parameters.
        let expanded = self.st.expand_in_type(&prefix, ty);
        // The companion of an inner class imported through a path (`import
        // o1.Inner.fromOther`): its members are read as seen from the path
        // (`Inner` is `o1.Inner`, pos/t4947).
        if let Type::SingleType { prefix: p, sym: m } = &prefix {
            if !m.is_none()
                && self.st.get(*m).kind == SymKind::Module
                && self.st.is_singleton_prefix(p)
            {
                let k = self.st.get(self.st.module_class_of(*m)).owner;
                if !k.is_none() && self.st.get(k).kind == SymKind::Class {
                    let mut recv = self.st.widen_prefix(p);
                    if self.st.class_sym_of(&recv).is_none() {
                        recv = self.st.type_of_class(k);
                    }
                    return Some(self.st.subst_as_seen_from_at(&recv, Some(p), &expanded));
                }
            }
        }
        Some(self.st.subst_as_seen_from(&prefix, &expanded))
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
        let origin = self
            .implicit_instance_origins
            .get(&id)
            .copied()
            .unwrap_or(id);
        if let Some(seen) = self.at_import_prefix_of(origin, ty) {
            return std::borrow::Cow::Owned(seen);
        }
        // A member of the companion of an inner class, reached through the
        // prefix the wanted type carries (`companion_prefixes`): `object
        // Inner { implicit def fromOther(b: Bridge): Inner }` found for an
        // `o1.Inner` is read as seen from `o1`, so its result is `o1.Inner`
        // and not `Outer.this.Inner` (pos/t4947). Reached through two
        // different paths in one search (`b: s.E` with `b: r.E`, whose
        // conversions come from both `r.E`'s and `s.E`'s companion,
        // pos/t5340), the member keeps no prefix: the class is read bare,
        // which conforms to either.
        if !owner.is_none() && self.st.get(owner).kind == SymKind::ModuleClass {
            let k = self.st.get(owner).owner;
            if !k.is_none() && self.st.get(k).kind == SymKind::Class {
                let pres = self
                    .implicit_memo
                    .try_borrow()
                    .ok()
                    .and_then(|m| m.companion_prefixes.get(&owner.0).cloned());
                if let Some(pres) = pres {
                    if pres.len() == 1 {
                        let mut recv = self.st.widen_prefix(&pres[0]);
                        if self.st.class_sym_of(&recv) != Some(k)
                            && !self
                                .st
                                .class_sym_of(&recv)
                                .is_some_and(|c| self.st.is_ancestor_of(k, c))
                        {
                            recv = self.st.type_of_class(k);
                        }
                        return std::borrow::Cow::Owned(self.st.subst_as_seen_from_at(
                            &recv,
                            Some(&pres[0]),
                            ty,
                        ));
                    }
                    return std::borrow::Cow::Owned(crate::prefix::bare_this_views(ty, k));
                }
            }
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
    /// Prepare detached search signatures. Each recursion depth gets distinct
    /// type-parameter symbols, including dependent bounds and higher-kinded
    /// parameters. They are not members of the declaring class and never replace
    /// the declaration selected for code generation. Divergence and memo masks
    /// continue to use the original declaration identity.
    pub(crate) fn prepare_implicit_instances(&mut self, id: SymbolId, depth_limit: usize) -> bool {
        let source = self.st.get(id).clone();
        if source.tparams.is_empty() || !self.only_implicit_clauses(id) {
            return false;
        }
        if self
            .implicit_instances
            .get(&id)
            .is_some_and(|(ty, instances)| *ty == source.ty && instances.len() > depth_limit)
        {
            return false;
        }
        let mut instances = self
            .implicit_instances
            .get(&id)
            .filter(|(ty, _)| *ty == source.ty)
            .map(|(_, ids)| ids.clone())
            .unwrap_or_default();
        for _ in instances.len()..=depth_limit {
            let instance = self.st.alloc(
                source.name.clone(),
                SymbolId::NONE,
                source.kind,
                source.flags,
                source.jvm_name.clone(),
            );
            let mut old = source.tparams.clone();
            let mut at = 0;
            while at < old.len() {
                old.extend(self.st.get(old[at]).tparams.clone());
                at += 1;
            }
            let mut fresh = Vec::new();
            for &tp in &old {
                let original = self.st.get(tp).clone();
                let new = self.st.alloc(
                    original.name.clone(),
                    SymbolId::NONE,
                    original.kind,
                    original.flags,
                    original.jvm_name.clone(),
                );
                fresh.push(new);
            }
            let args: Vec<Type> = fresh.iter().copied().map(Type::TypeParam).collect();
            for (&tp, &new) in old.iter().zip(&fresh) {
                let mut copy = self.st.get(tp).clone();
                copy.id = new;
                copy.owner = old
                    .iter()
                    .position(|p| *p == copy.owner)
                    .map(|i| fresh[i])
                    .unwrap_or(instance);
                copy.tparams = copy
                    .tparams
                    .iter()
                    .map(|p| fresh[old.iter().position(|x| x == p).unwrap()])
                    .collect();
                copy.ty = crate::symbol::subst_tparams_slice(&old, &args, &copy.ty);
                copy.bound_lo = copy
                    .bound_lo
                    .map(|t| crate::symbol::subst_tparams_slice(&old, &args, &t));
                copy.bound_hi = copy
                    .bound_hi
                    .map(|t| crate::symbol::subst_tparams_slice(&old, &args, &t));
                *self.st.get_mut(new) = copy;
            }
            let mut copy = source.clone();
            copy.id = instance;
            copy.tparams = fresh[..source.tparams.len()].to_vec();
            copy.ty = crate::symbol::subst_tparams_slice(&old, &args, &source.ty);
            *self.st.get_mut(instance) = copy;
            self.implicit_instance_origins.insert(instance, id);
            instances.push(instance);
        }
        self.implicit_instances.insert(id, (source.ty, instances));
        true
    }

    pub(crate) fn implicit_fit_at(
        &self,
        id: SymbolId,
        pt: &Type,
        depth: usize,
        undet: &[SymbolId],
    ) -> Option<ImplicitFit> {
        let id = self
            .implicit_instances
            .get(&id)
            .and_then(|(_, instances)| instances.get(depth))
            .copied()
            .unwrap_or(id);
        if !self.st.get(id).flags.contains(Flags::IMPLICIT) {
            return None;
        }
        let cand_ty = self.implicit_candidate_ty(id);
        // The cheap structural rejection, before anything is unified.
        // `cand_res` is exactly the type the arms below hand to
        // [`Self::implicit_solve`] / [`Self::implicit_fit_open`], so what
        // [`Self::plausibly_inhabits`] is asked about is what would have been
        // fitted.
        let cand_res: &Type = match &*cand_ty {
            Type::Method { ret, .. } => ret,
            Type::Function { params, ret } if params.is_empty() => ret,
            t => t,
        };
        if !self.plausibly_inhabits(cand_res, pt) {
            return None;
        }
        // The mirror of the erroneous *wanted* type in
        // [`Self::search_implicit_undet`]: nsc's `ImplicitComputation.survives`
        // requires `!isCyclicOrErroneous` of the candidate too, and for the
        // same reason -- `plausibly_inhabits` lets `Type::Error` stand for
        // anything, so a candidate whose own type failed to resolve is
        // applicable everywhere. Inside gitbucket's `extractFromJsonBody`, its
        // own `mf: Manifest[A]` parameter was such a candidate, and three
        // conversions in that method body came out `ambiguous implicit: mf,
        // request` / `mf, jsonFormats`.
        if crate::check::type_is_erroneous(cand_res) {
            return None;
        }
        match &*cand_ty {
            Type::Method { paramss, ret } => {
                if paramss.iter().all(|c| c.is_empty()) {
                    return self.implicit_solve(id, ret, pt, undet);
                }
                // A derivation rule: usable when its own implicits resolve.
                let origin = self
                    .implicit_instance_origins
                    .get(&id)
                    .copied()
                    .unwrap_or(id);
                let shrinking = self
                    .open_implicits
                    .borrow()
                    .iter()
                    .rev()
                    .find(|(prior, _)| {
                        self.implicit_instance_origins
                            .get(prior)
                            .copied()
                            .unwrap_or(*prior)
                            == origin
                    })
                    .is_some_and(|(_, previous)| complexity(pt) < complexity(previous));
                if depth >= MAX_IMPLICIT_DEPTH && !shrinking {
                    // The enclosing memo entry was decided by the depth limit,
                    // so it does not travel to a shallower search.
                    self.implicit_memo.borrow_mut().cut = true;
                    return None;
                }
                if !self.only_implicit_clauses(id) {
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
                        || self.built_not_found(&want, depth + 1)
                });
                self.open_implicits.borrow_mut().pop();
                if ok {
                    return Some(fit);
                }
                // The result type did not really determine this candidate: one
                // of its own parameters came back standing for a *call site*
                // parameter the search still has to solve, so the clause search
                // above asked for a type with a free parameter in it and could
                // not have answered. `tableShape[Level, T, C <: AbstractTable[_]]
                // (implicit ev: C <:< AbstractTable[T]): Shape[Level, C, T, C]`
                // against `Shape[_ <: FlatShapeLevel, Accounts, ?T, ?G]` is the
                // case: `C` and `?G` are `Accounts`, and the candidate's `T`
                // unifies with the call's `?T` -- neither of them known. Only
                // `ev` says what it is, which is what
                // [`Self::implicit_fit_open`] is for.
                if fit.targs.iter().any(|t| {
                    undet
                        .iter()
                        .any(|d| crate::check::type_mentions_tparam(t, *d))
                }) {
                    return self.implicit_fit_open(id, ret, pt, undet, paramss, depth);
                }
                None
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
    fn built_not_found(&self, want: &Type, depth: usize) -> bool {
        if self.manifest_available(want, depth) {
            return true;
        }
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
        let id = self
            .implicit_instance_origins
            .get(&id)
            .copied()
            .unwrap_or(id);
        // Whatever this answers is an answer about `id` against the open
        // stack, so the enclosing memo entry is only reusable where `id` is
        // not open. See [`ImplicitMemo`].
        self.implicit_memo.borrow_mut().probe |= sym_bit(id);
        let open = self.open_implicits.borrow();
        let hit = open.iter().any(|(sid, spt)| {
            self.implicit_instance_origins
                .get(sid)
                .copied()
                .unwrap_or(*sid)
                == id
                && dominates(self, pt, spt)
        });
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
            args.push(crate::check::unify_one(&self.st, *tp, ret, pt)?);
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
                None => targs.push(crate::check::unify_one(&self.st, *tp, ret, pt)?),
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
    /// The call site does **not** have to have left anything undetermined for
    /// this to be the answer. slick's
    ///
    /// ```text
    /// tuple2Shape[Level, M1, M2, U1, U2, P1, P2](implicit
    ///   u1: Shape[_ <: Level, M1, U1, P1], u2: Shape[_ <: Level, M2, U2, P2]
    /// ): Shape[Level, (M1, M2), (U1, U2), (P1, P2)]
    /// ```
    ///
    /// answered against `Shape[_ <: FlatShapeLevel, T, U, _]` -- the clause of
    /// `anyToShapedValue`, which is behind every `def * = (a, b).mapTo[M]` in a
    /// table -- has `P1`/`P2` standing opposite that trailing `_`. Nothing on
    /// the wanted side can ever say what they are, and `u1`/`u2` say it
    /// exactly. Requiring a non-empty `undet` made this whole family
    /// "could not find implicit value".
    ///
    /// Deliberately a *fallback*: it runs only for a candidate the ordinary
    /// solve rejected, and only when the wanted type pinned down at least one
    /// of the candidate's parameters. A rule that matched with everything open
    /// would be tried against every implicit in scope.
    ///
    /// **Not** a general instantiation: `Unify` keys its unknowns by symbol,
    /// so a rule that derives *itself* has its own `P1` and the caller's open
    /// `P1` as one symbol and the occurs check rejects `P1 := (P1, P2)`. nsc
    /// uses a fresh type variable per application; nested tuple shapes are
    /// still not found here.
    fn implicit_fit_open(
        &self,
        id: SymbolId,
        ret: &Type,
        pt: &Type,
        undet: &[SymbolId],
        paramss: &[Vec<Type>],
        depth: usize,
    ) -> Option<ImplicitFit> {
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
        // A parameter whose only "solution" is a wildcard has not been pinned
        // down by anything: `Unify::unify_at` answers `_` with `true` without
        // recording a constraint, so `Level := _ <: FlatShapeLevel` says no
        // more about `tuple22Shape` than leaving `Level` open does. It is
        // still *used* as the solution below -- only the "did the wanted type
        // say anything at all about this candidate" test discounts it.
        let mut pinned = 0usize;
        for tp in &tps {
            match u.solved(*tp) {
                Some(t) => {
                    if !matches!(t, Type::Wildcard | Type::BoundedWildcard { .. }) {
                        pinned += 1;
                    }
                    targs.push(self.simplify_solved(&t))
                }
                None => {
                    open.push(*tp);
                    targs.push(Type::TypeParam(*tp));
                }
            }
        }
        // Nothing left open: the ordinary solve already had its say, and
        // failed on conformance or bounds. Nothing *pinned*: the wanted type
        // says nothing about this candidate at all -- either every parameter
        // is open, or the only ones it settled it settled against a `_`.
        //
        // That second half is what keeps this fallback to the job its
        // documentation claims. slick's `tupleNShape` rules are
        // `Shape[Level, (M1, …, Mn), (U1, …, Un), (P1, …, Pn)]`, and a wanted
        // `Shape[_ <: L, ?M, ?U, ?P]` -- which is what every level of this
        // search below the first is asking for -- binds `Level` to the
        // wildcard and nothing else. Counted as pinned, all 22 of them then
        // search all of their own `Shape` clauses, at every level down to
        // `MAX_IMPLICIT_DEPTH`, and each clause is another wanted
        // `Shape[_ <: L', ?M', ?U', ?P']` that can never be answered. That
        // tree, not the fitting of any one candidate, is what made the
        // `import_wildcard` `pickle_readable` guard unaffordable.
        if open.is_empty() || pinned == 0 {
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
                // `Unify` binds the side it reaches first, and the candidate's
                // result is that side: matching `Shape[Level, C, T, C]` against
                // `Shape[_ <: FlatShapeLevel, Accounts, ?T, ?G]` records
                // `T := ?T`, not the reverse, so `?T` has no solution of its
                // own. Whatever the clauses have just said the candidate's `T`
                // is, `?T` is.
                let t = match u.solved_open(*d) {
                    Some(t) => t,
                    None => Type::TypeParam(u.alias_of(*d)?),
                };
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

    /// nsc's `isPlausiblyCompatible`: a purely structural test on a
    /// candidate's *declared* result type that rejects most of the implicit
    /// scope before [`Unify`] is asked anything.
    ///
    /// **What it can say "no" to, and why the "no" is sound.** Every path out
    /// of [`Self::implicit_fit_at`] that returns `Some` ends in
    /// [`Self::implicit_result_conforms(inst, want)`], where `inst` is `have`
    /// with the candidate's type parameters substituted (by
    /// `subst_tparams_slice`, then `simplify_solved`) and `want` is `pt` with
    /// the call site's undetermined ones substituted. Substitution replaces
    /// `Type::TypeParam` *leaves*; `fold_applied` and `collapse_refinements`
    /// rebuild a `Type::Class` under its own symbol. So when `have` and `pt`
    /// are both `Type::Class`, **the two head symbols are the ones `inst` and
    /// `want` will still have**, whatever the search goes on to solve.
    ///
    /// `implicit_result_conforms` compares two classes with different symbols
    /// by `SymbolTable::is_sub_type`, whose own answer for that pair is
    /// already decided by exactly the test below (the `class_reaches` fast
    /// rejection). This therefore rejects a candidate only where the
    /// conformance check at the end of the fit is *already* going to answer
    /// `false` -- it can lose no witness, and it changes no diagnostic.
    ///
    /// **And it needs no pickle.** `class_reaches` walks parent symbols with a
    /// visited set and never substitutes, so it asks the symbol table only
    /// what `is_sub_type` was going to ask it anyway; a hierarchy it cannot
    /// read as plain classes answers `None`, which is "cannot say" and lets
    /// the candidate through. The expensive step this replaces is
    /// `Unify::unify_at`'s pair of `base_type_instance` walks, which build a
    /// substituted base type on *both* sides before they can fail -- and,
    /// worse, a candidate that survives them recurses into
    /// [`Self::implicit_fit_open`] and searches for its own clauses.
    fn plausibly_inhabits(&self, have: &Type, pt: &Type) -> bool {
        let (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, .. }) = (have, pt) else {
            return true;
        };
        s1 == s2
            || self.st.is_function_class_shape(*s1, a1)
            || self.st.class_reaches(*s1, *s2) != Some(false)
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
        // An inner class behind a prefix (`prefix.rs`): the function it
        // extends is read through the view (DeliteDSL's `T <~< P`).
        if crate::prefix::view_prefix(ty).is_some() {
            let f = self
                .st
                .base_type_seq(ty)
                .into_iter()
                .find_map(|b| match &b {
                    Type::Function { .. } => Some(b.clone()),
                    Type::Class { sym, args } => self.st.function_class_shape(*sym, args),
                    _ => None,
                });
            return match f {
                Some(Type::Function { params, ret }) if params.len() == 1 => {
                    Some((params[0].clone(), (*ret).clone()))
                }
                _ => None,
            };
        }
        match ty {
            // A by-name parameter takes the value it delays: scalatra's
            // `implicit def booleanBlock2RouteMatcher(block: => Boolean):
            // RouteMatcher` views a `Boolean` (`get(!allowAnonymous) { … }`).
            // Applying it wraps the argument in its thunk like any call.
            Type::Method { paramss, ret } => {
                let ps = paramss.first()?;
                (ps.len() == 1).then(|| {
                    let p = match &ps[0] {
                        Type::ByName(t) => (**t).clone(),
                        t => t.clone(),
                    };
                    (p, (**ret).clone())
                })
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
        // A conversion whose own parameter would have to break its declared
        // bound is not applicable: `wrapRefArray[T <: AnyRef]` is no candidate
        // for an `Array[Char]` (`scala/io/Source.scala`'s `fromIterable(Array(c))`).
        if !self.conv_targs_within_bounds(id, &param, &targs) {
            return false;
        }
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
            ImplicitSearch::None if hits.is_empty() => {
                // Identity and array wrapping are also view witnesses. Use
                // the same ordered candidates as adaptation, then verify the
                // fully instantiated target before committing any bindings.
                let mut views = vec![from.clone()];
                if let Type::Array(elem) = from {
                    views.extend(
                        self.array_wrap_candidates(elem)
                            .into_iter()
                            .map(|(_, ty)| ty),
                    );
                }
                for view in views {
                    let mut u = Unify::new(self, std::iter::empty(), undet.iter().copied());
                    if !u.unify(&view, ret) {
                        continue;
                    }
                    let sol: Vec<_> = undet
                        .iter()
                        .filter_map(|d| u.solved(*d).map(|t| (*d, t)))
                        .collect();
                    let ids: Vec<_> = sol.iter().map(|(id, _)| *id).collect();
                    let ts: Vec<_> = sol.iter().map(|(_, ty)| ty.clone()).collect();
                    let to = crate::symbol::subst_tparams_slice(&ids, &ts, ret);
                    if sol.len() == undet.len() && self.st.is_sub_type(&view, &to) {
                        return Some(sol);
                    }
                }
                None
            }
            // Two conversions that disagree about the type argument are an
            // ambiguity, not a guess.
            _ => None,
        }
    }

    /// Every implicit clause of a conversion has a witness.
    fn conv_implicits_resolve(&self, id: SymbolId, from: &Type) -> bool {
        let wants = self.conv_implicit_params(id, from, &Type::NoType);
        if wants.iter().flatten().next().is_none() {
            return true;
        }
        // Checking the same conversion inside its own clause would not
        // terminate; nsc's `openImplicits` is the same guard, and its
        // `dominates` is what lets a *shrinking* re-entry through -- the view
        // for `((A, B), C)` asks for one for `(A, B)`, which is the same rule
        // on a smaller type and must be allowed.
        if self
            .open_implicits
            .borrow()
            .iter()
            .any(|(sid, spt)| *sid == id && dominates(self, from, spt))
        {
            return false;
        }
        self.open_implicits.borrow_mut().push((id, from.clone()));
        let ok = wants.iter().flatten().all(|want| {
            self.search_implicit_at(want, 1).is_found() || self.conv_param_view_resolves(want)
        });
        self.open_implicits.borrow_mut().pop();
        ok
    }

    /// A conversion's own implicit parameter of type `A => B` is a **view**
    /// request (SLS 7.2), not a value request: an `implicit def` answers it
    /// eta-expanded, exactly as [`Typer::conversion_view`] answers one in a
    /// method's clause.
    ///
    /// slick's `Ordered.tuple2Ordered[T1, T2](t: (T1, T2))(implicit ev1: T1 =>
    /// Ordered, ev2: T2 => Ordered)` is the shape: with `T1`/`T2` solved from
    /// the tuple, `ev1` is `Predef.$conforms` (a `ColumnOrdered[Int]` already
    /// *is* an `Ordered`) but `ev2` is the conversion `columnToOrdered`, which
    /// a value search can never find -- so the whole tuple view was refused
    /// and gitbucket's `sortBy { … => issue.issueId.desc -> commentId }` had
    /// no `Ordered` for its pair.
    ///
    /// A bare type parameter on either end is declined: a view out of a type
    /// the search has not pinned down would let any conversion in scope claim
    /// it, which is nsc's rule in `inferView` too.
    fn conv_param_view_resolves(&self, want: &Type) -> bool {
        let Type::Function { params, ret } = want else {
            return false;
        };
        if params.len() != 1 {
            return false;
        }
        let (from, to) = (&params[0], ret.as_ref());
        if [from, to].iter().any(|t| {
            t.is_no_type()
                || t.is_error()
                || matches!(t, Type::Wildcard | Type::TypeParam(_) | Type::Nothing)
        }) {
            return false;
        }
        self.search_conversion(from, to).is_found()
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
        // nsc's `inferImplicit`: a wanted type that is already erroneous ends
        // the search before a single candidate is looked at. Otherwise
        // [`Self::plausibly_inhabits`] lets `Type::Error` stand for anything and
        // every implicit in scope becomes a candidate -- which is how a missing
        // `Predef.Manifest` turned into `ambiguous implicit:` naming thirty-two
        // slick column types (`crate::check::type_is_erroneous`).
        if crate::check::type_is_erroneous(pt) {
            return (ImplicitSearch::None, Vec::new());
        }
        let key = memo_key(pt, undet);
        let open = self.open_implicit_mask();
        if let Some(hit) = self.memo_lookup(key, pt, undet, depth, open) {
            return hit;
        }
        // The memo lives from here until the outermost implicit operation
        // returns; see [`ImplicitMemo`] for why the context it was computed in
        // does not have to be part of the key.
        let live = self.memo_scope();
        let (outer_probe, outer_cut, mut outer_routes) = {
            let mut m = self.implicit_memo.borrow_mut();
            (
                std::mem::replace(&mut m.probe, 0),
                std::mem::take(&mut m.cut),
                std::mem::take(&mut m.routes),
            )
        };
        let out = self.search_implicit_uncached(pt, undet, depth);
        let (probe, cut, routes) = {
            let mut m = self.implicit_memo.borrow_mut();
            let (p, c) = (m.probe, m.cut);
            let routes = std::mem::take(&mut m.routes);
            outer_routes.extend(routes.iter().map(|(&member, &module)| (member, module)));
            m.probe = outer_probe | p;
            m.cut = outer_cut | c;
            m.routes = outer_routes;
            (p, c, routes)
        };
        if probe & open == 0 {
            let mut m = self.implicit_memo.borrow_mut();
            m.entries.entry(key).or_default().push(MemoEntry {
                pt: pt.clone(),
                undet: undet.to_vec(),
                probe,
                depth,
                cut,
                result: out.0.clone(),
                bindings: out.1.clone(),
                routes,
            });
        }
        drop(live);
        out
    }

    /// Keeps [`ImplicitMemo`] alive for the whole of one `&self` implicit
    /// operation, and clears it when that operation returns.
    ///
    /// Every entry point that runs more than one search has to hold one.
    /// [`Self::search_conversion`] tries every candidate in scope and
    /// [`Self::conv_targs`] runs a search per candidate, so a memo that spanned
    /// only one search was thrown away several hundred times per conversion
    /// and answered nothing: `sample` had a single `search_conversion` on the
    /// stack for twelve unbroken seconds.
    ///
    /// `&self` is what makes the memo sound -- see [`ImplicitMemo`] -- so a
    /// caller that needs `&mut self` cannot hold one, and the borrow checker
    /// is what says so.
    fn memo_scope(&self) -> MemoLive<'_> {
        self.implicit_memo.borrow_mut().depth += 1;
        MemoLive {
            typer: self,
            _prefixes: self.import_prefix_scope(),
        }
    }

    /// The candidates the open-implicit stack could make
    /// [`Self::implicit_diverges`] answer `true` for, as a signature.
    fn open_implicit_mask(&self) -> u64 {
        self.open_implicits
            .borrow()
            .iter()
            .fold(0u64, |m, (id, _)| {
                m | sym_bit(
                    self.implicit_instance_origins
                        .get(id)
                        .copied()
                        .unwrap_or(*id),
                )
            })
    }

    /// A hit also hands the entry's `probe` and `cut` to whatever search is
    /// running, exactly as recomputing it would have: the caller is about to
    /// be memoized itself, and an answer it took from here is part of *its*
    /// subtree. Without that, a search whose only depth-limited or
    /// divergence-checked step came from the memo would be recorded as
    /// depending on neither, and then reused where it does not hold.
    fn memo_lookup(
        &self,
        key: u64,
        pt: &Type,
        undet: &[SymbolId],
        depth: usize,
        open: u64,
    ) -> Option<(ImplicitSearch, Vec<(SymbolId, Type)>)> {
        let mut m = self.implicit_memo.borrow_mut();
        let e = m.entries.get(&key)?.iter().find(|e| {
            (e.depth == depth || (!e.cut && e.depth > depth))
                && e.probe & open == 0
                && e.undet.as_slice() == undet
                && e.pt == *pt
        })?;
        let out = (e.result.clone(), e.bindings.clone());
        let (probe, cut, routes) = (e.probe, e.cut, e.routes.clone());
        self.implicit_via_module
            .borrow_mut()
            .extend(routes.iter().map(|(&member, &module)| (member, module)));
        m.probe |= probe;
        m.cut |= cut;
        m.routes.extend(routes);
        Some(out)
    }

    fn search_implicit_uncached(
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

    /// Complete witness types before retrying a failed value conversion.
    /// Search itself is immutable, while normal implicit application loads
    /// both witness companions and candidate inheritance before rejecting them.
    pub(crate) fn warm_conversion_witnesses(&mut self, from: &Type, to: &Type) {
        self.warm_own_scope_once(to);
        let mut ids = self.implicits_in_scope();
        ids.extend(self.companion_implicits(from));
        ids.extend(self.companion_implicits(to));
        ids.sort_by_key(|id| id.0);
        ids.dedup();
        for id in ids {
            if self.first_clause_is_implicit(id) {
                continue;
            }
            let Some(param) = self.conversion_arg_ty(id) else {
                continue;
            };
            if !self.conv_param_matches(id, from, &param) {
                continue;
            }
            let wanted: Vec<Type> = self
                .conv_implicit_params(id, from, &Type::NoType)
                .into_iter()
                .flatten()
                .collect();
            for want in &wanted {
                self.warm_implicit_scope(want);
            }
            if !wanted.is_empty() {
                self.warm_implicit_candidates(&wanted);
            }
        }
    }

    pub(crate) fn search_conversion(&self, from: &Type, to: &Type) -> ImplicitSearch {
        let _live = self.memo_scope();
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
        let _live = self.memo_scope();
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
        // Free of the *unknowns* -- this view's own unsolved parameters and
        // the callee's -- not free of every type parameter. A type parameter
        // of an enclosing class or method is a fixed type here, and rejecting
        // it made `xs.to(mutable.ArrayBuffer)` fail for every abstract element
        // type: `class C[A] { def es: List[A] }` wants
        // `Factory[A, C1]`, `toFactory` answers `Factory[A, ArrayBuffer[A]]`,
        // and the `A` in it is C's own. `List[Int]` compiled all along, which
        // is why the shape survived. See `crates/cli/tests/tuplepat.rs`.
        let unknowns: Vec<SymbolId> = rest.iter().chain(open.iter()).copied().collect();
        if crate::check::mentions_tparam(&solved_to, &unknowns) {
            return None;
        }
        // The result may solve a parameter the source cannot determine.
        // Validate witnesses with that solution: a Shape[Rep[Int], Int]
        // cannot justify a conversion whose wanted result requires String.
        let source_args = self.conv_targs(id, from);
        let solved_args: Vec<Type> = tps
            .iter()
            .zip(solved_from_arg.iter())
            .zip(source_args)
            .map(|((tp, known), fallback)| {
                known.clone().or_else(|| u.solved(*tp)).unwrap_or(fallback)
            })
            .collect();
        if let Type::Method { paramss, .. } = &*cand_ty {
            for want in paramss.iter().skip(1).flatten() {
                let want = crate::symbol::subst_tparams_slice(&tps, &solved_args, want);
                // Viability and insertion must agree: a concrete ClassTag
                // is materialized rather than found in implicit scope.
                // The same fallback refuses an abstract A without evidence.
                if !self.search_implicit_at(&want, 1).is_found()
                    && self.classtag_apply_fallback(&want, Span::DUMMY).is_none()
                {
                    return None;
                }
            }
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
            // nsc's `isAsSpecific(ftpe1, ftpe2)`: `b` must be *applicable* to
            // `a`'s parameter type with `a`'s own type parameters abstract.
            // Comparing the two parameter types with `is_sub_type` instead left
            // `Array[T]` and `Array[T <: AnyRef]` unordered in both directions,
            // so `wrapRefArray` and `genericWrapArray` came out ambiguous for
            // every `Array[String]` used as a `Seq` (`scala/Array.scala`,
            // `scala/collection/concurrent/TrieMap.scala`, and the same pair
            // with `wrapCharArray` in `scala/io/Source.scala`).
            (Some(aa), Some(_)) => self.conv_accepts_opaque(b, &unwrap_byname(&aa)),
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
        let key = (a.0, b.0);
        {
            let m = self.implicit_memo.borrow();
            if m.depth > 0 {
                if let Some(&hit) = m.improves.get(&key) {
                    return hit;
                }
            }
        }
        let out = self.strictly_more_specific_uncached(a, b);
        let mut m = self.implicit_memo.borrow_mut();
        if m.depth > 0 {
            m.improves.insert(key, out);
        }
        out
    }

    fn strictly_more_specific_uncached(&self, a: SymbolId, b: SymbolId) -> bool {
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
    /// Warm `want`'s implicit scope, and that of the implicit clauses of the
    /// candidates its companions offer, `depth` levels further down.
    ///
    /// A witness can be a derivation rule whose own clause asks for a type
    /// the program never names: `OptionLift.repOptionLift` needs a
    /// `Shape[_ <: FlatShapeLevel, M, _, Rep[P]]`, and `Shape`'s companion is
    /// read by nothing else in a file with no table in it. The first
    /// `o.getOrElse(0)` on a `Rep[Option[Int]]` then had no `P` for its
    /// extension class and was "not a member"; the same line below a
    /// `TableQuery` compiled.
    fn warm_witness_chain(&mut self, want: &Type, depth: usize) {
        self.warm_implicit_scope(want);
        self.warm_implicit_candidates(std::slice::from_ref(want));
        if depth == 0 {
            return;
        }
        let mut nested: Vec<Type> = Vec::new();
        for c in self.companion_implicits(want) {
            if !self.only_implicit_clauses(c) {
                continue;
            }
            if let Type::Method { paramss, .. } = &*self.implicit_candidate_ty(c) {
                for p in paramss.iter().flatten() {
                    if !nested.contains(p) {
                        nested.push(p.clone());
                    }
                }
            }
        }
        for n in nested {
            self.warm_witness_chain(&n, depth - 1);
        }
    }

    pub(crate) fn search_extension(
        &mut self,
        from: &Type,
        name: &str,
        span: Span,
    ) -> Option<(SymbolId, SymbolId, Type)> {
        // A conversion is applicable only if its own implicit clauses have
        // witnesses ([`Self::drop_witnessless_conversions`], below). The
        // witness for `FlatMap[Box]` lives on `Box`'s companion, which is a
        // class file nothing else asks for, so warm the receiver's implicit
        // scope here as well: a conversion the receiver's own companion
        // supplies would otherwise be dropped for want of a class file.
        self.warm_implicit_scope(from);
        // Lexical implicits have precedence over the implicit scope. This is
        // observable when a source-defined implicit class deliberately
        // redeclares a standard extension name: cats' compat layer imports
        // `iterableOnceExtension.reduceOption` beside the library's
        // `IterableOnce.iterableOnceExtensionMethods`. The two conversions
        // have the same receiver shape, but SLS lookup chooses the imported
        // lexical candidate before consulting companions. Treating both
        // pools as one overload set made the selection look ambiguous and
        // reported "value reduceOption is not a member". Keep genuinely
        // ambiguous candidates within each pool; only fall back to the
        // companion pool when no lexical candidate can supply the member.
        let mut lexical_ids = self.implicits_in_scope();
        lexical_ids.sort_by_key(|id| id.0);
        lexical_ids.dedup();
        let mut companion_ids = self
            .companion_implicits(from)
            .into_iter()
            .chain(self.companion_implicits(&Type::Any))
            .collect::<Vec<_>>();
        companion_ids.sort_by_key(|id| id.0);
        companion_ids.dedup();
        let mut collect = |ids: Vec<SymbolId>| {
            let mut hits = Vec::new();
            for id in ids {
                let owner = self.st.get(id).owner;
                if let Some(prefix) = self.term_import_prefix_for(owner).map(|q| q.ty.clone()) {
                    let ty = self.st.get(id).ty.clone();
                    self.warm_receiver_type_members(&prefix, &ty);
                }
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
                // A result that still names the conversion's own type
                // parameters is one only its implicit clause can finish:
                // `anyOptionExtensionMethods[T, P](v: Rep[Option[T]])(implicit
                // ol: OptionLift[P, Rep[Option[T]]])` learns `P` from
                // `OptionLift`'s companion. Nothing has read that companion
                // the first time a file writes `c.isEmpty` on a `Rep[Option[_]]`,
                // so the witness search found no candidate at all and the
                // member was reported missing -- once, on the first use; every
                // later one worked. Warm the clauses' implicit scope and ask
                // again.
                let mut to = to;
                if !members.is_empty() {
                    let own = self.st.get(id).tparams.clone();
                    if crate::check::mentions_tparam(&to, &own) {
                        for want in self
                            .conv_implicit_params(id, from, &Type::NoType)
                            .into_iter()
                            .flatten()
                        {
                            self.warm_witness_chain(&want, 1);
                        }
                        if let Some(again) = self.conversion_result(id, from) {
                            to = again;
                        }
                    }
                }
                if let Some(m) = members.first() {
                    hits.push((id, *m, to));
                }
            }
            hits
        };
        let mut hits = collect(lexical_ids);
        if hits.is_empty() {
            hits = collect(companion_ids);
        }
        hits.sort_by_key(|(c, m, _)| (c.0, m.0));
        hits.dedup_by_key(|(c, m, _)| (c.0, m.0));
        self.drop_inherited_duplicates(&mut hits);
        self.drop_overridden_conversions(&mut hits);
        self.drop_witnessless_conversions(&mut hits, from, span);
        self.drop_inapplicable_conversions(&mut hits, name);
        self.drop_superseded_prelude_conversions(&mut hits);
        match hits.len() {
            1 => Some(hits.pop().unwrap()),
            0 => None,
            _ => {
                // nsc `improves` first: a view whose parameter is strictly more
                // specific than every other candidate's wins outright, however
                // the members are spread over the results. `import num._`
                // over an `Integral[T]` brings `mkNumericOps(lhs: T):
                // IntegralOps`, which *inherits* `+` from `NumericOps`, next to
                // `Predef.any2stringadd[A](self: A)`, which declares its own;
                // the declaration rule below picked the latter and every
                // `start + step` in `NumericRange` was "no matching overload
                // for (String)String with arguments (T)". A candidate the
                // owner rule ranks lower (`LowPriorityImplicits`) scores one
                // point each way in nsc, so it is left to the rules below.
                let low = self.inherited_conversions(&hits);
                let dominant: Vec<usize> = (0..hits.len())
                    .filter(|&i| {
                        (0..hits.len()).all(|j| {
                            j == i || self.conv_param_strictly_more_specific(hits[i].0, hits[j].0)
                        })
                    })
                    .collect();
                if let [i] = dominant[..] {
                    if !low[i] || low.iter().all(|l| *l) {
                        return Some(hits.swap_remove(i));
                    }
                }
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
                let low = self.inherited_conversions(&pool);
                if low.iter().any(|l| !l) {
                    let mut keep = low.iter().map(|l| !l);
                    pool.retain(|_| keep.next().unwrap_or(true));
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

    /// A stand-in for a `Predef` member does not compete with the member
    /// itself once the run's own sources have supplied it.
    ///
    /// `predef_reimport` supersedes the prelude's `Predef` snapshot **by
    /// name**, which covers every member the two spell the same way. They do
    /// not always: the prelude calls `Predef`'s `->` conversion
    /// `any2ArrowAssoc`, which is 2.10's name for it -- `javap -p
    /// scala.Predef$` on 2.13.16 has `public final <A> A ArrowAssoc(A)` and no
    /// `any2ArrowAssoc` at all -- while the library's `implicit final class
    /// ArrowAssoc` synthesizes `ArrowAssoc`. The names never met, both stayed
    /// in scope offering `->` for the same source type, and every `a -> b` in
    /// `src/library` tied between them and was reported as `value -> is not a
    /// member` (28 errors in 3 files, `docs/scala-library.md`'s item 0).
    ///
    /// The rule is narrow on purpose, in both directions:
    ///
    /// * It fires only in a run whose own sources define `scala.Predef`
    ///   (`SymbolTable::predef_superseded`). Two conversions genuinely in
    ///   scope for the same type **are** an ambiguity, and real scalac 2.13.16
    ///   says so -- a user `implicit class` offering `->` beside the real
    ///   `Predef.ArrowAssoc` is rejected with "implicit conversions are not
    ///   applicable because they are ambiguous". A blanket "source beats
    ///   prelude" would accept that program.
    /// * It only ever *narrows* a set of two or more, and only by dropping
    ///   candidates owned by the prelude `Predef`. A lone prelude candidate is
    ///   left standing, so a shape the source `Predef` cannot serve is not
    ///   turned into a member error.
    fn drop_superseded_prelude_conversions(&self, hits: &mut Vec<(SymbolId, SymbolId, Type)>) {
        if !self.st.predef_superseded || hits.len() < 2 {
            return;
        }
        let predef = self.st.predef;
        let pcls = self.st.module_class_of(predef);
        let prelude_end = self.st.prelude_end;
        let superseded = |c: &SymbolId| {
            c.0 < prelude_end && {
                let o = self.st.get(*c).owner;
                o == predef || o == pcls
            }
        };
        if hits.iter().any(|(c, _, _)| !superseded(c)) {
            hits.retain(|(c, _, _)| !superseded(c));
        }
    }

    /// A view whose own implicit arguments cannot be found is not a
    /// candidate, and the search carries on without it.
    ///
    /// nsc's `inferView` types the whole application, implicit clauses
    /// included; a failure there makes the candidate not applicable rather
    /// than raising its error, and `adaptToMemberWithArgs` falls back on the
    /// selection's own `value x is not a member of T`. cats'
    /// `toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])` fits *every*
    /// one-argument application by shape, so without this every
    /// `xs.flatMap(...)` on a type with no `FlatMap` instance was reported as
    /// a missing `FlatMap`, at two spans -- `type_select` inserting the view
    /// and `rewrite_apply_extension` inserting it again -- instead of as the
    /// member error scalac reports.
    ///
    /// Runs before the tie-breakers, so a witnessless conversion does not take
    /// part in an ambiguity either: the *other* view, the one that does apply,
    /// wins outright.
    fn drop_witnessless_conversions(
        &mut self,
        hits: &mut Vec<(SymbolId, SymbolId, Type)>,
        from: &Type,
        span: Span,
    ) {
        let mut i = 0;
        while i < hits.len() {
            if self.conv_implicits_available(hits[i].0, from, span) {
                i += 1;
            } else {
                hits.remove(i);
            }
        }
    }

    /// Whether every implicit clause of a conversion can actually be filled,
    /// warming exactly what [`Self::fill_conv_implicits`] warms.
    ///
    /// The two must agree: a clause this rejects costs a conversion nsc
    /// applies, and one it accepts that the fill then cannot satisfy is the
    /// duplicate diagnostic this pass exists to remove.
    fn conv_implicits_available(&mut self, id: SymbolId, from: &Type, span: Span) -> bool {
        let clauses = self.conv_implicit_params(id, from, &Type::NoType);
        for want in clauses.iter().flatten() {
            // `ClassTag[A]` with `A` still the conversion's own parameter is
            // `fill_conv_implicits`'s "unresolved spliceable type"; that
            // diagnostic says more than a member error, so leave it standing.
            if self.classtag_of_conv_tparam(id, want) {
                continue;
            }
            self.warm_implicit_scope(want);
            let mut search = self.search_implicit(want);
            if matches!(search, ImplicitSearch::None)
                && self.warm_implicit_candidates(std::slice::from_ref(want))
            {
                search = self.search_implicit(want);
            }
            if search.is_found()
                || self.classtag_apply_fallback(want, span).is_some()
                || (matches!(search, ImplicitSearch::None) && self.manifest_available(want, 0))
            {
                continue;
            }
            return false;
        }
        true
    }

    /// `ClassTag[A]` where `A` is one of the conversion's own type parameters.
    fn classtag_of_conv_tparam(&self, id: SymbolId, want: &Type) -> bool {
        let Type::Class { sym, args } = want else {
            return false;
        };
        self.st.get(*sym).jvm_name == "scala/reflect/ClassTag"
            && matches!(args.first(), Some(Type::TypeParam(tp))
                if self.st.get(id).tparams.contains(tp))
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

    /// Which of `pool`'s conversions nsc would score as *lower* priority.
    ///
    /// SLS 6.26.3 breaks a tie between two implicits partly by where they are
    /// defined: one owned by a class that another candidate's owner inherits
    /// from is the weaker of the two. `object Predef extends
    /// LowPriorityImplicits` is the standard library's own use of the rule,
    /// and it is the whole reason `"abc".slice(0, 2)` compiles: `augmentString`
    /// (on `Predef`) and `wrapString` (on `LowPriorityImplicits`) both offer a
    /// `slice`, both *declare* it -- `WrappedString` overrides
    /// `IndexedSeqOps#slice` -- and both take a bare `String`, so every later
    /// tie-break here scores them equal and the search gave up with `value
    /// slice is not a member of String`.
    ///
    /// The prelude's hand-written `Predef` snapshot has no base class to
    /// inherit from, so it carries the same fact as the `low_priority` flag
    /// on the two conversions that need it. That flag stays; this generalises
    /// it to every run whose own sources spell the hierarchy out, which is any
    /// run that compiles `Predef.scala` -- and to user code, which had the
    /// defect too and is what `crates/cli/tests/strarrayops.rs` pins.
    fn inherited_conversions(&self, pool: &[(SymbolId, SymbolId, Type)]) -> Vec<bool> {
        let owners: Vec<SymbolId> = pool.iter().map(|(c, _, _)| self.st.get(*c).owner).collect();
        pool.iter()
            .enumerate()
            .map(|(i, (c, _, _))| {
                if self.st.get(*c).low_priority {
                    return true;
                }
                let mine = owners[i];
                if mine.is_none() {
                    return false;
                }
                owners.iter().enumerate().any(|(j, &other)| {
                    j != i
                        && !other.is_none()
                        && other != mine
                        && self.st.is_ancestor_of(mine, other)
                })
            })
            .collect()
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

    /// nsc `isAsSpecific` for two one-parameter views, both ways: `a`'s
    /// parameter, its own type parameters kept abstract, must be accepted by
    /// `b`'s with `b`'s type parameters free to be inferred -- and not the
    /// other way round. `(T)IntegralOps` is strictly more specific than
    /// `[A](A)any2stringadd[A]`: `A := T` accepts a `T`, while an arbitrary
    /// `A` is no `T`. [`Self::conv_arg_strictly_more_specific`] frees the
    /// type parameters on *both* sides, which makes those two equal.
    fn conv_param_strictly_more_specific(&self, a: SymbolId, b: SymbolId) -> bool {
        let as_specific = |x: SymbolId, y: SymbolId| -> bool {
            match self.conversion_arg_ty(x) {
                Some(px) => self.conv_accepts_opaque(y, &unwrap_byname(&px)),
                None => false,
            }
        };
        a != b && as_specific(a, b) && !as_specific(b, a)
    }

    /// Whether the view `y` is applicable to an argument of type `px`, whose
    /// own type parameters are **abstract**: nsc's
    /// `isApplicableSafe(Nil, ftpe2, ftpe1.paramTypes, WildcardType)`.
    ///
    /// The bounds are part of applicability, and that is what separates two
    /// views an erasure to wildcards leaves equal:
    ///
    /// ```scala
    /// implicit def genericWrapArray[T](xs: Array[T]): ArraySeq[T]
    /// implicit def wrapRefArray[T <: AnyRef](xs: Array[T]): ArraySeq.ofRef[T]
    /// ```
    ///
    /// `genericWrapArray` accepts an `Array[T']` for `wrapRefArray`'s abstract
    /// `T'`; `wrapRefArray` does *not* accept an `Array[T]` for
    /// `genericWrapArray`'s unbounded `T`, because nothing says `T <: AnyRef`.
    /// So `wrapRefArray` is strictly the more specific, which is the one nsc
    /// picks for an `Array[String]`. Both directions held before and the pair
    /// was reported as "ambiguous implicit: wrapRefArray, genericWrapArray"
    /// (`scala/Array.scala`, `scala/collection/concurrent/TrieMap.scala`).
    fn conv_accepts_opaque(&self, y: SymbolId, px: &Type) -> bool {
        let Some(py) = self.conversion_arg_ty(y) else {
            return false;
        };
        let py = unwrap_byname(&py);
        let tps = self.st.get(y).tparams.clone();
        if tps.is_empty() {
            return self.st.is_sub_type(px, &py);
        }
        let targs = self.conv_targs(y, px);
        for (i, tp) in tps.iter().enumerate() {
            if !crate::check::mentions_tparam(&py, &[*tp]) {
                continue;
            }
            let (Some(arg), Some(hi)) = (targs.get(i), self.st.get(*tp).bound_hi.clone()) else {
                continue;
            };
            let hi = crate::symbol::subst_tparams_slice(&tps, &targs, &hi);
            if !self.arg_provably_within(arg, &hi) {
                return false;
            }
        }
        let inst = crate::symbol::subst_tparams_slice(&tps, &targs, &py);
        self.st.is_sub_type(px, &inst)
    }

    /// `arg <: hi`, judged so that an **abstract** `arg` is only as good as its
    /// own declared bound.
    ///
    /// `is_sub_type` lets a bare type parameter stand for any reference type
    /// (`def c[T](t: T): AnyRef = t` is accepted, where scalac reports a
    /// mismatch), and the specificity comparison above cannot use an answer
    /// that is true of every parameter. Judged here rather than by tightening
    /// conformance, which every part of the compiler leans on.
    fn arg_provably_within(&self, arg: &Type, hi: &Type) -> bool {
        match arg {
            Type::TypeParam(id) | Type::TypeMember(id) => match self.st.get(*id).bound_hi.clone() {
                Some(b) => self.st.is_sub_type(&b, hi),
                None => matches!(hi, Type::Any),
            },
            _ => self.st.is_sub_type(arg, hi),
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

    /// A candidate that is a one-argument function *by inheritance*, read as
    /// the `A => B` it conforms to.
    ///
    /// nsc searches a view with the expected type `Function1[from, ?]`, so
    /// **any** implicit whose type conforms to it is a view -- not only a `def`
    /// and not only a value whose type is written as a function. `<:<` is the
    /// case the library relies on: `sealed abstract class <:<[-From, +To]
    /// extends (From => To)`, so
    ///
    /// ```scala
    /// def invert[El1, It1[a] <: Iterable[a]](implicit w1: T1 <:< It1[El1]) = {
    ///   val it1 = x._1.iterator   // T1 seen through w1
    /// ```
    ///
    /// resolves `iterator` through `w1` (`scala/runtime/Tuple2Zipped.scala`,
    /// `Tuple3Zipped.scala`). Only `Type::Function` parents count: the
    /// `PartialFunction` / `Map` stand-ins [`Typer::function_view`] also
    /// recognises are how the prelude spells those classes, and letting an
    /// implicit `Map[K, V]` act as a view for every `K` receiver is a much
    /// wider rule than the one the library needs.
    fn conv_inherited_function1(&self, cand_ty: &Type) -> Option<Type> {
        let Type::Class { sym, args } = cand_ty else {
            return None;
        };
        if self.st.function_class_shape(*sym, args).is_some() {
            // Already a `Function1` applied as a class: the callers' own
            // `Type::Function` arm (after `function_class_shape`) handles it.
            return None;
        }
        self.st
            .base_type_seq(cand_ty)
            .into_iter()
            .find_map(|base| match &base {
                Type::Function { params, .. } if params.len() == 1 => Some(base),
                Type::Class { sym, args } => self
                    .st
                    .function_class_shape(*sym, args)
                    .filter(|f| matches!(f, Type::Function { params, .. } if params.len() == 1)),
                _ => None,
            })
    }

    fn conversion_result(&self, id: SymbolId, from: &Type) -> Option<Type> {
        let _prefixes = self.import_prefix_scope();
        if !self.st.get(id).flags.contains(Flags::IMPLICIT) {
            return None;
        }
        let cand_ty = self.implicit_candidate_ty(id);
        let cand_ty = match self.conv_inherited_function1(&cand_ty) {
            Some(f) => std::borrow::Cow::Owned(f),
            None => cand_ty,
        };
        match &*cand_ty {
            Type::Method { paramss, ret } => {
                let ps = paramss.first().filter(|ps| ps.len() == 1)?;
                // `implicit def toDeferrer[A](l: => LazyList[A]): Deferrer[A]`
                // converts a `LazyList` like any other conversion; the thunk is
                // the *call*'s business, and `adapt` wraps the argument when it
                // gets there. Reading the parameter as written made every
                // by-name conversion invisible -- which is the whole of
                // `a #:: xs`.
                let p0 = unwrap_byname(&ps[0]);
                if self.conv_param_matches(id, from, &p0) {
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
        if tps.is_empty() {
            // No type arguments to infer. Callers still check the view's
            // implicit clauses before applying it.
            return Vec::new();
        }
        let cand_ty = self.implicit_candidate_ty(id);
        let cand_ty = match self.conv_inherited_function1(&cand_ty) {
            Some(f) => std::borrow::Cow::Owned(f),
            None => cand_ty,
        };
        let param: Option<&Type> = match &*cand_ty {
            Type::Method { paramss, .. } => paramss.first().and_then(|c| c.first()),
            Type::Function { params, .. } => params.first(),
            _ => None,
        };
        let Some(param) = param else {
            return vec![Type::AnyRef; tps.len()];
        };
        let param = &unwrap_byname(param);
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
        self.solve_conv_targs_from_implicits(&cand_ty, tps, &mut solved);
        let ret = match &*cand_ty {
            Type::Method { ret, .. } | Type::Function { ret, .. } => Some(ret.as_ref()),
            _ => None,
        };
        solved
            .into_iter()
            .zip(tps)
            .map(|(t, tp)| {
                t.unwrap_or_else(|| {
                    // A parameter escaping in the view result is still open.
                    // Replacing it with Object also fabricates ClassTag evidence.
                    if ret.is_some_and(|r| crate::check::mentions_tparam(r, &[*tp])) {
                        Type::TypeParam(*tp)
                    } else {
                        self.st.get(*tp).bound_lo.clone().unwrap_or(Type::Nothing)
                    }
                })
            })
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
    /// Search only parameters mentioned by an implicit clause. Even a
    /// parameter absent from the result can be fixed by an explicit witness.
    fn solve_conv_targs_from_implicits(
        &self,
        cand_ty: &Type,
        tps: &[SymbolId],
        solved: &mut [Option<Type>],
    ) {
        // The caller already resolved the candidate at its import/owner
        // prefix, and no implicit search has run since that snapshot.
        let Type::Method { paramss, .. } = cand_ty else {
            return;
        };
        if paramss.len() < 2 {
            return;
        }
        let undet: Vec<SymbolId> = tps
            .iter()
            .zip(solved.iter())
            .filter(|(tp, s)| {
                s.is_none()
                    && paramss[1..]
                        .iter()
                        .flatten()
                        .any(|p| crate::check::mentions_tparam(p, &[**tp]))
            })
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
    pub(crate) fn conv_implicit_params(
        &self,
        id: SymbolId,
        from: &Type,
        to: &Type,
    ) -> Vec<Vec<Type>> {
        let cand_ty = self.implicit_candidate_ty(id);
        let Type::Method { paramss, ret } = &*cand_ty else {
            return Vec::new();
        };
        if paramss.len() < 2 {
            return Vec::new();
        }
        let tps = self.st.get(id).tparams.clone();
        let mut targs = self.conv_targs(id, from);
        // Open-view inference has already solved the conversion's result.
        // Parameters absent from the receiver (A in EvidenceIterableFactory)
        // must use that solution before searching Ordering[A]/ClassTag[A].
        // Receiver constraints remain authoritative; only still-open slots
        // are completed from the actual result of the inserted application.
        let result = crate::symbol::subst_tparams_slice(&tps, &targs, ret);
        for (tp, arg) in tps.iter().zip(targs.iter_mut()) {
            if !to.is_no_type() && *arg == Type::TypeParam(*tp) {
                if let Some(solved) = unify_conv_tparam(*tp, &result, to) {
                    *arg = solved;
                }
            }
        }
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
        let param = &unwrap_byname(param);
        // Match a polymorphic conversion against the receiver after solving
        // its own parameters structurally. Erasing them to wildcards first
        // loses the variance of nested function types such as
        // `A => Option[B]`; the exact `Int => Option[String]` receiver then
        // fails the wildcard subtype check even though nsc accepts the view.
        let tps = &self.st.get(id).tparams;
        if !tps.is_empty() {
            let targs = self.conv_targs(id, from);
            if !self.conv_targs_within_bounds(id, param, &targs) {
                return false;
            }
            let instantiated = crate::symbol::subst_tparams_slice(tps, &targs, param);
            if self.st.is_sub_type(from, &instantiated) {
                return true;
            }
        }
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
        // That widening alone would let a higher-kinded conversion claim every
        // applied type. What keeps it honest is that a conversion whose own
        // implicit clause has no witness is not applicable at all -- but that
        // is a property of the whole application, not of this parameter, and
        // deciding it here could only be done with the implicit scopes as they
        // happened to be loaded. Every caller of [`Self::conversion_result`]
        // now settles it for itself: [`Self::view_undet_bindings`] with
        // [`Self::conv_implicits_resolve`], [`Self::search_extension`] with
        // [`Self::drop_witnessless_conversions`], which may load first.
        fits_ctor
    }

    /// Whether the type arguments the receiver pins on a conversion's own
    /// parameters respect their declared **upper bounds**.
    ///
    /// `Predef.wrapRefArray[T <: AnyRef](xs: Array[T])` is not a candidate for
    /// an `Array[Char]`: `T` would have to be `Char`. Without the check it
    /// stood beside `wrapCharArray` and `genericWrapArray` and every
    /// `Array(c): Seq[Char]` in `scala/io/Source.scala` was "ambiguous
    /// implicit: wrapRefArray, wrapCharArray, genericWrapArray". nsc's
    /// `isApplicable` checks the bounds as part of deciding applicability, and
    /// an inapplicable alternative never reaches the ambiguity report.
    ///
    /// Only a parameter the declared argument type actually mentions is
    /// judged: that is the one [`Self::conv_targs`] solved from the receiver
    /// structurally. A parameter the receiver says nothing about falls back to
    /// `Nothing` (or stays itself) there, and holding that against the bound
    /// would reject conversions nsc accepts. Lower bounds are left alone for
    /// the same reason -- a `B >: A` is routinely solved as exactly `A` here,
    /// and nsc widens it instead.
    fn conv_targs_within_bounds(&self, id: SymbolId, param: &Type, targs: &[Type]) -> bool {
        let tps = self.st.get(id).tparams.clone();
        for (i, tp) in tps.iter().enumerate() {
            let Some(arg) = targs.get(i) else {
                continue;
            };
            if matches!(arg, Type::TypeParam(x) if x == tp) {
                continue;
            }
            if !crate::check::mentions_tparam(param, &[*tp]) {
                continue;
            }
            let Some(hi) = self.st.get(*tp).bound_hi.clone() else {
                continue;
            };
            let hi = crate::symbol::subst_tparams_slice(&tps, targs, &hi);
            if !self.st.is_sub_type(arg, &hi) {
                return false;
            }
        }
        true
    }

    /// The single value parameter of a conversion, as declared.
    pub(crate) fn conv_first_param(&self, id: SymbolId) -> Option<Type> {
        match &*self.implicit_candidate_ty(id) {
            Type::Method { paramss, .. } => paramss.first().and_then(|c| c.first()).cloned(),
            Type::Function { params, .. } => params.first().cloned(),
            _ => None,
        }
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
            byname_thunk: false,
            byname_type_marker: false,
        };
        if let Some(prefix) = self.instance_object_import_prefix(id) {
            return Tree {
                kind: TreeKind::Select {
                    qual: Box::new(prefix),
                    name,
                },
                ..ident
            };
        }
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
                        qual: Box::new(prefix),
                        name,
                    },
                    ty,
                    sym: id,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                    byname_thunk: false,
                    byname_type_marker: false,
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
            byname_thunk: false,
            byname_type_marker: false,
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
            byname_thunk: false,
            byname_type_marker: false,
        }
    }

    /// The import path an implicit declared in an `object` that belongs to an
    /// *instance* was brought in through.
    ///
    /// `import profile.api._` where `api` is an `object` of `profile`'s class:
    /// the object is `profile`'s own, reached only by calling `profile.api`.
    /// Emitted as a bare name, the view was loaded as the `api` of a cast
    /// `this` -- a `ClassCastException` from a program that typechecked, where
    /// nsc's tree is `X.this.profile.api.wrap(…)`. A static object (one only
    /// packages and objects enclose) is its own receiver and is left to the
    /// callers below. The path is the one the scope's own binding of the name
    /// was imported under, so it is this import's and no other file's.
    fn instance_object_import_prefix(&self, id: SymbolId) -> Option<Tree> {
        let owner = self.st.get(id).owner;
        if owner.is_none() || self.st.get(owner).kind != SymKind::ModuleClass {
            return None;
        }
        let mut up = self.st.get(owner).owner;
        loop {
            if up.is_none() || up == self.st.root {
                return None;
            }
            match self.st.get(up).kind {
                SymKind::Package => return None,
                SymKind::Module | SymKind::ModuleClass => up = self.st.get(up).owner,
                _ => break,
            }
        }
        // Written inside the object (or a class it encloses): its `this`.
        if self.st.enclosing_class_reaching(owner).is_some() {
            return None;
        }
        let name = &self.st.get(id).name;
        for scope in self.st.scopes.iter().rev() {
            let origin = scope
                .lookup_ranked(name)
                .iter()
                .find(|b| b.sym == id)
                .map(|b| b.origin)
                .or_else(|| {
                    scope
                        .wildcards()
                        .iter()
                        .find(|w| {
                            w.offers(name)
                                && (w.owner == owner
                                    || crate::pickle_supply::inherits_from(
                                        &self.st, w.owner, owner,
                                    ))
                        })
                        .map(|w| w.origin)
                });
            let Some(origin) = origin else {
                continue;
            };
            let prefix = self.object_import_prefixes.get(&origin)?;
            if prefix.ty.is_no_type() || prefix.ty.is_error() {
                return None;
            }
            return self.writable_import_prefix(prefix);
        }
        None
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

    /// The unknown that was bound *to* `tp`, when one was.
    ///
    /// Unification of two unknowns binds whichever side it reaches first, and
    /// for a candidate's result against the wanted type that is always the
    /// candidate's own parameter. The wanted type's parameter is then left
    /// with no solution of its own even though the two are known to be equal,
    /// and a caller that goes on to solve the candidate's parameters
    /// separately ([`Typer::implicit_fit_open`]) needs the other direction.
    ///
    /// Lowest symbol id when several are bound to `tp`, so the answer does not
    /// depend on the hash map's iteration order.
    fn alias_of(&self, tp: SymbolId) -> Option<SymbolId> {
        self.bound
            .iter()
            .filter(|(_, v)| matches!(v, Type::TypeParam(x) if *x == tp))
            .map(|(k, _)| SymbolId(*k))
            .min_by_key(|s| s.0)
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
        // An unknown facing a bare `_` is not constrained by it (see the
        // wildcard arm below), and `bind` refuses to record `_` as a
        // solution -- which used to fail the whole unification.
        // `repColumnShape[T]: Shape[Level, Rep[T], T, Rep[T]]` against
        // `Shape[_ <: FlatShapeLevel, ?M, _, Rep[Int]]` (slick's
        // `OptionLift.repOptionLift` asking which `M` packs to `Rep[Int]`)
        // stopped at `T` against `_`, before `Rep[T]` against `Rep[Int]`
        // could say `T = Int`; every `c.? isEmpty` then had no
        // `AnyOptionExtensionMethods`.
        if matches!(a, Type::Wildcard) || matches!(b, Type::Wildcard) {
            let unbound = |t: &Type| {
                self.unknown_of(t)
                    .is_some_and(|id| !self.bound.contains_key(&id))
            };
            if unbound(a) || unbound(b) {
                return true;
            }
        }
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
        // An inner class behind a prefix (`prefix.rs`) unifies as the class it
        // is -- the prefix decides conformance, not what the arguments are.
        // Against a *different* class its base type there is read through the
        // prefix, which is what instantiates the enclosing class's parameters:
        // `refl: A =:= A` fitted to `hm.KeySet <:< MySet[?T]` solves `?T` from
        // `MySet[Int]`, not `MySet[K]`.
        let (av, bv) = (
            crate::symbol::SymbolTable::as_seen_from_view(a).is_some(),
            crate::symbol::SymbolTable::as_seen_from_view(b).is_some(),
        );
        if av || bv {
            let ca = crate::prefix::strip_view(a).clone();
            let cb = crate::prefix::strip_view(b).clone();
            if let (Type::Class { sym: s1, .. }, Type::Class { sym: s2, .. }) = (&ca, &cb) {
                if s1 != s2 {
                    if av {
                        if let Some(bt) = self.typer.base_type_instance(a, *s2, 0) {
                            return self.unify_at(&bt, &cb, depth + 1);
                        }
                    }
                    if bv {
                        if let Some(bt) = self.typer.base_type_instance(b, *s1, 0) {
                            return self.unify_at(&ca, &bt, depth + 1);
                        }
                    }
                }
            }
            return self.unify_at(&ca, &cb, depth + 1);
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
            // `(A, B)` and `Tuple2[A, B]` are two spellings of one type -- but
            // only for the tuple class itself. Any class of the same arity used
            // to pass: slick's `anyToShapedValue(v): ShapedValue[T, U]` unified
            // with a wanted `(String, String)` by `T := String, U := String`,
            // and `val t: (String, String) = "x"` compiled (to a
            // `ShapedValue` where a `Tuple2` was promised).
            (Type::Tuple(ts), Type::Class { sym, args })
            | (Type::Class { sym, args }, Type::Tuple(ts))
                if ts.len() == args.len()
                    && self.typer.st.get(*sym).jvm_name == format!("scala/Tuple{}", ts.len()) =>
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
        (
            Type::Function {
                params: pp,
                ret: pr,
            },
            Type::Function {
                params: fp,
                ret: fr,
            },
        ) => pp
            .iter()
            .zip(fp.iter())
            .find_map(|(p, f)| unify_conv_tparam(tp, p, f))
            .or_else(|| unify_conv_tparam(tp, pr, fr)),
        (Type::Array(p), Type::Array(a)) => unify_conv_tparam(tp, p, a),
        (Type::Class { args: pa, .. }, Type::Class { args: fa, .. }) => pa
            .iter()
            .zip(fa.iter())
            .find_map(|(p, f)| unify_conv_tparam(tp, p, f)),
        // A tuple parameter. `Type::Tuple` and `TupleN[...]` are the same
        // type and both spellings reach here -- the pickle writes slick's
        // `Ordered.tuple2Ordered[T1, T2](t: (T1, T2))` parameter as a
        // `Type::Tuple`, and the argument `(issue.issueId.desc, commentId)`
        // as the class. Without this the parameters were never solved from
        // the argument, the conversion's own `T1 => Ordered` / `T2 => Ordered`
        // clause was searched with them open, and no tuple view applied at
        // all (gitbucket's `sortBy { … => issue.issueId.desc -> commentId }`).
        (Type::Tuple(pa), Type::Tuple(fa))
        | (Type::Tuple(pa), Type::Class { args: fa, .. })
        | (Type::Class { args: pa, .. }, Type::Tuple(fa))
            if pa.len() == fa.len() =>
        {
            pa.iter()
                .zip(fa.iter())
                .find_map(|(p, f)| unify_conv_tparam(tp, p, f))
        }
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

/// A conversion parameter as the *receiver* has to fit it.
///
/// `implicit def toDeferrer[A](l: => LazyList[A])` takes its argument by name,
/// and a by-name parameter accepts exactly what the type inside it accepts --
/// the thunk is built by `adapt` at the call, not by the conformance test.
fn unwrap_byname(t: &Type) -> Type {
    match t {
        Type::ByName(inner) => (**inner).clone(),
        other => other.clone(),
    }
}

#[cfg(test)]
mod memo_tests {
    use super::*;
    use crate::check::TypecheckOptions;

    #[test]
    fn fresh_search_signatures_do_not_become_declared_candidates() {
        let mut typer = Typer::new(0, &TypecheckOptions::default());
        let root = typer.st.root;
        let owner = typer
            .st
            .alloc("Scope", root, SymKind::Class, Flags::EMPTY, "Scope");
        let method = typer
            .st
            .alloc("derive", owner, SymKind::Method, Flags::IMPLICIT, "derive");
        let tp = typer
            .st
            .alloc("A", method, SymKind::TypeParam, Flags::EMPTY, "A");
        typer.st.get_mut(method).tparams = vec![tp];
        typer.st.get_mut(method).ty = Type::Method {
            paramss: vec![],
            ret: Box::new(Type::TypeParam(tp)),
        };
        let members = typer.st.get(owner).members.clone();
        let method_members = typer.st.get(method).members.clone();
        assert!(typer.prepare_implicit_instances(method, MAX_IMPLICIT_DEPTH));
        assert_eq!(typer.st.get(owner).members, members);
        assert_eq!(typer.st.get(method).members, method_members);
        assert!(!typer.prepare_implicit_instances(method, MAX_IMPLICIT_DEPTH));
        assert_eq!(typer.st.get(owner).members, members);
    }

    #[test]
    fn annotated_recursive_tail_decreases_but_self_loop_does_not() {
        let mut typer = Typer::new(0, &TypecheckOptions::default());
        let root = typer.st.root;
        let list = typer
            .st
            .alloc("List", root, SymKind::Class, Flags::EMPTY, "List");
        let tail = Type::Class {
            sym: list,
            args: vec![Type::Int],
        };
        let full = Type::Class {
            sym: list,
            args: vec![Type::Annotated {
                tpe: Box::new(tail.clone()),
                annot: "uncheckedVariance".into(),
            }],
        };
        assert!(!dominates(&typer, &tail, &full));
        assert!(dominates(&typer, &full, &full));
        assert!(dominates(&typer, &full, &tail));
    }

    #[test]
    fn memo_replays_inherited_companion_route() {
        let mut typer = Typer::new(0, &TypecheckOptions::default());
        let root = typer.st.root;
        let shared = typer
            .st
            .alloc("Shared", root, SymKind::Class, Flags::TRAIT, "Shared");
        let mut companion = |name: &str| {
            let class = typer
                .st
                .alloc(name, root, SymKind::Class, Flags::EMPTY, name);
            let module = typer.st.alloc(
                name,
                root,
                SymKind::Module,
                Flags::EMPTY,
                format!("{name}$"),
            );
            let module_class = typer.st.alloc(
                format!("{name}$"),
                root,
                SymKind::ModuleClass,
                Flags::EMPTY,
                format!("{name}$"),
            );
            typer.st.get_mut(module).ty = Type::ModuleRef(module_class);
            typer.st.get_mut(module_class).parents = vec![Type::Class {
                sym: shared,
                args: vec![],
            }];
            (
                Type::Class {
                    sym: class,
                    args: vec![],
                },
                module,
            )
        };
        let (a, module_a) = companion("MemoA");
        let (b, module_b) = companion("MemoB");
        let witness = typer.st.alloc(
            "evidence",
            shared,
            SymKind::Term,
            Flags::IMPLICIT,
            "evidence",
        );
        typer.st.get_mut(witness).ty = a.clone();

        // Conversion search keeps one memo alive while trying different
        // wanted types. Both companions inherit the same member symbol.
        let _live = typer.memo_scope();
        assert!(
            matches!(typer.search_implicit_at(&a, 1), ImplicitSearch::Found(id) if id == witness)
        );
        assert_eq!(
            typer.implicit_via_module.borrow().get(&witness.0),
            Some(&module_a)
        );
        assert!(matches!(
            typer.search_implicit_at(&b, 1),
            ImplicitSearch::None
        ));
        assert_eq!(
            typer.implicit_via_module.borrow().get(&witness.0),
            Some(&module_b)
        );
        assert!(
            matches!(typer.search_implicit_at(&a, 1), ImplicitSearch::Found(id) if id == witness)
        );
        assert_eq!(
            typer.implicit_via_module.borrow().get(&witness.0),
            Some(&module_a)
        );
        // Failed searches must replay their candidate discovery too.
        assert!(matches!(
            typer.search_implicit_at(&b, 1),
            ImplicitSearch::None
        ));
        assert_eq!(
            typer.implicit_via_module.borrow().get(&witness.0),
            Some(&module_b)
        );
    }
}
