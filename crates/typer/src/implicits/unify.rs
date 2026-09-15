//! Two-sided type unification used by implicit search.
//!
//! The unifier solves both a candidate's type parameters and the call site's
//! undetermined parameters while fitting a candidate result to a wanted type.

use scala_rs_parser::{Flags, RefineDecl, SymbolId, Type};

use crate::check::Typer;

/// Two-sided unification for implicit search.
///
/// `unknowns` holds the candidate's own type parameters *and* the call-site
/// parameters the search has to solve. One-sided `check::unify_one` cannot do
/// the latter: fitting `<:<.refl[A0]: A0 =:= A0` to `A <:< (K, V)` binds `A0`
/// from the `From` position and then `K`/`V` from the `To` position, in the
/// same pass.
pub(super) struct Unify<'a> {
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
    /// Evidence may determine a call site's constructor (for example
    /// `Future[Int] <:< G[B]`). View insertion keeps constructors fixed so a
    /// conversion cannot replace an already applicable argument constructor.
    pub(super) fn allow_evidence_constructors(&mut self) {
        self.ctor_unknowns.extend(self.unknowns.iter().copied());
    }

    /// `own` are the candidate's own type parameters, `undet` the call site's.
    pub(super) fn new(
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
    pub(super) fn solved_open(&self, tp: SymbolId) -> Option<Type> {
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
    pub(super) fn alias_of(&self, tp: SymbolId) -> Option<SymbolId> {
        self.bound
            .iter()
            .filter(|(_, v)| matches!(v, Type::TypeParam(x) if *x == tp))
            .map(|(k, _)| SymbolId(*k))
            .min_by_key(|s| s.0)
    }

    /// The solution for `tp`, with any nested unknowns resolved.
    pub(super) fn solved(&self, tp: SymbolId) -> Option<Type> {
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
        // These constraints come from an expected evidence type, not from a
        // term whose singleton type should be widened during inference.
        self.bound.insert(id, ty.clone());
        true
    }

    pub(super) fn unify(&mut self, a: &Type, b: &Type) -> bool {
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
        // A class that inherits `FunctionN` is also a structural function
        // type.  Implicit result fitting has to use that fact in the same
        // direction as ordinary subtyping: a generic tuple reader can extend
        // a row-reader trait that extends `(Positioned => T)`, and its tuple
        // factory can be an implicit method.  Matching the factory's result
        // directly against the structural function wanted type used to fail
        // before the factory's element-reader clauses could solve `T1`..`T4`.
        //
        // `base_type_instance` reads the inherited `FunctionN` arguments at
        // the concrete class's type parameters; converting a structural
        // function to that same class form handles the reverse direction as
        // well.  This stays generic: any user class extending `FunctionN`
        // has the same relation.
        match (a, b) {
            (Type::Class { .. }, Type::Function { .. }) => {
                if let Some(function_class @ Type::Class { sym, .. }) =
                    self.typer.st.function_class_form(b)
                {
                    if let Some(base) = self.typer.base_type_instance(a, sym, 0) {
                        return self.unify_at(&base, &function_class, depth + 1);
                    }
                }
            }
            (Type::Function { .. }, Type::Class { .. }) => {
                if let Some(function_class) = self.typer.st.function_class_form(a) {
                    return self.unify_at(&function_class, b, depth + 1);
                }
            }
            _ => {}
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
            (Type::ThisType(x), Type::ModuleRef(y)) | (Type::ModuleRef(x), Type::ThisType(y))
                if x == y && self.typer.st.get(*x).kind == crate::symbol::SymKind::ModuleClass =>
            {
                true
            }
            (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, args: a2 }) => {
                if s1 == s2 && a1.len() == a2.len() {
                    let variances = self.typer.st.get(*s1).tparams.clone();
                    return a1.iter().zip(a2.iter()).enumerate().all(|(i, (x, y))| {
                        // A position that mentions **no unknown on either side**
                        // has nothing to solve, so the question there is
                        // conformance -- in the direction that position's
                        // variance demands, which is how nsc's constraint solver
                        // reads it. `IterableFactory.toBuildFrom[A, CC](f:
                        // IterableFactory[CC]): BuildFrom[Any, A, CC[A]]` fitted
                        // to a wanted `BuildFrom[IterableOnce[Future[T]], T, To]`
                        // stopped at `Any` against `IterableOnce[Future[T]]` --
                        // `BuildFrom`'s `From` is *contravariant*, so the
                        // candidate's wider type is exactly right -- and
                        // `Future.fold`/`Future.reduce`'s
                        // `sequence(futures)(ArrayBuffer, executor)` was
                        // `no matching overload` (`concurrent/Future.scala:813,832`).
                        // Tried only after structural unification declines, so
                        // no position that could still bind an unknown is
                        // decided this way.
                        if self.unify_at(x, y, depth + 1) {
                            return true;
                        }
                        if mentions_unknown(x, &self.unknowns)
                            || mentions_unknown(y, &self.unknowns)
                        {
                            return false;
                        }
                        let f = variances
                            .get(i)
                            .map(|&tp| self.typer.st.get(tp).flags)
                            .unwrap_or(Flags::EMPTY);
                        if f.contains(Flags::COVARIANT) {
                            self.typer.st.is_sub_type(x, y)
                        } else if f.contains(Flags::CONTRAVARIANT) {
                            self.typer.st.is_sub_type(y, x)
                        } else {
                            false
                        }
                    });
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

pub(super) fn mentions_unknown(ty: &Type, unknowns: &rustc_hash::FxHashSet<u32>) -> bool {
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
        Type::Refined { .. } => unknowns
            .iter()
            .any(|tp| crate::check::type_mentions_tparam_deep(ty, SymbolId(*tp))),
        _ => false,
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
