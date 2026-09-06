#![allow(dead_code)]
//! Type inference and adaptation to an expected type.
//!
//! Holds the undetermined-type-parameter machinery: which parameters are open,
//! what an argument or an expected type constrains them to, how a solution is
//! chosen and substituted back, and the bounds it then has to satisfy. It ends
//! with `adapt`, the point where a typed tree is made to fit the type that was
//! expected of it -- widening, eta expansion, SAM conversion, auto-application
//! and singleton adaptation.

use crate::check::*;
use crate::implicits::ImplicitSearch;
use crate::symbol::SymKind;
use crate::uncurry::eta_expand;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

impl Typer {
    /// nsc's `inferExprInstance`: a *parameterless* polymorphic method used in
    /// value position (`Vector.empty`, `mutable.HashMap.empty`) has nothing but
    /// the expected type to solve its parameters from, and whatever the
    /// expected type leaves open is solved to a bound -- the lower one
    /// (`Nothing`) where the parameter occurs covariantly, the declared upper
    /// one where it occurs contravariantly. Without this the reference keeps
    /// `Vector[A]` and conforms to nothing.
    ///
    /// Only done when there *is* an expected type: with none, an open parameter
    /// is still the more useful type here (the argument of a following call can
    /// pin it), and nsc's own instantiation point is later than ours.
    pub(crate) fn instantiate_parameterless(&self, sym: SymbolId, ty: Type, pt: &Type) -> Type {
        if sym.is_none() || pt.is_no_type() || pt.is_error() {
            return ty;
        }
        if matches!(ty, Type::Method { .. } | Type::Overload(_)) {
            return ty;
        }
        let tps = self.st.get(sym).tparams.clone();
        if tps.is_empty() || !mentions_tparam(&ty, &tps) {
            return ty;
        }
        // The expected type is the reference's own open parameters handed back
        // (an argument's parameter type inferred from that very argument): it
        // constrains nothing, and solving against it would only invent bounds.
        if mentions_tparam(pt, &tps) {
            return ty;
        }
        if !self.is_nullary_method_sym(sym) {
            return ty;
        }
        let inst = self.add_expected_constraints(sym, &ty, pt, Vec::new());
        let ids: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
        let vals: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
        let mut ty = crate::symbol::subst_tparams_slice(&ids, &vals, &ty);
        for tp in tps {
            if ids.contains(&tp) {
                continue;
            }
            let Some(v) = self.tparam_variance_in(&ty, tp, 1) else {
                continue;
            };
            let s = self.st.get(tp);
            let solved = if v < 0 {
                s.bound_hi.clone().unwrap_or(Type::Any)
            } else {
                s.bound_lo.clone().unwrap_or(Type::Nothing)
            };
            ty = crate::symbol::subst_tparams_slice(&[tp], &[solved], &ty);
        }
        ty
    }

    /// Type each argument against the parameter it fills and adapt it there,
    /// treating the callee's unsolved type parameters as variables: the
    /// argument is what says what they are.
    ///
    /// Used by the two recoveries that pick an alternative a second time
    /// (`widen_with_companion`, `rewrite_apply_extension`).
    pub(crate) fn adapt_args_to_params(
        &mut self,
        args: &mut [Tree],
        param_tys: &[Type],
        sym: SymbolId,
    ) {
        let own = (!sym.is_none()).then(|| self.st.get(sym).tparams.clone());
        for (i, a) in args.iter_mut().enumerate() {
            let p = param_at(param_tys, i).cloned().unwrap_or(Type::NoType);
            let open = self.open_tparams_of(&p, own.as_deref());
            if matches!(a.kind, TreeKind::Function { .. }) || a.ty.is_no_type() {
                let pt_arg = self.open_to_bounds(&p, &open);
                self.type_expr(a, &pt_arg);
            }
            if !p.is_no_type() {
                let p_check = self
                    .solve_open_from_arg(&a.ty, &p, &open)
                    .unwrap_or_else(|| self.open_to_bounds(&p, &open));
                self.adapt(a, &p_check);
            }
        }
    }

    /// The type parameters an argument's type still carries because the
    /// argument was typed with no expected type -- nsc's undetermined type
    /// variables. `Map.empty` in argument position is `Map[K, V]` with `K` and
    /// `V` undetermined; the call solves them, so they must not be compared as
    /// if they were fixed types.
    pub(crate) fn undetermined_of(&self, a: &Tree) -> Vec<SymbolId> {
        if a.sym.is_none() || a.ty.is_no_type() || a.ty.is_error() {
            return Vec::new();
        }
        // A residual method type is not a value yet; its own clauses are what
        // is missing, not an instantiation. The exception is a clause that is
        // *all implicit*: `Array.empty` is `(ClassTag[T])Array[T]`, and the
        // parameter it fills is the only thing that can say what `T` is, so
        // `T` is undetermined here exactly as `Map.empty`'s `K`/`V` are.
        let value_ty = match self.implicit_only_result(a) {
            Some(ret) => ret,
            None if matches!(a.ty, Type::Method { .. } | Type::Overload(_)) => return Vec::new(),
            None => a.ty.clone(),
        };
        self.st
            .get(a.sym)
            .tparams
            .iter()
            .copied()
            .filter(|tp| type_mentions_tparam(&value_ty, *tp) && !self.tparam_in_scope(*tp))
            .collect()
    }

    /// A type parameter an enclosing definition binds is a *type* here, not a
    /// variable: `def g[K](m: Map[K, Int]) = take(m)` has to keep reporting
    /// the mismatch, and `def rec[T](x: T): Map[T, Int] = take(rec(x))` must
    /// not "solve" its own `T` from the parameter it is passed to. Only a
    /// parameter of a method this argument has already applied -- one that can
    /// no longer be named here -- is undetermined.
    pub(crate) fn tparam_in_scope(&self, tp: SymbolId) -> bool {
        // A case-local existential stays rigid after its lexical case scope
        // is popped; minimising it to Nothing corrupts the join of branches.
        if self.st.get(tp).is_pattern_skolem {
            return true;
        }
        let name = self.st.get(tp).name.clone();
        self.st.lookup_type(&name).contains(&tp)
    }

    /// nsc's `isCompatible` with undetermined type variables in play: does
    /// `arg` become `param` for *some* instantiation of the variables it still
    /// carries? `Map[?K, ?V]` is compatible with `Map[String, Int]`; it is not
    /// compatible with `List[Int]`.
    ///
    /// Only the variables recorded in `undet_tvars` count. A type parameter of
    /// the enclosing method is a fixed type here, not a variable, so
    /// `def m[T](x: T) = take(x)` still has to report the mismatch.
    pub(crate) fn undet_compatible(&self, arg: &Type, param: &Type) -> bool {
        if self.undet_tvars.is_empty() || param.is_no_type() || param.is_error() {
            return false;
        }
        let open: Vec<SymbolId> = self
            .undet_tvars
            .iter()
            .copied()
            .filter(|tp| type_mentions_tparam(arg, *tp))
            .collect();
        if open.is_empty() {
            return false;
        }
        let mut solved = arg.clone();
        for tp in &open {
            let Some(t) = unify_one(*tp, arg, param) else {
                // The parameter pins nothing here (`def f(x: Any)` taking
                // `List.empty`). Leave the variable to its bound: an
                // undetermined variable is `Nothing` at worst, and `Nothing`
                // inhabits every type.
                let lo = self.st.get(*tp).bound_lo.clone().unwrap_or(Type::Nothing);
                solved = crate::symbol::subst_tparams_slice(&[*tp], &[lo], &solved);
                continue;
            };
            if t.is_no_type() || t.is_error() {
                return false;
            }
            // A variable is still bounded. `Ordering.empty[T <: Comparable[T]]`
            // handed to a `Seq[Int]` parameter is not compatible just because
            // the shapes line up.
            if let Some(hi) = self.st.get(*tp).bound_hi.clone() {
                if !mentions_any_tparam(&hi) && !self.st.is_sub_type(&t, &hi) {
                    return false;
                }
            }
            if let Some(lo) = self.st.get(*tp).bound_lo.clone() {
                if !mentions_any_tparam(&lo) && !self.st.is_sub_type(&lo, &t) {
                    return false;
                }
            }
            solved = crate::symbol::subst_tparams_slice(&[*tp], &[t], &solved);
        }
        self.st.is_sub_type(&solved, param)
    }

    /// Solve the variables an argument still carries from the parameter it
    /// fills, and rewrite the argument's type to the solution. This is where
    /// nsc's undetermined variables stop being variables: the alternative is
    /// picked, so nothing else is going to constrain them.
    ///
    /// The rewrite only happens when it makes the argument conform. A solution
    /// that does not is not an improvement -- the argument really is wrong,
    /// and the mismatch should describe what was written.
    pub(crate) fn instantiate_undet_arg(&mut self, a: &mut Tree, p: &Type) {
        if p.is_no_type() || p.is_error() || self.undet_tvars.is_empty() {
            return;
        }
        let open: Vec<SymbolId> = self
            .undet_tvars
            .iter()
            .copied()
            .filter(|tp| type_mentions_tparam(&a.ty, *tp))
            .collect();
        if open.is_empty() {
            return;
        }
        let mut solved = a.ty.clone();
        for tp in &open {
            let t = match unify_one(*tp, &a.ty, p) {
                Some(t) if !t.is_no_type() && !t.is_error() => t,
                // Nothing pins it. An unconstrained variable is its lower
                // bound, the same instantiation nsc's `solve` picks.
                _ => self.st.get(*tp).bound_lo.clone().unwrap_or(Type::Nothing),
            };
            solved = crate::symbol::subst_tparams_slice(&[*tp], &[t], &solved);
        }
        if self.st.is_sub_type(&solved, p) {
            a.ty = solved;
        }
    }

    /// Solve the variables that reached an application's *result* against the
    /// expected type. `def f[T](x: T): List[T]` applied to `Map.empty` has a
    /// result of `List[Map[?K, ?V]]`; a declared `List[Map[String, Int]]` is
    /// what says what `?K` and `?V` are.
    ///
    /// A variable the expected type does not pin is left alone -- it is still
    /// undetermined, and the call that encloses this one may yet fix it.
    pub(crate) fn solve_undet_result(&mut self, tree: &mut Tree, pt: &Type) {
        if pt.is_no_type()
            || pt.is_error()
            || tree.ty.is_no_type()
            || tree.ty.is_error()
            || self.undet_tvars.is_empty()
        {
            return;
        }
        let open: Vec<SymbolId> = self
            .undet_tvars
            .iter()
            .copied()
            .filter(|tp| type_mentions_tparam(&tree.ty, *tp))
            .collect();
        if open.is_empty() {
            return;
        }
        let mut solved = tree.ty.clone();
        for tp in &open {
            let Some(t) = unify_one(*tp, &tree.ty, pt) else {
                continue;
            };
            if t.is_no_type() || t.is_error() {
                continue;
            }
            solved = crate::symbol::subst_tparams_slice(&[*tp], &[t], &solved);
        }
        if self.st.is_sub_type(&solved, pt) {
            tree.ty = solved;
        }
    }

    /// The callee's type parameters that a parameter type still mentions
    /// because this call has not solved them yet -- the other half of nsc's
    /// undetermined type variables. `List.collect`'s `B` in
    /// `PartialFunction[A, B]` after `A` came from the receiver.
    /// `SymbolTable::lub` for the branches of an `if` or a `match`, with a
    /// variable nothing has pinned closed first.
    ///
    /// `if (c) Vector.empty else names.zip(kids).toVector` joins `Vector[?A]`
    /// with `Vector[(String, Int)]`. `?A` is undetermined, so joining the
    /// arguments walked all the way to `AnyRef` and the result conformed to
    /// nothing the source had written -- slick's `Node.getDumpInfo` built its
    /// `ch` this way and the whole method's inferred type became an error,
    /// which is what left `n.toString` an `<overload String | <error>>`.
    /// nsc's `solve` instantiates an unconstrained variable to its lower
    /// bound, and `Vector[Nothing]` *is* a `Vector[(String, Int)]`.
    ///
    /// Three conditions keep this narrow, so that only a leftover is closed:
    /// the parameter is *not in scope* here (an enclosing `def f[T]`'s own `T`
    /// is, and stays open), the other branch does not mention it (one both
    /// sides carry is still the enclosing call's to fix), and it occurs
    /// covariantly (an invariant occurrence has no bound to read it at).
    pub(crate) fn lub_branches(&self, a: &Type, b: &Type) -> Type {
        if a == b
            || matches!(a, Type::Nothing)
            || matches!(b, Type::Nothing)
            || a.is_no_type()
            || b.is_no_type()
            || a.is_error()
            || b.is_error()
            || self.st.is_sub_type(a, b)
            || self.st.is_sub_type(b, a)
        {
            return self.st.lub(a, b);
        }
        let closed = |ty: &Type, other: &Type| -> Type {
            // A bare *class* parameter is a real abstract type, not an
            // unsolved inference variable: `case List(x) => x` gives `x` the
            // type `A` of `List`, and `lub(Unit, A)` is `Any`. Closing it to
            // `Nothing` answered `Unit`, so
            // `def f(h: Any) = h match { case 5 => () ; case List(x) => x }`
            // came out returning `void` with every case's value discarded.
            //
            // A bare *method* parameter that reached here is the other thing:
            // `Iterator.empty.next()` leaves `T` of `def empty[T]` unsolved
            // where nsc has already minimised it to `Nothing`, and closing it
            // is how that is recovered. (A method parameter still in lexical
            // scope is excluded below, by name.)
            if let Type::TypeParam(tp) = ty {
                if self.st.get(self.st.get(*tp).owner).kind == SymKind::Class {
                    return ty.clone();
                }
            }
            let mut open = Vec::new();
            collect_tparams(ty, &mut open);
            let mut out = ty.clone();
            for tp in open {
                if type_mentions_tparam(other, tp)
                    || self.st.get(tp).kind != SymKind::TypeParam
                    || self.tparam_in_scope(tp)
                    || self.tparam_variance_in(&out, tp, 1) != Some(1)
                {
                    continue;
                }
                let lo = self.st.get(tp).bound_lo.clone().unwrap_or(Type::Nothing);
                out = crate::symbol::subst_tparams_slice(&[tp], &[lo], &out);
            }
            out
        };
        // And the answer is still one of the two branch types: closing may
        // only reveal that one of them already *is* the join. Anything else
        // stays with the ordinary walk rather than invent a `Nothing` the
        // source never wrote.
        let ca = closed(a, b);
        if ca != *a && self.st.is_sub_type(&ca, b) {
            return b.clone();
        }
        let cb = closed(b, a);
        if cb != *b && self.st.is_sub_type(&cb, a) {
            return a.clone();
        }
        let plain = self.st.lub(a, b);
        // Neither branch is the join, and the ordinary walk gave up. nsc
        // *minimises* each side's own undetermined variables before it joins
        // them (`solvedTypes`: a variable with no upper constraint is its
        // lower bound), and joining an open variable against a real type is
        // exactly what walks out to `AnyRef`. cats' `nonEmptyPartition`
        //
        // ```scala
        // val lastIor = f(reversed.head) match {
        //   case Right(c) => Ior.right(NonEmptyList.one(c))   // Ior[?A, NEL[C]]
        //   case Left(b)  => Ior.left(NonEmptyList.one(b))    // Ior[NEL[B], ?B]
        // }
        // ```
        //
        // was `Ior[AnyRef, AnyRef]`, and every use of `lastIor` after it read
        // `value :: is not a member of AnyRef` (`NonEmptyList.scala`, and the
        // same three lines in `NonEmptySeq` and `NonEmptyVector`).
        //
        // Only a *tighter* upper bound is taken: the minimised join has to
        // conform to the one the ordinary walk found, so this can never widen
        // an answer, only stop it from climbing past the type both branches
        // were already written at.
        if ca != *a || cb != *b {
            let closed_join = self.st.lub(&ca, &cb);
            if closed_join != plain
                && !matches!(closed_join, Type::Nothing)
                && self.st.is_sub_type(&closed_join, &plain)
            {
                return closed_join;
            }
        }
        plain
    }

    /// The type an `if` or a `match` takes: [`pt_or_lub`], except that an
    /// expected type which is still a stand-in for an undetermined variable
    /// ([`pt_is_undecided`]) does not get to be the answer. Adopting `F[_]`
    /// there is what stopped `F.flatMap(value) { case … }` from ever deciding
    /// `flatMap`'s `Y` -- the argument that was supposed to decide it said
    /// `Y = _` instead.
    ///
    /// Two ways out, in order:
    ///
    /// * fill the stand-in positions from the branches
    ///   ([`Self::fill_undecided`]) -- this is what nsc's `solve` does, taking
    ///   the lub of the *lower bounds* a variable collected, rather than a lub
    ///   of whole types;
    /// * failing that, the branch lub, when it is a real type that already
    ///   conforms to `pt`.
    pub(crate) fn branch_result_ty(&self, pt: &Type, branch_tys: &[Type], joined: Type) -> Type {
        if pt_is_undecided(pt) {
            if let Some(filled) = self.fill_undecided(pt, branch_tys) {
                return filled;
            }
            if !joined.is_no_type()
                && !joined.is_error()
                && !matches!(joined, Type::Nothing)
                && self.st.is_sub_type(&joined, pt)
            {
                return joined;
            }
        }
        pt_or_lub(pt, joined)
    }

    /// Replace the `Wildcard` stand-ins in `pt` with the join of what the
    /// branches put in those positions.
    ///
    /// nsc never joins the branch types here: the expected type is a real type
    /// variable, each branch adds a lower bound to it, and `solve` takes the
    /// lub of *those*. So cats' `EitherT.orElse`
    ///
    /// ```text
    /// EitherT(F.flatMap(value) {
    ///   case Left(_)      => default.value          // F[Either[C, BB]]
    ///   case r @ Right(_) => F.pure(leftCast(r))    // F[Right[C, BB]]
    /// })
    /// ```
    ///
    /// gives `?Y = Either[C, BB]`, which is what `EitherT`'s constructor then
    /// takes. Joining the two *applications* cannot reach that answer: `F` is
    /// an abstract constructor whose parameter is invariant, so the lub of
    /// `F[Either[…]]` and `F[Right[…]]` is at best an existential, and here it
    /// was not even that -- `SymbolTable::lub` has no arm for `Type::Applied`
    /// and walked out to `AnyRef`.
    ///
    /// Only the stand-in positions are filled. Whatever `pt` already says
    /// stays, so the answer is still the expected type, just decided.
    fn fill_undecided(&self, pt: &Type, branch_tys: &[Type]) -> Option<Type> {
        let usable =
            |t: &Type| !t.is_no_type() && !t.is_error() && !matches!(t, Type::Nothing | Type::Null);
        if matches!(pt, Type::Wildcard) {
            let mut acc: Option<Type> = None;
            for b in branch_tys.iter().filter(|t| usable(t)) {
                acc = Some(match acc {
                    None => b.clone(),
                    Some(prev) => self.lub_branches(&prev, b),
                });
            }
            return acc.filter(usable);
        }
        if !pt_is_undecided(pt) {
            return Some(pt.clone());
        }
        // Every branch has to be an application of the same constructor, or
        // there is nothing to read the argument off.
        let pt_args = match pt {
            Type::Class { args, .. } | Type::Applied { args, .. } => args,
            _ => return None,
        };
        let same_head = |b: &Type| -> Option<Vec<Type>> {
            match (pt, b) {
                (Type::Class { sym: s1, .. }, Type::Class { sym: s2, args }) if s1 == s2 => {
                    Some(args.clone())
                }
                (Type::Applied { ctor: c1, .. }, Type::Applied { ctor: c2, args }) if c1 == c2 => {
                    Some(args.clone())
                }
                _ => None,
            }
        };
        let mut per_branch = Vec::new();
        for b in branch_tys.iter().filter(|t| usable(t)) {
            let args = same_head(b)?;
            if args.len() != pt_args.len() {
                return None;
            }
            per_branch.push(args);
        }
        if per_branch.is_empty() {
            return None;
        }
        let mut out_args = Vec::with_capacity(pt_args.len());
        for (i, a) in pt_args.iter().enumerate() {
            if !pt_is_undecided(a) && !matches!(a, Type::Wildcard) {
                out_args.push(a.clone());
                continue;
            }
            let nested: Vec<Type> = per_branch.iter().map(|args| args[i].clone()).collect();
            out_args.push(self.fill_undecided(a, &nested)?);
        }
        Some(match pt {
            Type::Class { sym, .. } => Type::Class {
                sym: *sym,
                args: out_args,
            },
            Type::Applied { ctor, .. } => Type::Applied {
                ctor: ctor.clone(),
                args: out_args,
            },
            _ => return None,
        })
    }

    pub(crate) fn open_tparams_of(&self, p: &Type, own: Option<&[SymbolId]>) -> Vec<SymbolId> {
        let Some(own) = own else { return Vec::new() };
        own.iter()
            .copied()
            .filter(|tp| type_mentions_tparam(p, *tp))
            .collect()
    }

    /// A parameter type with its undetermined variables opened up to their
    /// declared bounds, which is what an unsolved variable means as an
    /// *expected* type: it constrains nothing beyond the bound. This is only
    /// ever the expected type handed to an argument, never a result.
    pub(crate) fn open_to_bounds(&self, p: &Type, open: &[SymbolId]) -> Type {
        if open.is_empty() {
            return p.clone();
        }
        let mut out = p.clone();
        for tp in open {
            // A *type constructor* has no bound that is a type. `Any` is not
            // one of its inhabitants -- it is not even the same kind -- so
            // slick's `flatMap[F, T, D[_]](f: E => Query[F, T, D])` reached
            // the lambda as `Query[F, T, Any]` and its `Query[G, T, Seq]` body
            // was `found: Query[G, T, Seq]  required: Query[G, T, Any]`. A
            // wildcard is what "some constructor, not yet decided" means in a
            // position `is_sub_type` already understands.
            if !self.st.get(*tp).tparams.is_empty() {
                out = crate::symbol::subst_tparams_slice(&[*tp], &[Type::Wildcard], &out);
                continue;
            }
            let hi = self.st.get(*tp).bound_hi.clone().unwrap_or(Type::Any);
            // A bound written in terms of the other open variables says
            // nothing more than `Any` does here.
            let hi = if mentions_tparam(&hi, open) {
                Type::Any
            } else {
                hi
            };
            out = crate::symbol::subst_tparams_slice(&[*tp], &[hi], &out);
        }
        out
    }

    /// Whether an expected *tuple* type constrains nothing, because every one
    /// of its components is an undetermined variable opened up to `Any`.
    ///
    /// `open_to_bounds` erases a variable structurally: `SortedMapOps.collect
    /// [K2, V2](pf: PartialFunction[(K, V), (K2, V2)])(implicit Ordering[K2])`
    /// reaches the literal as `PartialFunction[(Int, String), (Any, Any)]`.
    /// A *bare* variable is already recognised and left open, so the case
    /// bodies decide it; a variable inside a tuple was not, and `(k, v.length)`
    /// came back `(Any, Any)` -- which then asked for `Ordering[Any]`.
    ///
    /// Only tuples: a component of one is always a reference, so leaving the
    /// position open cannot drop a boxing an expected `Any` would have forced.
    pub(crate) fn pt_says_nothing(&self, ty: &Type) -> bool {
        let Some(args) = self.as_tuple_args(ty) else {
            return false;
        };
        !args.is_empty()
            && args.iter().all(|a| {
                matches!(a, Type::Any | Type::Wildcard | Type::TypeParam(_))
                    || self.pt_says_nothing(a)
            })
    }

    /// Solve a parameter's undetermined variables from the argument that fills
    /// it, so the argument is checked against `PartialFunction[Int, String]`
    /// rather than against an erased `PartialFunction[Int, Any]`.
    ///
    /// `None` when the argument does not pin every one of them; the call then
    /// falls back to the bounds, and whatever is left undetermined is reported
    /// by the usual mismatch rather than papered over.
    pub(crate) fn solve_open_from_arg(
        &self,
        arg: &Type,
        p: &Type,
        open: &[SymbolId],
    ) -> Option<Type> {
        if open.is_empty() || arg.is_no_type() || arg.is_error() {
            return None;
        }
        let mut out = p.clone();
        for tp in open {
            let t = unify_one(*tp, p, arg)?;
            if t.is_no_type() || t.is_error() {
                return None;
            }
            out = crate::symbol::subst_tparams_slice(&[*tp], &[t], &out);
        }
        (!mentions_tparam(&out, open)).then_some(out)
    }

    /// nsc's `dependentTypeMap`. `def get[P <: Phase](p: P): Option[p.State]`
    /// reads `State` off the *argument*, not off `Phase`, so
    /// `get(Phase.assignUniqueSymbols)` is an `Option[UsedFeatures]` and not an
    /// `Option[Phase#State]` that degrades to `Any`.
    ///
    /// The type carries no prefix, so the parameter that could have been one is
    /// found by its bound: only when exactly one parameter's type has the
    /// member's owner as a base class is that argument the prefix. An argument
    /// that leaves the member abstract changes nothing.
    pub(crate) fn subst_dependent_members(
        &self,
        param_tys: &[Type],
        arg_tys: &[Type],
        ret: &Type,
    ) -> Type {
        let mut members = Vec::new();
        crate::symbol::collect_type_members(ret, &mut members);
        if members.is_empty() {
            return ret.clone();
        }
        let mut out = ret.clone();
        for m in members {
            let info = self.st.get(m);
            if !info.tparams.is_empty() {
                continue;
            }
            let owner = info.owner;
            let name = info.name.clone();
            if owner.is_none() || !self.st.get(owner).is_class_like() {
                continue;
            }
            let mut cand = None;
            for (i, p) in param_tys.iter().enumerate() {
                let base = match p {
                    Type::TypeParam(id) => match &self.st.get(*id).bound_hi {
                        Some(hi) => hi.clone(),
                        None => continue,
                    },
                    other => other.clone(),
                };
                if self.base_type_instance(&base, owner, 0).is_none() {
                    continue;
                }
                if cand.is_some() {
                    cand = None;
                    break;
                }
                cand = Some(i);
            }
            let Some(i) = cand else { continue };
            let Some(acls) = arg_tys.get(i).and_then(|a| self.st.class_sym_of(a)) else {
                continue;
            };
            let Some(found) = self
                .st
                .lookup_member(acls, &name)
                .into_iter()
                .find(|&s| self.st.get(s).kind == SymKind::TypeMember)
            else {
                continue;
            };
            if found == m {
                continue;
            }
            let seen = self.st.dealias(&Type::TypeMember(found));
            if seen.is_no_type()
                || seen.is_error()
                || matches!(&seen, Type::TypeMember(x) if *x == m)
            {
                continue;
            }
            out = crate::symbol::subst_type_member(&out, m, &seen);
        }
        out
    }

    /// nsc's `solvedTypes` for what a call leaves over. A type parameter that
    /// occurs in the result but in *no* parameter type is one no argument
    /// could ever pin; once the expected type has had its say, nsc instantiates
    /// it to a bound -- the lower one where it occurs covariantly, the upper
    /// one where it occurs contravariantly. Leaving it a parameter is what made
    /// `dbAction { … }` checked against `FixedBasicAction[Unit, Nothing,
    /// Effect.Schema]` report `found: FixedBasicAction[Unit, S, Schema]`.
    ///
    /// A parameter that *does* occur in a parameter type is left alone: that is
    /// the one the arguments (or a still-missing implicit) are supposed to
    /// determine, and papering over it would hide the diagnostic.
    ///
    /// `nargs` is how many arguments the call actually passed, because a
    /// repeated parameter that got none is not one an argument can pin.
    pub(crate) fn instantiate_leftover_tparams(
        &self,
        method: SymbolId,
        ret: Type,
        pt: &Type,
        nargs: usize,
    ) -> Type {
        // Only where the call is what the expected type is checked against.
        // With none, this is a receiver a further application may still solve
        // (nsc keeps those in `Context.undetparams`).
        if method.is_none() || pt.is_no_type() || pt.is_error() {
            return ret;
        }
        let tps = self.st.get(method).tparams.clone();
        if tps.is_empty() || !mentions_tparam(&ret, &tps) {
            return ret;
        }
        // A repeated parameter the call left empty solves nothing: `List()`
        // has no element to read `A` out of, so `A` is unconstrained and nsc
        // minimises it to `Nothing`. Leaving it in `sig_params` made every
        // empty factory call -- `List()`, `Seq()`, `Map()` -- keep the
        // callee's own parameter and fail to conform to anything.
        let sig_params: Vec<Type> = match &self.st.get(method).ty {
            Type::Method { paramss, .. } => paramss
                .iter()
                .enumerate()
                .flat_map(|(i, clause)| {
                    clause
                        .iter()
                        .enumerate()
                        .filter(move |(j, p)| {
                            !(i == 0 && *j >= nargs && matches!(p, Type::Repeated(_)))
                        })
                        .map(|(_, p)| p.clone())
                })
                .collect(),
            _ => Vec::new(),
        };
        let mut out = ret;
        for tp in tps {
            if !mentions_tparam(&out, &[tp]) {
                continue;
            }
            if sig_params.iter().any(|p| mentions_tparam(p, &[tp])) {
                continue;
            }
            // A type *constructor* has no bound we could name: `Nothing` is not
            // an `F[_]`, and putting it there gave `ActionListener[Nothing]`.
            if !self.st.get(tp).tparams.is_empty() {
                continue;
            }
            let v = self.tparam_variance_in(&out, tp, 1).unwrap_or(0);
            let info = self.st.get(tp);
            let bound = if v < 0 {
                info.bound_hi.clone().unwrap_or(Type::Any)
            } else {
                info.bound_lo.clone().unwrap_or(Type::Nothing)
            };
            if bound.is_no_type() || bound.is_error() || mentions_tparam(&bound, &[tp]) {
                continue;
            }
            out = crate::symbol::subst_tparams_slice(&[tp], &[bound], &out);
        }
        out
    }

    /// Variance of `tp`'s occurrences in `ty`, or `None` when it does not
    /// occur. Two occurrences that disagree make the parameter invariant.
    pub(crate) fn tparam_variance_in(&self, ty: &Type, tp: SymbolId, variance: i8) -> Option<i8> {
        let merge = |a: Option<i8>, b: Option<i8>| match (a, b) {
            (None, x) | (x, None) => x,
            (Some(x), Some(y)) if x == y => Some(x),
            _ => Some(0),
        };
        match ty {
            Type::TypeParam(id) if *id == tp => Some(variance),
            Type::Class { sym, args } => {
                let tparams = self.st.get(*sym).tparams.clone();
                let mut out = None;
                for (i, a) in args.iter().enumerate() {
                    let v = tparams
                        .get(i)
                        .map(|&t| {
                            let f = self.st.get(t).flags;
                            if f.contains(Flags::COVARIANT) {
                                1
                            } else if f.contains(Flags::CONTRAVARIANT) {
                                -1
                            } else {
                                0
                            }
                        })
                        .unwrap_or(0);
                    out = merge(
                        out,
                        self.tparam_variance_in(a, tp, compose_variance(variance, v)),
                    );
                }
                out
            }
            Type::Tuple(ts) => ts.iter().fold(None, |acc, t| {
                merge(acc, self.tparam_variance_in(t, tp, variance))
            }),
            Type::Function { params, ret } => {
                let mut out = self.tparam_variance_in(ret, tp, variance);
                for p in params {
                    out = merge(out, self.tparam_variance_in(p, tp, flip_variance(variance)));
                }
                out
            }
            Type::Array(e) => self.tparam_variance_in(e, tp, 0),
            Type::ByName(e) | Type::Repeated(e) => self.tparam_variance_in(e, tp, variance),
            Type::Annotated { tpe, .. } => self.tparam_variance_in(tpe, tp, variance),
            Type::Applied { ctor, args } => {
                let mut out = self.tparam_variance_in(ctor, tp, 0);
                for a in args {
                    out = merge(out, self.tparam_variance_in(a, tp, 0));
                }
                out
            }
            _ => None,
        }
    }

    pub(crate) fn maybe_auto_apply(&self, ty: Type, pt: &Type) -> Type {
        match &ty {
            Type::Method { paramss, ret }
                if paramss.is_empty() || paramss.iter().all(|c| c.is_empty()) =>
            {
                // Eta-expansion is for methods that *take* parameters: `def
                // f(): T` may become `() => T`, a parameterless `def f: T` may
                // not. Declining to apply the latter turned `val g: Int => Int
                // = fs.head` into `() => (Int => Int)`. A `Method` expected
                // type is not a value position at all -- it marks the callee
                // of an `Apply`, which must stay a method.
                let eta = !paramss.is_empty() && matches!(pt, Type::Function { .. });
                if eta || matches!(pt, Type::Method { .. }) {
                    ty
                } else {
                    (**ret).clone()
                }
            }
            Type::Overload(alts) => {
                // Alternatives that are the same type are one member, not an
                // overload: a member reaches a class through every inheritance
                // route it has, and a diamond yields it twice.
                let mut distinct: Vec<&Type> = Vec::new();
                for a in alts {
                    if !distinct.contains(&a) {
                        distinct.push(a);
                    }
                }
                if let [only] = distinct.as_slice() {
                    return self.maybe_auto_apply((*only).clone(), pt);
                }
                if matches!(pt, Type::Function { .. } | Type::Method { .. }) {
                    return ty;
                }
                let nullary: Vec<&Type> = alts
                    .iter()
                    .filter(|a| match a {
                        Type::Method { paramss, .. } => {
                            paramss.is_empty() || paramss.iter().all(|c| c.is_empty())
                        }
                        _ => false,
                    })
                    .collect();
                if let [Type::Method { ret, .. }] = nullary.as_slice() {
                    return (**ret).clone();
                }
                // nsc (SLS 6.26.3): an overloaded reference in value position
                // keeps only the alternatives that take no parameters. A `val`
                // is not a method type at all, so `object Lib { val == = … }`
                // reads as the value, not as the inherited `Any.==(x: Any)`.
                let mut values = alts.iter().filter(|a| !matches!(a, Type::Method { .. }));
                match (values.next(), values.next()) {
                    (Some(v), None) if nullary.is_empty() => v.clone(),
                    _ => ty,
                }
            }
            Type::ByName(inner) => {
                if matches!(
                    pt,
                    Type::Function { .. } | Type::ByName(_) | Type::Method { .. }
                ) {
                    ty
                } else {
                    (**inner).clone()
                }
            }
            _ => ty,
        }
    }

    pub(crate) fn is_nullary_method_sym(&self, id: SymbolId) -> bool {
        match &self.st.get(id).ty {
            Type::Method { paramss, .. } => {
                paramss.is_empty() || paramss.iter().all(|c| c.is_empty())
            }
            _ => false,
        }
    }

    /// The alternatives `maybe_auto_apply` keeps in value position: a nullary
    /// method or a `val`/`object` whose type is not a method type at all.
    pub(crate) fn is_parameterless_sym(&self, id: SymbolId) -> bool {
        !matches!(&self.st.get(id).ty, Type::Method { .. }) || self.is_nullary_method_sym(id)
    }

    /// The one alternative an overloaded reference in *value* position keeps
    /// when its parameters are all implicit -- nsc's `inferExprAlternative`
    /// for a shape [`Typer::maybe_auto_apply`] cannot see.
    ///
    /// `Type::Method` carries parameter *types* and no flags, so nothing in
    /// the type says which clause is implicit; only the parameter symbols do,
    /// which is why this takes the alternatives with their symbols rather than
    /// living in `maybe_auto_apply` beside the nullary rule.
    ///
    /// nsc's `isAsSpecific` looks straight through an implicit clause
    /// (`case mt: MethodType if mt.isImplicit => isAsSpecific(mt.resultType,
    /// ftpe2)`), and its mirror case answers `!mt.isImplicit` for a value type
    /// weighed against a method that takes explicit parameters. So
    /// `(implicit r: R)P` is as specific as such an alternative while that one
    /// is not as specific as it, and `isStrictlyMoreSpecific` picks it
    /// outright. scalatra declares exactly that pair:
    ///
    /// ```scala
    /// def params(implicit request: HttpServletRequest): Params
    /// def params(key: String)(implicit request: HttpServletRequest): String
    /// ```
    ///
    /// and `params.get("id")` looked `get` up on the unresolved set --
    /// `value get is not a member of <overload (String)(HttpServletRequest)
    /// String | (HttpServletRequest)MultiMapHeadView[String, String]>`, the
    /// largest family left in the gitbucket measurement, repeated for
    /// `flash`, `session` and `multiParams`.
    ///
    /// The clause is *not* stripped: the witness still has to be searched for
    /// and passed, which [`Typer::adapt_implicit_apply`] does once the
    /// reference names a single alternative. That is also why a candidate must
    /// have exactly one clause -- `adapt_implicit_apply` fills the first and
    /// only the first, and resolving to an alternative whose second clause
    /// nothing would fill trades one diagnostic for a worse one.
    ///
    /// Answers `None` when some alternative takes no parameters at all (that
    /// one is at least as specific, and `maybe_auto_apply` has already had its
    /// say), and when two distinct alternatives are implicit-only -- nsc
    /// reports that as an ambiguous reference, and leaving the set standing
    /// keeps the diagnostic scala-rs already gives.
    pub(crate) fn implicit_only_alternative(&self, alts: &[(SymbolId, Type)]) -> Option<SymbolId> {
        let mut only: Option<SymbolId> = None;
        for (s, t) in alts {
            if self.is_parameterless_sym(*s) || !matches!(t, Type::Method { .. }) {
                return None;
            }
            let decl = &self.st.get(*s).paramss;
            if decl.len() != 1
                || !decl
                    .iter()
                    .flatten()
                    .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
            {
                continue;
            }
            match only {
                // The same declaration reached through two parents is one
                // alternative, not two.
                Some(prev) if prev == *s => {}
                Some(_) => return None,
                None => only = Some(*s),
            }
        }
        only
    }

    /// The result of an argument that is still a method whose only remaining
    /// clause is implicit (`Array.empty` is `(ClassTag[T])Array[T]` until the
    /// expected type says what `T` is). `None` for anything else, including a
    /// method value that is genuinely being eta-expanded.
    pub(crate) fn implicit_only_result(&self, tree: &Tree) -> Option<Type> {
        let Type::Method { paramss, ret } = &tree.ty else {
            return None;
        };
        if paramss.len() != 1 || paramss[0].is_empty() || tree.sym.is_none() {
            return None;
        }
        let first = self.st.get(tree.sym).paramss.first().cloned()?;
        if first.len() != paramss[0].len()
            || !first
                .iter()
                .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
        {
            return None;
        }
        Some((**ret).clone())
    }

    /// nsc's `adaptToImplicitMethod`: the undetermined type parameters of an
    /// expression whose only remaining clause is implicit are instantiated
    /// *before* the witness is searched (`inferExprInstance` with
    /// `keepNothings = false`). A variable that would come out `Nothing` is
    /// kept undetermined -- that is what leaves `take(Array.empty)` for the
    /// parameter to settle -- but one with a real lower bound is instantiated
    /// *at* that bound, and stops being a variable at all.
    ///
    /// slick's `ConstArray#toArray[R >: T : ClassTag]: Array[R]` is the case
    /// that needs it: `session.withPreparedInsertStatement(sql,
    /// keyColumns.toArray)` weighs `(String, Array[String])` against
    /// `(String, Array[Int])`, and an argument left as `Array[R]` fits both
    /// (`jdbc/JdbcActionComponent.scala:725`, "ambiguous overload").
    pub(crate) fn solve_lower_bounded_undet(&mut self, a: &mut Tree) {
        if a.sym.is_none() || self.implicit_only_result(a).is_none() {
            return;
        }
        // The bound is written in the *declaration*'s type parameters
        // (`R >: T`); what this call sees is that bound at the receiver.
        let recv = match &a.kind {
            TreeKind::Select { qual, .. } => Some(qual.ty.clone()),
            _ => None,
        };
        let mut ids: Vec<SymbolId> = Vec::new();
        let mut vals: Vec<Type> = Vec::new();
        for tp in self.st.get(a.sym).tparams.clone() {
            if !type_mentions_tparam(&a.ty, tp) || self.tparam_in_scope(tp) {
                continue;
            }
            let Some(lo) = self.st.get(tp).bound_lo.clone() else {
                continue;
            };
            let lo = match &recv {
                Some(r) if !r.is_no_type() && !r.is_error() => self.st.subst_as_seen_from(r, &lo),
                _ => lo,
            };
            // `Nothing` is the "no constraint" answer nsc keeps open, and a
            // bound that is still a type parameter nothing here can name says
            // the receiver did not substitute it.
            if matches!(lo, Type::Nothing | Type::NoType | Type::Error)
                || type_mentions_tparam(&lo, tp)
                || matches!(lo, Type::TypeParam(id) if !self.tparam_in_scope(id))
            {
                continue;
            }
            ids.push(tp);
            vals.push(lo);
        }
        if ids.is_empty() {
            return;
        }
        a.ty = crate::symbol::subst_tparams_slice(&ids, &vals, &a.ty);
    }

    /// `implicitly[Int]` is a TypeApply of a method whose remaining clause is
    /// implicit; rewrite to an Apply filled from implicit search.
    pub(crate) fn adapt_implicit_apply(&mut self, tree: &mut Tree, pt: &Type) {
        if matches!(pt, Type::Method { .. } | Type::Function { .. }) {
            return;
        }
        if tree.sym.is_none() {
            return;
        }
        let paramss = self.st.get(tree.sym).paramss.clone();
        let first = paramss.first().cloned().unwrap_or_default();
        if first.is_empty() {
            return;
        }
        if !first
            .iter()
            .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
        {
            return;
        }
        if !matches!(&tree.ty, Type::Method { .. }) {
            return;
        }
        // Wait for TypeApply when the method still has unsubstituted tparams —
        // otherwise `implicitly[Int]` types the Ident first and searches for
        // `T`. The exception is nsc's *undetermined* parameters: when every
        // one of them sits strictly inside an implicit parameter's type
        // (`toMap[K, V](implicit ev: A <:< (K, V))`), only the witness can pin
        // them down, and the search does it.
        let undet = self.undetermined_tparams(tree, &first);
        let span = tree.span;
        let ret = match &tree.ty {
            Type::Method { ret, .. } => (**ret).clone(),
            _ => return,
        };
        // The expected type pins the parameters the implicit search would
        // otherwise have to guess: `take(Array.empty)` on
        // `take(a: Array[String])` is `T = String`, so the search is for
        // `ClassTag[String]` and not for an arbitrary `ClassTag[T]`.
        let from_pt: Vec<(SymbolId, Type)> = self
            .add_expected_constraints_in(tree.sym, &ret, pt, Vec::new(), true)
            .into_iter()
            .filter(|(_, t)| !t.is_no_type() && !t.is_error() && !matches!(t, Type::TypeParam(_)))
            .collect();
        // Whether this method type has already had its own parameters
        // substituted away. Asking only "does the clause still mention them"
        // is not enough: `type_mentions_tparam` does not look inside a
        // compound type, so slick's `BaseColumnType[U]` (=
        // `ScalaType[U] with BaseTypedType[U]`) reads as mentioning nothing
        // and the search would run at an unsubstituted `U` (fixture `ovl4`).
        // Comparing against the *declaration* keeps that case waiting and lets
        // through only the one where a parameter demonstrably went away --
        // `keyColumns.toArray`'s `R`, instantiated from its lower bound by
        // `solve_lower_bounded_undet`.
        let tps_of = self.st.get(tree.sym).tparams.clone();
        let decl_ty = self.st.get(tree.sym).ty.clone();
        let already_substituted = tps_of.iter().any(|tp| type_mentions_tparam(&decl_ty, *tp))
            && !tps_of.iter().any(|tp| type_mentions_tparam(&tree.ty, *tp));
        if !already_substituted
            && !self.st.get(tree.sym).tparams.is_empty()
            && !matches!(tree.kind, TreeKind::TypeApply { .. })
            && undet.is_empty()
        {
            // `TreeMap.empty` is `[K: Ordering, V]: TreeMap[K, V]`: `V` is in
            // no implicit parameter, so the search alone cannot pin the
            // parameters -- but `val m: TreeMap[Long, String] = TreeMap.empty`
            // does. nsc runs `inferExprInstance` against the expected type and
            // only then searches. Waiting for a `TypeApply` that never comes
            // left the whole method type standing as the value's type.
            let implicit_tps: Vec<SymbolId> = self
                .st
                .get(tree.sym)
                .tparams
                .iter()
                .copied()
                .filter(|tp| match &tree.ty {
                    Type::Method { paramss, .. } => paramss
                        .first()
                        .is_some_and(|ps| ps.iter().any(|t| type_mentions_tparam(t, *tp))),
                    _ => false,
                })
                .collect();
            let pinned_by_pt = !implicit_tps.is_empty()
                && implicit_tps
                    .iter()
                    .all(|tp| from_pt.iter().any(|(id, _)| id == tp));
            if !pinned_by_pt {
                return;
            }
        }
        // An argument being typed before the alternative is picked: leave the
        // clause alone. The parameter it fills is what says what the
        // undetermined parameters are, and this pass has no expected type at
        // all -- committing here picks a witness for `take(empty)` rather than
        // reporting the one the parameter really asks for.
        if self.typing_call_args && !undet.is_empty() && pt.is_no_type() {
            return;
        }
        let (ret, tree_ty) = if from_pt.is_empty() {
            (ret, tree.ty.clone())
        } else {
            let ids: Vec<SymbolId> = from_pt.iter().map(|(i, _)| *i).collect();
            let ts: Vec<Type> = from_pt.iter().map(|(_, t)| t.clone()).collect();
            (
                crate::symbol::subst_tparams_slice(&ids, &ts, &ret),
                crate::symbol::subst_tparams_slice(&ids, &ts, &tree.ty),
            )
        };
        tree.ty = tree_ty;
        let tys: Vec<Type> = first
            .iter()
            .map(|id| match &tree.ty {
                Type::Method { paramss, .. } => paramss
                    .first()
                    .and_then(|ps| ps.get(first.iter().position(|x| x == id).unwrap_or(0)))
                    .cloned()
                    .unwrap_or_else(|| self.st.get(*id).ty.clone()),
                _ => self.st.get(*id).ty.clone(),
            })
            .collect();
        // With undetermined parameters, commit only when the witness really
        // pins every one of them down. Otherwise leave the tree alone: a
        // `show[Int]` still has its `TypeApply` coming. A parameter the
        // expected type has already fixed is no longer one of them.
        let undet: Vec<SymbolId> = undet
            .into_iter()
            .filter(|tp| tys.iter().any(|t| type_mentions_tparam(t, *tp)))
            .collect();
        let mut solved: Vec<(SymbolId, Type)> = Vec::new();
        let mut tys = tys;
        let mut ret = ret;
        if !undet.is_empty() {
            // `undet_solution` searches under an immutable borrow and cannot
            // load a companion itself.
            for t in tys.clone() {
                self.warm_implicit_scope(&t);
            }
            let mut solution = self.undet_solution(&tys, &undet);
            if solution.is_none() && self.warm_implicit_candidates(&tys) {
                // A witness whose class came from a jar answers only once its
                // pickled/JVM parents have been read: `implicit F: Async[F]`
                // is a `GenTemporal[F, Throwable]` through three levels of
                // `extends`, and that chain is what says `E = Throwable` for
                // `timeoutTo[B, E](…)(implicit F: GenTemporal[F, E])`. Left
                // unsolved, the parameter reached the search as
                // `GenTemporal[F, _]` and nothing matched
                // (`slick/basic/ConcurrencyControl.scala`).
                solution = self.undet_solution(&tys, &undet);
            }
            let Some(sol) = solution else {
                return;
            };
            let ids: Vec<SymbolId> = sol.iter().map(|(i, _)| *i).collect();
            let ts: Vec<Type> = sol.iter().map(|(_, t)| t.clone()).collect();
            tys = tys
                .iter()
                .map(|t| crate::symbol::subst_tparams_slice(&ids, &ts, t))
                .collect();
            ret = crate::symbol::subst_tparams_slice(&ids, &ts, &ret);
            solved = sol;
        }
        let mut args = Vec::new();
        self.fill_implicit_params(span, &mut args, &tys, &first);
        let mut inner = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
        if !solved.is_empty() {
            let ids: Vec<SymbolId> = solved.iter().map(|(i, _)| *i).collect();
            let ts: Vec<Type> = solved.iter().map(|(_, t)| t.clone()).collect();
            inner.ty = crate::symbol::subst_tparams_slice(&ids, &ts, &inner.ty);
        }
        let id = inner.id;
        let sym = inner.sym;
        *tree = Tree {
            id,
            span,
            kind: TreeKind::Apply {
                fun: Box::new(inner),
                args,
            },
            ty: ret,
            sym,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
    }

    /// Solve `undet` from the implicit parameter types alone, without emitting
    /// anything. `None` when a parameter has no (or no unique) witness, or when
    /// the witness leaves a parameter open — the caller then leaves the tree be.
    pub(crate) fn undet_solution(
        &self,
        param_tys: &[Type],
        undet: &[SymbolId],
    ) -> Option<Vec<(SymbolId, Type)>> {
        let mut solved: Vec<(SymbolId, Type)> = Vec::new();
        for pty in param_tys {
            let ids: Vec<SymbolId> = solved.iter().map(|(i, _)| *i).collect();
            let ts: Vec<Type> = solved.iter().map(|(_, t)| t.clone()).collect();
            let pty = crate::symbol::subst_tparams_slice(&ids, &ts, pty);
            let open: Vec<SymbolId> = undet
                .iter()
                .copied()
                .filter(|u| solved.iter().all(|(s, _)| s != u))
                .collect();
            let (found, bindings) = self.search_implicit_undet(&pty, &open, 0);
            if !found.is_found() {
                // No *value* of that type. A function-typed parameter is a
                // view request, and the conversion that answers it can pin the
                // open parameters just as well (`List[Option[A]].flatten`).
                let Some(view) = self.view_undet_bindings(&pty, &open) else {
                    return None;
                };
                solved.extend(view);
                continue;
            }
            solved.extend(bindings);
        }
        undet
            .iter()
            .all(|u| solved.iter().any(|(s, _)| s == u))
            .then_some(solved)
    }

    /// nsc's undetermined type parameters: those of `tree.sym` that appear
    /// *strictly inside* an implicit parameter's type, so the implicit search
    /// is the only thing that can solve them. A parameter whose type is the
    /// bare type parameter itself (`implicitly[T](implicit e: T)`) is not one:
    /// every implicit in scope would match it.
    fn undetermined_tparams(&self, tree: &Tree, first: &[SymbolId]) -> Vec<SymbolId> {
        let tps = self.st.get(tree.sym).tparams.clone();
        if tps.is_empty() || matches!(tree.kind, TreeKind::TypeApply { .. }) {
            return Vec::new();
        }
        let ptys: Vec<Type> = match &tree.ty {
            Type::Method { paramss, .. } => paramss.first().cloned().unwrap_or_default(),
            _ => return Vec::new(),
        };
        if ptys.len() != first.len() || ptys.iter().any(|t| matches!(t, Type::TypeParam(_))) {
            return Vec::new();
        }
        if tps
            .iter()
            .all(|tp| ptys.iter().any(|t| type_mentions_tparam(t, *tp)))
        {
            tps
        } else {
            Vec::new()
        }
    }

    /// Whether a selection's qualifier names a *type* (or package / module)
    /// rather than a value. `java.lang.Integer.valueOf(3)` selects through the
    /// class symbol; `b.intValue` selects through a `val`. Only the first may
    /// reach a Java `static` member, exactly as in nsc, where those live on the
    /// companion object.
    pub(crate) fn is_type_qualifier(&self, qual: &Tree) -> bool {
        if qual.sym.is_none() {
            return false;
        }
        matches!(
            self.st.get(qual.sym).kind,
            SymKind::Class
                | SymKind::Module
                | SymKind::ModuleClass
                | SymKind::Package
                | SymKind::TypeMember
        )
    }

    /// `scala.FunctionN` for a function type, so `f.tupled` has somewhere to
    /// look. `class_sym_of` deliberately leaves `Type::Function` alone (it
    /// feeds conformance and erasure, which treat function types structurally),
    /// so this is a member-lookup-only detour.
    pub(crate) fn function_class_of(&self, ty: &Type) -> Option<SymbolId> {
        let Type::Function { params, .. } = ty else {
            return None;
        };
        crate::classpath::find_by_jvm(&self.st, &format!("scala/Function{}", params.len()))
    }

    /// Unify `tp` against every argument, not just the first match, and join the
    /// results. Needed for repeated parameters (`List(Circle(1), Rect(2, 3))`)
    /// and for `def f[A](x: A, y: A)`.
    pub(crate) fn unify_tparam_all(
        &self,
        tp: SymbolId,
        params: &[Type],
        args: &[Type],
    ) -> Option<Type> {
        let mut acc: Option<Type> = None;
        for (i, a) in args.iter().enumerate() {
            let Some(p) = param_at(params, i) else {
                break;
            };
            // `unify_one` zips type arguments positionally and has no symbol
            // table to ask, so an argument of a *subclass* has to be lined up
            // with the parameter's class first: `def id[R, U](c: RC[R, U])`
            // given a `UnitRC[String]` is `RC[String, Unit]`, not a one-argument
            // list zipped against `[R, U]`.
            // `param_at` already unwrapped a repeated *parameter* to its
            // element, and a `xs: _*` argument is the matching `Repeated` of
            // its own element: unwrapping only one side solved
            // `def mk[A](xs: A*)` to `A = Int*`.
            let a = match a {
                Type::Repeated(e) => e.as_ref(),
                other => other,
            };
            let a = &self.align_arg_to_param(p, a);
            let keep_singleton = self.st.get(tp).bound_hi.as_ref().is_some_and(|hi| {
                self.st.is_sub_type(hi, &Type::Class { sym: self.st.singleton_sym, args: vec![] })
            });
            let mut hit = if keep_singleton {
                unify_one_precise(tp, p, a)
            } else {
                unify_one(tp, p, a)
            };
            // The same step for a *function* parameter: a `Map[K, V]` is a
            // `K => V`, and that is the shape `def map[B](f: A => B)` reads
            // `B` out of. Only where the argument as written pinned nothing:
            // weighing the view first changed which alternative slick's `map`
            // calls resolved to.
            if hit.is_none() && is_function_pt(p) && !matches!(a, Type::Function { .. }) {
                if let Some(view) = self.function_view(a) {
                    hit = unify_one(tp, p, &view);
                }
            }
            // A rigid type parameter argument is what its *upper bound* is,
            // and `unify_one` has no symbol table to ask. slick's
            // `Comprehension[+Fetch <: Option[Node]]` hands its `fetch: Fetch`
            // to `mapOrNone[A](o: Option[A])(f: A => A)`, and only
            // `Option[Node]` says what `A` is; with nothing inferred `A` fell
            // back to `Any` and the literal's `_.infer(scope, …)` was
            // `value infer is not a member of Any`.
            if hit.is_none() {
                if let Type::TypeParam(id) = a {
                    let hi = self.st.get(*id).bound_hi.clone();
                    if let Some(hi) = hi {
                        if !matches!(hi, Type::TypeParam(_)) {
                            let hi = self.align_to_param_class(p, &hi);
                            hit = unify_one(tp, p, &hi);
                        }
                    }
                }
            }
            if let Some(t) = hit {
                acc = Some(match acc {
                    None => t,
                    // Two arguments contributing to the same parameter: nsc
                    // *minimises* each one's own undetermined variables before
                    // it joins them (`solvedTypes`, no upper constraint ⇒ the
                    // lower bound). `m.getOrElse(1, Seq.empty)` on a
                    // `Map[K, Vector[TS]]` is `lub(Vector[TS], Seq[Nothing])`
                    // = `Seq[TS]`; joining against `Seq[A]` with `A` still a
                    // variable walked the base types until the arguments met
                    // at `Seq` and answered `Seq[AnyRef]`
                    // (slick `compiler/MergeToComprehensions.scala:218`).
                    Some(prev) => {
                        let prev = self.minimize_undet(&prev);
                        let t = self.minimize_undet(&t);
                        self.lub_ty(&prev, &t)
                    }
                });
            }
        }
        acc
    }

    /// Substitute every undetermined variable in `t` by its lower bound
    /// (`Nothing` when it has none) -- nsc's minimisation of a type variable
    /// nothing constrains from above.
    fn minimize_undet(&self, t: &Type) -> Type {
        if self.undet_tvars.is_empty() {
            return t.clone();
        }
        let ids: Vec<SymbolId> = self
            .undet_tvars
            .iter()
            .copied()
            .filter(|tp| type_mentions_tparam(t, *tp))
            .collect();
        if ids.is_empty() {
            return t.clone();
        }
        let vals: Vec<Type> = ids
            .iter()
            .map(|tp| {
                self.st
                    .get(*tp)
                    .bound_lo
                    .clone()
                    .filter(|b| !b.is_no_type() && !b.is_error())
                    .unwrap_or(Type::Nothing)
            })
            .collect();
        crate::symbol::subst_tparams_slice(&ids, &vals, t)
    }

    /// nsc's `instantiateExpecting`: the expected type constrains a method's
    /// type parameters just as the arguments do, so `Array("x", "y")` checked
    /// against `Array[AnyRef]` is an `Array[AnyRef]`, and
    /// `Library.CountAll.column(n): Rep[Int]` knows `T = Int` before its
    /// `implicit TypedType[T]` is searched for.
    ///
    /// Merges into the argument solution `inst` rather than replacing it:
    /// nsc prefers the arguments' lower bound, and only an *invariant*
    /// occurrence in the result forces the expected type to win. A covariant
    /// occurrence is a mere upper bound -- `def cov[T]: List[T]` checked
    /// against `List[Any]` leaves `T = Nothing` in nsc, not `Any` -- so it is
    /// not allowed to instantiate anything here either.
    pub(crate) fn add_expected_constraints(
        &self,
        method: SymbolId,
        ret: &Type,
        pt: &Type,
        inst: Vec<(SymbolId, Type)>,
    ) -> Vec<(SymbolId, Type)> {
        self.add_expected_constraints_in(method, ret, pt, inst, false)
    }

    pub(crate) fn add_expected_constraints_in(
        &self,
        method: SymbolId,
        ret: &Type,
        pt: &Type,
        inst: Vec<(SymbolId, Type)>,
        allow_covariant: bool,
    ) -> Vec<(SymbolId, Type)> {
        if pt.is_no_type() || pt.is_error() || ret.is_no_type() || ret.is_error() {
            return inst;
        }
        let tps = self.st.get(method).tparams.clone();
        if tps.is_empty() {
            return inst;
        }
        let mut found: Vec<(SymbolId, Type, bool)> = Vec::new();
        self.collect_expected(&tps, ret, pt, 1, 0, allow_covariant, &mut found);
        if found.is_empty() {
            return inst;
        }
        let mut inst = inst;
        for (tp, ty, strong) in found {
            match inst.iter_mut().find(|(id, _)| *id == tp) {
                // The arguments already pinned it. Only an invariant position
                // overrides them, and only when the argument solution actually
                // conforms -- otherwise the call is ill-typed and the argument
                // type is what the user needs to see in the message.
                Some(slot) => {
                    if strong && slot.1 != ty && self.st.is_sub_type(&slot.1, &ty) {
                        slot.1 = ty;
                    }
                }
                None => inst.push((tp, ty)),
            }
        }
        inst
    }

    /// Walk the method result type against the expected type, recording what
    /// each type parameter is forced to in a non-covariant position.
    /// `variance` is `1` covariant, `-1` contravariant, `0` invariant; the
    /// `bool` marks an invariant occurrence, which outranks the arguments.
    #[allow(clippy::too_many_arguments)]
    fn collect_expected(
        &self,
        tps: &[SymbolId],
        ret: &Type,
        pt: &Type,
        variance: i8,
        depth: u32,
        allow_covariant: bool,
        out: &mut Vec<(SymbolId, Type, bool)>,
    ) {
        if depth > 12 {
            return;
        }
        match (ret, pt) {
            (Type::Annotated { tpe, .. }, _) => {
                self.collect_expected(tps, tpe, pt, variance, depth + 1, allow_covariant, out)
            }
            (_, Type::Annotated { tpe, .. }) => {
                self.collect_expected(tps, ret, tpe, variance, depth + 1, allow_covariant, out)
            }
            (Type::TypeParam(id), _) if tps.contains(id) => {
                if variance != 1 || allow_covariant {
                    if let Some(t) = self.expected_solution(tps, pt) {
                        out.push((*id, t, variance == 0));
                    }
                }
            }
            // `type Scope = Map[TermSymbol, Type]` names a `Map`; the walk has
            // to see through it or `Map.empty` solves nothing from it. Only an
            // alias dealiases -- an abstract member stays itself, and the
            // arms above have already had their say.
            (_, Type::TypeMember(_)) => {
                let expanded = self.st.dealias(pt);
                if expanded != *pt {
                    self.collect_expected(
                        tps,
                        ret,
                        &expanded,
                        variance,
                        depth + 1,
                        allow_covariant,
                        out,
                    );
                }
            }
            (Type::TypeMember(_), _) => {
                let expanded = self.st.dealias(ret);
                if expanded != *ret {
                    self.collect_expected(
                        tps,
                        &expanded,
                        pt,
                        variance,
                        depth + 1,
                        allow_covariant,
                        out,
                    );
                }
            }
            // `Array` is invariant, and its element must stay an element:
            // rebuilding it from the expected type as a whole would turn
            // `Type::Array` into `Type::Class { array_sym }`, whose JVM name is
            // the pseudo-name `[java/lang/Object`.
            (Type::Array(a), Type::Array(b)) => {
                self.collect_expected(tps, a, b, 0, depth + 1, allow_covariant, out)
            }
            (Type::Array(a), Type::Class { sym, args })
                if *sym == self.st.array_sym && args.len() == 1 =>
            {
                self.collect_expected(tps, a, &args[0], 0, depth + 1, allow_covariant, out)
            }
            (
                Type::Function {
                    params: rp,
                    ret: rr,
                },
                Type::Function {
                    params: pp,
                    ret: pr,
                },
            ) if rp.len() == pp.len() => {
                for (a, b) in rp.iter().zip(pp) {
                    self.collect_expected(
                        tps,
                        a,
                        b,
                        flip_variance(variance),
                        depth + 1,
                        allow_covariant,
                        out,
                    );
                }
                self.collect_expected(tps, rr, pr, variance, depth + 1, allow_covariant, out);
            }
            (Type::Tuple(a), Type::Tuple(b)) if a.len() == b.len() => {
                for (x, y) in a.iter().zip(b) {
                    self.collect_expected(tps, x, y, variance, depth + 1, allow_covariant, out);
                }
            }
            (Type::Tuple(a), Type::Class { args, .. }) if a.len() == args.len() => {
                for (x, y) in a.iter().zip(args) {
                    self.collect_expected(tps, x, y, variance, depth + 1, allow_covariant, out);
                }
            }
            (Type::Class { args, .. }, Type::Tuple(b)) if args.len() == b.len() => {
                for (x, y) in args.iter().zip(b) {
                    self.collect_expected(tps, x, y, variance, depth + 1, allow_covariant, out);
                }
            }
            // A higher-kinded application. `def flatMap[A, B](fa: F[A])(f: A =>
            // F[B]): F[B]` on an abstract `F[_]` has an `Applied` result, not a
            // `Class`, and nothing here matched it: `B` was never read out of
            // the expected `F[String]` and every cats-style `F.flatMap(fa) { … }`
            // came back `F[Any]`. A type constructor's parameter carries no
            // variance annotation the application can see, so the argument
            // position is invariant.
            (
                Type::Applied {
                    ctor: rc,
                    args: ras,
                },
                Type::Applied {
                    ctor: pc,
                    args: pas,
                },
            ) if ras.len() == pas.len() => {
                self.collect_expected(tps, rc, pc, variance, depth + 1, allow_covariant, out);
                for (x, y) in ras.iter().zip(pas) {
                    self.collect_expected(tps, x, y, 0, depth + 1, allow_covariant, out);
                }
            }
            // The same, where the expected type has already settled on a real
            // class (`F[B]` against `List[String]`). The constructor is lined
            // up unapplied so `F` itself is not solved to `List[String]`.
            (Type::Applied { ctor, args: ras }, Type::Class { sym, args: pas })
                if ras.len() == pas.len() =>
            {
                let unapplied = Type::Class {
                    sym: *sym,
                    args: vec![],
                };
                self.collect_expected(
                    tps,
                    ctor,
                    &unapplied,
                    variance,
                    depth + 1,
                    allow_covariant,
                    out,
                );
                for (x, y) in ras.iter().zip(pas) {
                    self.collect_expected(tps, x, y, 0, depth + 1, allow_covariant, out);
                }
            }
            (Type::Class { args: ras, .. }, Type::Applied { args: pas, .. })
                if ras.len() == pas.len() =>
            {
                for (x, y) in ras.iter().zip(pas) {
                    self.collect_expected(tps, x, y, 0, depth + 1, allow_covariant, out);
                }
            }
            // A compound (`A with B`) on either side. Nothing matched it, so
            // `def instance[F[_]](…): Traverse[F] with Reducible[F]` assigned
            // to a `Traverse[Tuple1] with Reducible[Tuple1]` solved `F` from
            // nothing at all and the argument function's parameter came out as
            // `F[Any]` with `F` still its own placeholder -- cats' generated
            // `NTupleUnorderedFoldableInstances` reported `value _1 is not a
            // member of _[Any]` 22 times. Parents are paired by the class they
            // name so that `Traverse[F]` is never read against `Reducible[…]`;
            // the arms below still decide what each pair says.
            (Type::Refined { parents: rps, .. }, Type::Refined { parents: pps, .. }) => {
                for rp in rps {
                    let head = self.st.class_sym_of(rp);
                    for pp in pps.iter().filter(|pp| self.st.class_sym_of(pp) == head) {
                        self.collect_expected(
                            tps,
                            rp,
                            pp,
                            variance,
                            depth + 1,
                            allow_covariant,
                            out,
                        );
                    }
                }
            }
            (Type::Refined { parents: rps, .. }, _) => {
                let head = self.st.class_sym_of(pt);
                for rp in rps.iter().filter(|rp| self.st.class_sym_of(rp) == head) {
                    self.collect_expected(tps, rp, pt, variance, depth + 1, allow_covariant, out);
                }
            }
            (_, Type::Refined { parents: pps, .. }) => {
                let head = self.st.class_sym_of(ret);
                for pp in pps.iter().filter(|pp| self.st.class_sym_of(pp) == head) {
                    self.collect_expected(tps, ret, pp, variance, depth + 1, allow_covariant, out);
                }
            }
            (Type::Class { sym: rs, args: ras }, Type::Class { sym: ps, args: pas }) => {
                if rs == ps {
                    if ras.len() != pas.len() {
                        return;
                    }
                    let tparams = self.st.get(*rs).tparams.clone();
                    for (i, (x, y)) in ras.iter().zip(pas).enumerate() {
                        let v = tparams
                            .get(i)
                            .map(|&tp| {
                                let f = self.st.get(tp).flags;
                                if f.contains(Flags::COVARIANT) {
                                    1
                                } else if f.contains(Flags::CONTRAVARIANT) {
                                    -1
                                } else {
                                    0
                                }
                            })
                            .unwrap_or(0);
                        self.collect_expected(
                            tps,
                            x,
                            y,
                            compose_variance(variance, v),
                            depth + 1,
                            allow_covariant,
                            out,
                        );
                    }
                } else if let Some(base) = self.base_type_instance(ret, *ps, 0) {
                    // `def f[T]: List[T]` against `Seq[Any]`: line the result
                    // up with the expected type's class first.
                    if !matches!(&base, Type::Class { sym, .. } if sym == rs) {
                        self.collect_expected(
                            tps,
                            &base,
                            pt,
                            variance,
                            depth + 1,
                            allow_covariant,
                            out,
                        );
                    }
                }
            }
            // A *compound* result type. `private def instance[F[_] <: Product]
            // (trav: …): Traverse[F] with Reducible[F]` names `F` nowhere but
            // in its result, so cats' generated
            // `catsUnorderedFoldableInstancesForTuple1: Traverse[Tuple1] with
            // Reducible[Tuple1] = instance(…)` has only the expected type to
            // read it off; without this the parameter stayed open, the lambda
            // was typed against `_[Any]`, and every one of the twenty-two
            // tuple instances reported `value _1 is not a member of _[Any]`
            // followed by `found: Traverse[F] with Reducible[F]`.
            //
            // Components pair by position -- both sides come from the same
            // declaration whenever this fires -- and a non-compound on either
            // side is tried against every component, which is how `unify_one`
            // already reads a parameter out of a compound argument.
            (Type::Refined { parents: rps, .. }, Type::Refined { parents: pps, .. })
                if rps.len() == pps.len() =>
            {
                for (x, y) in rps.iter().zip(pps) {
                    self.collect_expected(tps, x, y, variance, depth + 1, allow_covariant, out);
                }
            }
            (Type::Refined { parents: rps, .. }, _) => {
                for x in rps {
                    self.collect_expected(tps, x, pt, variance, depth + 1, allow_covariant, out);
                }
            }
            (_, Type::Refined { parents: pps, .. }) => {
                for y in pps {
                    self.collect_expected(tps, ret, y, variance, depth + 1, allow_covariant, out);
                }
            }
            _ => {}
        }
    }

    /// The type an expected-type position forces a parameter to, or `None`
    /// when it says nothing usable.
    fn expected_solution(&self, tps: &[SymbolId], pt: &Type) -> Option<Type> {
        let pt = pt.widen_constant();
        match pt {
            Type::NoType
            | Type::Error
            | Type::Nothing
            | Type::Null
            | Type::Wildcard
            | Type::BoundedWildcard { .. }
            | Type::Method { .. }
            | Type::Overload(_)
            | Type::ByName(_)
            | Type::Repeated(_) => return None,
            _ => {}
        }
        // Still open: the expected type is itself expressed in terms of the
        // very parameters being solved.
        if mentions_tparam(&pt, tps) {
            return None;
        }
        Some(unarrayify(&pt, self.st.array_sym))
    }

    /// The declared lower bound of `tp`, as seen from the receiver type
    /// (`List[Rect].::[B >: A]` has `B >: Rect`). `None` when there is no bound
    /// or when it is still expressed in terms of unresolved type parameters.
    /// The arguments the *owner* of an inherited member sees, given the
    /// receiver. `Map[K, V]` inherits `++` from `IterableOps[A, …]` with
    /// `A = (K, V)`, so a bound written in terms of `A` has to be read through
    /// the receiver's base type at the owner. Taking the receiver's own
    /// arguments positionally instead made `A` the `K`. Empty when there is
    /// nothing to substitute.
    fn owner_args_as_seen_from(&self, owner: SymbolId, recv: Option<&Type>) -> Vec<Type> {
        let Some(r) = recv else {
            return Vec::new();
        };
        if let Some(Type::Class { args, .. }) = self.base_type_instance(r, owner, 0) {
            if !args.is_empty() {
                return args;
            }
        }
        match r {
            Type::Class { args, .. }
                if !args.is_empty() && args.len() == self.st.get(owner).tparams.len() =>
            {
                args.clone()
            }
            _ => Vec::new(),
        }
    }

    fn tparam_lower_bound(
        &self,
        method: SymbolId,
        tp: SymbolId,
        recv: Option<&Type>,
    ) -> Option<Type> {
        let lo = self.st.get(tp).bound_lo.clone()?;
        // `[B >: A]` is the *owner's* `A`, so it has to be read through the
        // receiver's base type at the owner -- not off the receiver's own
        // arguments. `Map[K, V]`'s `++` is inherited from `IterableOps[A, …]`
        // with `A = (K, V)`; substituting positionally made `A` the `K`, and
        // `Map("a" -> 1) ++ Map("b" -> 2)` came out `Iterable[Serializable]`
        // (`lub(String, (String, Int))`) -- but only in files that had already
        // completed `IterableOps.++` for some other receiver, which is what
        // made it look like an unrelated line's bug.
        let owner = self.st.get(method).owner;
        let owner_args = self.owner_args_as_seen_from(owner, recv);
        let lo = if owner_args.is_empty() {
            lo
        } else {
            self.st.subst_tparams(owner, &owner_args, &lo)
        };
        // A bound that still mentions the *owner's* parameters was not read
        // through the receiver at all (`owner_args` was empty), and one that
        // mentions the method's own is a variable this very call is solving:
        // neither is usable as a lower bound. A parameter of an enclosing
        // method or class is a different matter -- it is a fixed type here,
        // and dropping the bound because of it is what made
        //
        //   def use[T](e: Either[Int, It[T]]) =
        //     e.getOrElse(throw new NoSuchElementException).xs
        //
        // (slick's `JdbcActionComponent.openStream`) solve `B1` to the
        // argument's `Nothing` and report `value xs is not a member of
        // Nothing`, while the same code with `It[String]` compiled.
        let owner_tps = self.st.get(owner).tparams.clone();
        let method_tps = self.st.get(method).tparams.clone();
        if lo.is_no_type()
            || lo.is_error()
            || matches!(lo, Type::Nothing)
            || mentions_tparam(&lo, &owner_tps)
            || mentions_tparam(&lo, &method_tps)
        {
            return None;
        }
        Some(lo)
    }

    pub(crate) fn infer_method_tparams(
        &self,
        method: SymbolId,
        param_tys: &[Type],
        arg_tys: &[Type],
    ) -> Vec<(SymbolId, Type)> {
        self.infer_method_tparams_in(method, param_tys, arg_tys, None)
    }

    /// Method type-parameter inference. `recv` is the receiver type, used to
    /// read `[B >: A]` lower bounds as seen from the receiver.
    pub(crate) fn infer_method_tparams_in(
        &self,
        method: SymbolId,
        param_tys: &[Type],
        arg_tys: &[Type],
        recv: Option<&Type>,
    ) -> Vec<(SymbolId, Type)> {
        let tps = self.st.get(method).tparams.clone();
        let mut out = Vec::new();
        for tp in tps {
            let inferred = self.unify_tparam_all(tp, param_tys, arg_tys);
            let lo = self.tparam_lower_bound(method, tp, recv);
            match (inferred, lo) {
                // The declared lower bound joins with what the arguments said,
                // and the argument's own undetermined variables are minimised
                // first -- `m.getOrElse(1, Seq.empty)` on a
                // `Map[K, Vector[TS]]` is `lub(Seq[Nothing], Vector[TS])` =
                // `Seq[TS]`, where joining `Seq[A]` with `A` still open landed
                // on `Seq[AnyRef]`.
                // The bound is minimised the same way. `Set() ++ opt` reads
                // `++[B >: A]`'s bound off a receiver whose own `A` is still a
                // variable, and `lub(SqlType, ?A)` walked up to `AnyRef` --
                // which no longer conforms to the expected
                // `Set[ColumnOption[_]]`, so the expected type could not
                // override it either (`jdbc/JdbcModelBuilder.scala:279`).
                (Some(t), Some(lo)) => {
                    let t = self.minimize_undet(&t);
                    let lo = self.minimize_undet(&lo);
                    out.push((tp, self.lub_ty(&t, &lo)))
                }
                (Some(t), None) => out.push((tp, t)),
                (None, Some(lo)) => out.push((tp, lo)),
                (None, None) => {}
            }
        }
        out
    }

    /// The argument type read as the parameter's own class, when it is a
    /// strict subclass of it. Everything else is handed back unchanged.
    pub(crate) fn align_to_param_class(&self, param: &Type, arg: &Type) -> Type {
        let Type::Class { sym: ps, args: pas } = param else {
            return arg.clone();
        };
        if pas.is_empty() {
            return arg.clone();
        }
        // A singleton argument (`object OD extends D[Int]`, `this`, `p.type`)
        // is lined up the same way: its base type at the parameter's class is
        // what carries the type arguments.
        match arg {
            Type::Class { sym: as_, .. } if as_ == ps => return arg.clone(),
            Type::Class { .. }
            | Type::ModuleRef(_)
            | Type::ThisType(_)
            | Type::SingleType { .. } => {}
            _ => return arg.clone(),
        }
        match self.base_type_instance(arg, *ps, 0) {
            Some(b) => b,
            None => arg.clone(),
        }
    }

    /// `align_to_param_class` where the parameter is a *function* type: the
    /// lambda's result has to be lined up with the parameter's result class
    /// too, not just the argument as a whole.
    ///
    /// `unify_one` zips type arguments positionally without consulting the
    /// symbol table, so `flatMap[B](f: A => IterableOnce[B])` given a literal
    /// whose body is a `Map[K, V]` zipped `[B]` against `[K, V]` and solved
    /// `B = K`. `mapped.iterator.flatMap(_._2).toMap` (slick's
    /// `CreateAggregates`) then asked for a `TermSymbol <:< (K, V)`, found
    /// none, and left `toMap`'s implicit clause standing as the expression's
    /// type — `value isEmpty is not a member of (<:<[TermSymbol, (K, V)])Map[K, V]`.
    /// Only the *result* is realigned: a function parameter is contravariant,
    /// and reading the literal's parameter as the expected one's base class
    /// would throw away what the literal actually said.
    pub(crate) fn align_arg_to_param(&self, param: &Type, arg: &Type) -> Type {
        if let (
            Type::Function {
                params: pps,
                ret: pr,
            },
            Type::Function {
                params: aps,
                ret: ar,
            },
        ) = (param, arg)
        {
            if pps.len() == aps.len() {
                return Type::Function {
                    params: aps.clone(),
                    ret: Box::new(self.align_to_param_class(pr, ar)),
                };
            }
        }
        self.align_to_param_class(param, arg)
    }

    fn as_tuple_args(&self, ty: &Type) -> Option<Vec<Type>> {
        match ty {
            Type::Tuple(ts) => Some(ts.clone()),
            Type::Class { sym, args } if !args.is_empty() => {
                let n = self.st.get(*sym).name.clone();
                (n.starts_with("Tuple") && n[5..].parse::<usize>() == Ok(args.len()))
                    .then(|| args.clone())
            }
            _ => None,
        }
    }

    /// nsc's "inferred type arguments … do not conform to … type parameter bounds".
    pub(crate) fn check_tparam_bounds(
        &mut self,
        method: SymbolId,
        inst: &[(SymbolId, Type)],
        recv: Option<&Type>,
        span: Span,
        inferred: bool,
    ) {
        let tps = self.st.get(method).tparams.clone();
        if tps.is_empty() || inst.is_empty() {
            return;
        }
        let owner = self.st.get(method).owner;
        let recv_args: Vec<Type> = self.owner_args_as_seen_from(owner, recv);
        let ids: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
        let vals: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
        let mut bad = false;
        for (tp, actual) in inst {
            if actual.is_error() || actual.is_no_type() {
                continue;
            }
            for (bound, upper) in [
                (self.st.get(*tp).bound_hi.clone(), true),
                (self.st.get(*tp).bound_lo.clone(), false),
            ] {
                let Some(bound) = bound else { continue };
                let bound = if recv_args.is_empty() {
                    bound
                } else {
                    self.st.subst_tparams(owner, &recv_args, &bound)
                };
                let bound = crate::symbol::subst_tparams_slice(&ids, &vals, &bound);
                if bound.is_error() || bound.is_no_type() || mentions_any_tparam(&bound) {
                    continue;
                }
                let ok = if upper {
                    self.st.is_sub_type(actual, &bound)
                        || self.st.hk_ctor_meets_proper_bound(actual, &bound)
                } else {
                    self.st.is_sub_type(&bound, actual)
                };
                if !ok {
                    bad = true;
                }
            }
        }
        if !bad {
            return;
        }
        let args_s = inst
            .iter()
            .map(|(_, t)| self.st.display_type(t))
            .collect::<Vec<_>>()
            .join(",");
        let bounds_s = tps
            .iter()
            .map(|tp| self.tparam_bounds_string(*tp))
            .collect::<Vec<_>>()
            .join(",");
        let name = self.st.get(method).name.clone();
        let what = if inferred {
            "inferred type arguments"
        } else {
            "type arguments"
        };
        self.error(
            span,
            format!(
                "{what} [{args_s}] do not conform to method {name}'s type parameter bounds [{bounds_s}]"
            ),
        );
    }

    /// `f[Int](…)` written out: check the explicit type arguments against the
    /// method's declared bounds.
    pub(crate) fn check_explicit_tparam_bounds(&mut self, fun: &Tree, targs: &[Type], span: Span) {
        let sym = fun.sym;
        if self.st.get(sym).kind != crate::symbol::SymKind::Method {
            return;
        }
        let tps = self.st.get(sym).tparams.clone();
        if tps.is_empty() || tps.len() != targs.len() {
            return;
        }
        let recv = match &fun.kind {
            TreeKind::Select { qual, .. } => Some(qual.ty.clone()),
            _ => None,
        };
        let inst: Vec<(SymbolId, Type)> = tps.iter().copied().zip(targs.iter().cloned()).collect();
        self.check_tparam_bounds(sym, &inst, recv.as_ref(), span, false);
    }

    fn tparam_bounds_string(&self, tp: SymbolId) -> String {
        let mut s = self.st.get(tp).name.clone();
        if let Some(lo) = self.st.get(tp).bound_lo.clone() {
            s.push_str(&format!(" >: {}", self.st.display_type(&lo)));
        }
        if let Some(hi) = self.st.get(tp).bound_hi.clone() {
            s.push_str(&format!(" <: {}", self.st.display_type(&hi)));
        }
        s
    }

    /// Wrap `tree` in the `toLong` / `toDouble` / `toFloat` conversion that
    /// widens it to `to`, so the backend emits `i2l` / `i2d` / `i2f`.
    fn wrap_numeric_widen(&mut self, tree: &mut Tree, to: &Type) {
        let name = match to {
            Type::Long => "toLong",
            Type::Float => "toFloat",
            Type::Double => "toDouble",
            _ => {
                tree.ty = to.clone();
                return;
            }
        };
        let from = tree.ty.widen_constant();
        let conv = self
            .st
            .class_sym_of(&from)
            .map(|cls| self.st.lookup_member(cls, name))
            .unwrap_or_default()
            .into_iter()
            .find(|&s| {
                self.st.get(s).kind == SymKind::Method
                    && !matches!(self.st.get(s).intrinsic, crate::symbol::Intrinsic::None)
            });
        let Some(conv) = conv else {
            tree.ty = to.clone();
            return;
        };
        let span = tree.span;
        let inner = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
        let id = inner.id;
        *tree = Tree {
            id,
            span,
            kind: TreeKind::Select {
                qual: Box::new(inner),
                name: name.to_string(),
            },
            ty: to.clone(),
            sym: conv,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
    }

    /// A method whose first parameter clause is implicit is never a value.
    ///
    /// nsc either applies that clause or reports the missing implicit; there
    /// is no third outcome. scala-rs had one: `adapt_implicit_apply` bails in
    /// several places (waiting for a `TypeApply`, or for an argument that is
    /// being typed before its expected type is known), and when nothing ever
    /// came back to apply the clause, the method type stood as the
    /// expression's type — and was then **eta-expanded into a function
    /// value**. `println(List(Some(1), None, Some(3)).flatten)` compiled
    /// cleanly and printed `Main$$$anonfun$0@7a765367`: a silent miscompile,
    /// with the lambda substituted for the result. Written where the type is
    /// visible (`List(Some(1)).flatten.sum`) the same tree surfaced as
    /// `value sum is not a member of ((Some[Int]) => IterableOnce[B])List[B]`.
    ///
    /// This is the backstop. `adapt` runs only when the tree is being used as
    /// a value with a known expected type, and `adapt_implicit_apply` has
    /// already had its chance with that same expected type, so a first clause
    /// still standing here is one that will never be filled. Reported as the
    /// missing implicit it is, never eta-expanded.
    ///
    /// Deliberately *not* fired for `pt: Type::Method` — a method being
    /// applied by an enclosing `Apply` is typed against a method-shaped
    /// expected type and `adapt` returns before this — nor for a first clause
    /// that has an explicit parameter, which really can eta-expand.
    pub(crate) fn reject_unapplied_implicit_clause(&mut self, tree: &mut Tree) -> bool {
        let Type::Method { paramss, .. } = &tree.ty else {
            return false;
        };
        if tree.sym.is_none() {
            return false;
        }
        let first = self
            .st
            .get(tree.sym)
            .paramss
            .first()
            .cloned()
            .unwrap_or_default();
        if first.is_empty()
            || !first
                .iter()
                .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
        {
            return false;
        }
        let want = paramss
            .first()
            .and_then(|c| c.first())
            .cloned()
            .unwrap_or_else(|| self.st.get(first[0]).ty.clone());
        let diverged = self.diverged_implicit.borrow().clone();
        self.error(tree.span, self.missing_implicit_message(&want, diverged));
        tree.ty = Type::Error;
        true
    }

    pub(crate) fn adapt(&mut self, tree: &mut Tree, pt: &Type) {
        if matches!(pt, Type::Method { .. }) {
            return;
        }
        if pt.is_no_type() || tree.ty.is_error() || pt.is_error() {
            return;
        }
        // `xs: _*` is already the sequence a repeated parameter takes.
        if matches!(tree.ty, Type::Repeated(_)) {
            return;
        }
        if self.reject_unapplied_implicit_clause(tree) {
            return;
        }
        self.complete_java_type(&tree.ty, tree.span);
        self.complete_java_type(pt, tree.span);
        // By-name wrap must run before `Nothing <: pt` (Nothing inhabits every
        // type, including `=> T`). Otherwise `tryBreakable { throw e }` would
        // skip Function0 and throw in the caller.
        if let Type::ByName(inner) = pt {
            // Forward an existing by-name parameter's thunk. Wrapping `x`
            // as `() => x` here builds a new thunk on every tail iteration;
            // forcing the final argument then overflows despite the loop.
            // Erasure recognises ByName + a thunk-expected slot and keeps
            // the original Function0, preserving laziness and evaluation count.
            if matches!(tree.kind, TreeKind::Ident { .. })
                && !tree.sym.is_none()
                && self.st.get(tree.sym).flags.contains(Flags::BYNAME)
            {
                let source = unwrap_fn0_or_byname(&self.st.get(tree.sym).ty);
                // This is genuine subtyping, not weak numeric conformance:
                // Int -> Long needs a new thunk that widens each forced value.
                if self.st.is_sub_type(&source, inner) {
                    tree.ty = Type::ByName(Box::new(source));
                    return;
                }
                self.adapt(tree, inner);
            }
            if !matches!(&tree.kind, TreeKind::Function { .. }) {
                let span = tree.span;
                let inner_tree = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
                let ret = if inner_tree.ty.is_no_type() || inner_tree.ty.is_error() {
                    (**inner).clone()
                } else {
                    inner_tree.ty.clone()
                };
                *tree = Tree {
                    id: inner_tree.id,
                    span,
                    kind: TreeKind::Function {
                        vparams: vec![],
                        body: Box::new(inner_tree),
                    },
                    ty: Type::Function {
                        params: vec![],
                        ret: Box::new(ret),
                    },
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
            }
            return;
        }
        if matches!(self.st.dealias(pt), Type::Class { sym, .. } if sym == self.st.singleton_sym)
            && self.is_stable_path(tree)
        {
            tree.ty = self.singleton_to_type(tree.span, tree);
        }
        if self.st.is_sub_type(&tree.ty, pt) {
            // `b.x` with `{ type A <: Int }` stays a TypeMember; pin it to the
            // expected primitive so erasure inserts the same unbox as `Bar#A`.
            if matches!(&tree.ty, Type::TypeMember(_))
                && matches!(
                    pt,
                    Type::Int
                        | Type::Long
                        | Type::Float
                        | Type::Double
                        | Type::Boolean
                        | Type::Byte
                        | Type::Short
                        | Type::Char
                        | Type::Unit
                )
            {
                tree.ty = pt.clone();
            }
            return;
        }
        if self.adapt_singleton(tree, pt) {
            return;
        }
        if self.adapt_function_literal_result(tree, pt) {
            return;
        }
        if let Some(w) = numeric_widen(&tree.ty, pt) {
            // The JVM needs the conversion instruction; setting the type alone
            // left an `int` on the stack where a `double` was expected.
            self.wrap_numeric_widen(tree, &w);
            return;
        }
        // SLS 6.26.1: an `Int` *literal* in range narrows to `Byte`, `Short`
        // or `Char` (`val b: Byte = 1`). Only a constant; `val b: Byte = n`
        // stays an error.
        if let Type::Constant(Lit::Int(v)) = &tree.ty {
            let fits = match pt {
                Type::Byte => (-128..=127).contains(v),
                Type::Short => (-32768..=32767).contains(v),
                Type::Char => (0..=65535).contains(v),
                _ => false,
            };
            if fits {
                tree.ty = pt.clone();
                return;
            }
        }
        if matches!(pt, Type::Unit) {
            // value discarded
            return;
        }
        if matches!(pt, Type::String) && !matches!(tree.ty, Type::String) {
            // allow via toString in concat contexts only — not general
        }
        if matches!(pt, Type::Any | Type::AnyRef | Type::AnyVal) {
            return;
        }
        // nsc `inferExprAlternative`: an *overloaded* method named where a
        // function type is expected settles on the alternative that
        // eta-expands to it -- `constOp[Long]("min")(math.min)` picks
        // `min(Long, Long)` out of the four `math.min`s. Before the
        // eta-expansion below, which needs a single method type.
        if matches!(tree.ty, Type::Overload(_)) {
            self.pick_overload_for_function(tree, pt);
        }
        if let Type::Method { paramss, ret } = &tree.ty {
            if is_function_pt(pt) || self.st.sam_sig(pt).is_some() {
                let params: Vec<Type> = paramss.iter().flatten().cloned().collect();
                let ret = (**ret).clone();
                let (params, ret) = self.solve_eta_tparams(tree.sym, params, ret, pt);
                eta_expand(&mut self.st, &mut self.gensym, tree, params, ret);
                if self.st.is_sub_type(&tree.ty, pt) {
                    return;
                }
            }
        }
        if self.adapt_to_sam(tree, pt) {
            return;
        }
        // `val f: Int => Boolean = anArrayOfBoolean` / an argument already
        // scored applicable by the `Array` fallback in `arg_score`: build the
        // real `wrapBooleanArray(...)` call now (`seqfn_view.rs`).
        if is_function_pt(pt) && self.coerce_array_to_function(tree, pt) {
            return;
        }
        // Same reason as the retry in `type_apply`'s `OverloadPick::None`:
        // `search_conversion` runs on `&self`, and the conversion this
        // position needs may still be sitting unread in a library pickle
        // (`val xs: Iterable[String] = anOption`). Everything above has
        // already declined, so this costs a class file only where the
        // alternative is an error.
        let from_ty = tree.ty.clone();
        self.warm_own_scope_once(&from_ty);
        match self.search_conversion(&tree.ty, pt) {
            ImplicitSearch::Found(id) => {
                let span = tree.span;
                let arg = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
                let from = arg.ty.clone();
                let fun = self.ref_implicit(id, span);
                let applied = Tree {
                    id: arg.id,
                    span,
                    kind: TreeKind::Apply {
                        fun: Box::new(fun),
                        args: vec![arg],
                    },
                    ty: pt.clone(),
                    sym: id,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
                *tree = self.fill_conv_implicits(id, &from, applied, span);
                return;
            }
            ImplicitSearch::Ambiguous(ids) => {
                self.error(
                    tree.span,
                    format!("ambiguous implicit: {}", self.describe_implicits(&ids)),
                );
                tree.ty = Type::Error;
                return;
            }
            ImplicitSearch::None => {}
        }
        // `val xs: Seq[Any] = anArray`. nsc closes this with one of `Predef`'s
        // array wrappings, which are `implicit` there but deliberately not
        // here (they would out-compete `refArrayOps` for every `Array` member
        // selection -- `seqfn_view.rs`), so `search_conversion` above cannot
        // see them. Last, after every implicit the program itself supplies:
        // this only ever replaces the `type mismatch` that follows.
        if self.coerce_array_to_collection(tree, pt) {
            return;
        }
        // A tree the typer could not give a type to has already been reported
        // where it failed; `found: <notype>` only repeats it. nsc's `ErrorType`
        // absorbs the same way, and every other arm here (the overload and
        // constructor errors) already declines to speak for a failed operand.
        if tree.ty.is_no_type() {
            tree.ty = Type::Error;
            return;
        }
        self.error(
            tree.span,
            format!(
                "type mismatch; found: {}  required: {}",
                self.st.display_type(&tree.ty),
                self.st.display_type(pt)
            ),
        );
        tree.ty = Type::Error;
    }

    /// Solve a polymorphic method's own type parameters against the function
    /// type its eta-expansion has to conform to.
    ///
    /// `val f: Node => Node = identity` expands `def identity[A](x: A): A`;
    /// written as it stands that is `A => A`, which conforms to nothing. A
    /// function's parameters are contravariant and its result covariant, so
    /// `A => A <: Node => ?U` says `Node <: A` and `A <: ?U`: nsc solves `A`
    /// from the *parameters*, and the result is only an upper bound. Taking
    /// both at once made `xs.map(identity)` a `CA[Any]`, because a `map` whose
    /// own result is still being inferred expects `Node => Any` and the lub of
    /// the two swallowed `Node`. A parameter the arguments cannot pin -- one
    /// that occurs only in the result -- is still the expected result's to fix.
    pub(crate) fn solve_eta_tparams(
        &mut self,
        sym: SymbolId,
        params: Vec<Type>,
        ret: Type,
        pt: &Type,
    ) -> (Vec<Type>, Type) {
        let Some((pt_params, pt_ret)) = function_sig(pt) else {
            return (params, ret);
        };
        if sym.is_none() {
            return (params, ret);
        }
        let tps = self.st.get(sym).tparams.clone();
        if tps.is_empty() || pt_params.len() != params.len() {
            return (params, ret);
        }
        let mut inst = self.infer_method_tparams(sym, &params, &pt_params);
        if inst.len() < tps.len() {
            let mut sig = params.clone();
            sig.push(ret.clone());
            let mut want = pt_params.clone();
            want.push(pt_ret);
            for (id, t) in self.infer_method_tparams(sym, &sig, &want) {
                if !inst.iter().any(|(i, _)| *i == id) {
                    inst.push((id, t));
                }
            }
        }
        if inst.is_empty() {
            return (params, ret);
        }
        let ids: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
        let vals: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
        (
            params
                .iter()
                .map(|p| crate::symbol::subst_tparams_slice(&ids, &vals, p))
                .collect(),
            crate::symbol::subst_tparams_slice(&ids, &vals, &ret),
        )
    }

    /// Narrow an overloaded reference to the one alternative whose
    /// eta-expansion conforms to the expected function (or SAM) type. Leaves
    /// the tree alone when the expected type is not a function shape, or when
    /// no single alternative fits -- the caller then reports the mismatch it
    /// would have reported anyway.
    fn pick_overload_for_function(&mut self, tree: &mut Tree, pt: &Type) {
        let want = match function_sig(pt) {
            Some(sig) => sig,
            None => match self.st.sam_sig(pt) {
                Some(sam) => (sam.param_tys, sam.ret_ty),
                None => return,
            },
        };
        if tree.sym.is_none() {
            return;
        }
        let name = self.st.get(tree.sym).name.clone();
        let alts = self.drop_overridden(self.overload_alternatives(tree.sym, &name));
        let instantiated = self.overload_member_types.get(&tree.sym.0).cloned();
        let (want_params, want_ret) = want;
        let mut hit: Option<(SymbolId, Type)> = None;
        for m in alts {
            let ty = instantiated
                .as_ref()
                .and_then(|g| g.iter().find(|(s, _)| *s == m).map(|(_, t)| t.clone()))
                .unwrap_or_else(|| self.st.get(m).ty.clone());
            let Type::Method { paramss, ret } = &ty else {
                continue;
            };
            let params: Vec<Type> = paramss.iter().flatten().cloned().collect();
            if params.len() != want_params.len() {
                continue;
            }
            let as_fn = Type::Function {
                params: params.clone(),
                ret: ret.clone(),
            };
            let fits = self.st.is_sub_type(
                &as_fn,
                &Type::Function {
                    params: want_params.clone(),
                    ret: Box::new(want_ret.clone()),
                },
            );
            if !fits {
                continue;
            }
            // Two alternatives can both fit (`min(Int, Int)` conforms to
            // nothing a `(Long, Long) => Long` wants, but a widening pair
            // could); the exact one wins, as it does for an application.
            let exact = params == want_params;
            match &hit {
                None => hit = Some((m, ty)),
                Some((_, prev)) => {
                    let prev_exact = matches!(prev, Type::Method { paramss, .. }
                        if paramss.iter().flatten().cloned().collect::<Vec<_>>() == want_params);
                    if exact && !prev_exact {
                        hit = Some((m, ty));
                    } else if !prev_exact {
                        // Ambiguous: leave the overload alone.
                        return;
                    }
                }
            }
        }
        if let Some((m, ty)) = hit {
            tree.sym = m;
            tree.ty = ty;
        }
    }

    /// nsc types a function *literal*'s body against the expected result type,
    /// so `foreach((x: Int) => x + 1)` discards the value and
    /// `f((x: Int) => x)` against `Int => Long` widens it. A literal whose
    /// parameter types are written out is typed *before* its expected type is
    /// known -- overload resolution needs its result -- so its body never saw
    /// the expected result. Adapt it here instead.
    ///
    /// Only a literal: `val h: Int => Int = …; fu(h)` stays the mismatch nsc
    /// reports. And only the result -- the parameters have to be the ones the
    /// expected type asks for already.
    fn adapt_function_literal_result(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        if !matches!(tree.kind, TreeKind::Function { .. }) {
            return false;
        }
        let Type::Function { params, ret } = tree.ty.clone() else {
            return false;
        };
        let Some((pt_params, pt_ret)) = function_sig(pt) else {
            return false;
        };
        if pt_params.len() != params.len() || pt_ret.is_no_type() || pt_ret.is_error() {
            return false;
        }
        if self.st.is_sub_type(&ret, &pt_ret) {
            // The result already fits; whatever else fails here is not ours.
            return false;
        }
        if !pt_params
            .iter()
            .zip(&params)
            .all(|(p, a)| self.st.is_sub_type(p, a) && self.st.is_sub_type(a, p))
        {
            return false;
        }
        let TreeKind::Function { body, .. } = &mut tree.kind else {
            return false;
        };
        if body.ty.is_no_type() || body.ty.is_error() {
            return false;
        }
        let mut adapted = std::mem::replace(body.as_mut(), Tree::dummy(TreeKind::Empty));
        let before = self.diags.len();
        self.adapt(&mut adapted, &pt_ret);
        let ok = self.diags.len() == before && !adapted.ty.is_error();
        self.diags.truncate(before);
        if !ok {
            // Put the body back and leave the function type as it was: the
            // caller reports the mismatch on the whole literal, the way nsc
            // does. (The body itself may have been rewritten on the way, but
            // this expression is an error either way.)
            **body = adapted;
            return false;
        }
        **body = adapted;
        tree.ty = Type::Function {
            params,
            ret: Box::new(pt_ret),
        };
        true
    }

    fn adapt_to_sam(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let Some(sam) = self.st.sam_sig(pt) else {
            return false;
        };
        let Type::Function { params, ret } = &tree.ty else {
            return false;
        };
        if params.len() != sam.param_tys.len() {
            return false;
        }
        if !ret.is_no_type() && !self.st.is_sub_type(ret, &sam.ret_ty) {
            return false;
        }
        for (have, want) in params.iter().zip(sam.param_tys.iter()) {
            if have.is_no_type() {
                continue;
            }
            if !self.st.is_sub_type(want, have) && !self.st.is_sub_type(have, want) {
                return false;
            }
        }
        tree.ty = pt.clone();
        true
    }

    /// The class a `This` tree stands for derives from `cls`.
    fn this_derives_from(&self, tree: &Tree, cls: SymbolId) -> bool {
        if cls.is_none() || !matches!(&tree.kind, TreeKind::This { .. }) {
            return false;
        }
        let here = if tree.sym.is_none() {
            self.st.class_sym_of(&tree.ty).unwrap_or(SymbolId::NONE)
        } else {
            tree.sym
        };
        !here.is_none()
            && self
                .base_type_instance(&self.st.self_type_of_class(here), cls, 0)
                .is_some()
    }

    /// `adapt_singleton` without the side effect, for the compound arm: a
    /// parent may hold on its own without the whole type being adopted.
    fn can_adapt_singleton(&self, tree: &Tree, pt: &Type) -> bool {
        let mut probe = Tree {
            id: tree.id,
            span: tree.span,
            kind: TreeKind::This { qual: None },
            ty: tree.ty.clone(),
            sym: tree.sym,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        if !matches!(&tree.kind, TreeKind::This { .. }) {
            return false;
        }
        self.adapt_singleton(&mut probe, pt)
    }

    fn adapt_singleton(&self, tree: &mut Tree, pt: &Type) -> bool {
        match pt {
            Type::ThisType(cls) => {
                if !matches!(&tree.kind, TreeKind::This { .. }) {
                    return false;
                }
                // A `this.type` written in a *parent* is this class's `this`
                // once it is read here: `type Self >: this.type <: Node`
                // declared by `Node` means `NullaryNode.this.type` inside
                // `trait NullaryNode extends Node`, so
                // `def mapChildren(…): Self = this` is right. Only a `This`
                // tree gets this -- an ordinary value of the class does not.
                let ok = tree.sym == *cls
                    || matches!(
                        &tree.ty,
                        Type::Class { sym, .. } | Type::ModuleRef(sym) if *sym == *cls
                    )
                    || self.this_derives_from(tree, *cls);
                if ok {
                    tree.ty = pt.clone();
                }
                ok
            }
            // `Node.Self with DefNode`: each parent has to hold, and the
            // `this.type` half holds the way the arm above says.
            Type::Refined { parents, decls } if decls.is_empty() && !parents.is_empty() => {
                let all = parents
                    .iter()
                    .all(|p| self.st.is_sub_type(&tree.ty, p) || self.can_adapt_singleton(tree, p));
                if all {
                    tree.ty = pt.clone();
                }
                all
            }
            Type::SingleType { sym, .. } => {
                if tree.sym == *sym {
                    tree.ty = pt.clone();
                    true
                } else {
                    false
                }
            }
            // `type Self >: this.type <: Node`: the declaration says
            // `this.type <: Self`, so `this` conforms to `Self` even though
            // `Node` does not. Only the *lower* bound admits this, and only a
            // tree that really is that singleton -- `def id: Self = someNode`
            // still fails.
            Type::TypeMember(id) => {
                let Some(lo) = self.st.get(*id).bound_lo.clone() else {
                    return false;
                };
                if lo.is_no_type() || lo.is_error() || matches!(lo, Type::Nothing) {
                    return false;
                }
                if self.st.is_sub_type(&tree.ty, &lo) || self.adapt_singleton(tree, &lo) {
                    tree.ty = pt.clone();
                    true
                } else {
                    false
                }
            }
            Type::Annotated { tpe, .. } => {
                if self.st.is_sub_type(&tree.ty, tpe) || self.adapt_singleton(tree, tpe) {
                    tree.ty = pt.clone();
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    pub(crate) fn desugar_custom_interpolator(&mut self, tree: &mut Tree) {
        let TreeKind::InterpolatedString {
            prefix,
            parts,
            args,
        } = &tree.kind
        else {
            return;
        };
        let span = tree.span;
        let prefix = prefix.clone();
        let parts = parts.clone();
        let args = args.clone();
        let sc = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: "StringContext".into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let apply = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(sc),
                name: "apply".into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let part_lits: Vec<Tree> = parts
            .into_iter()
            .map(|p| Tree {
                id: NodeId(0),
                span,
                kind: TreeKind::Literal {
                    lit: Lit::String(p),
                },
                ty: Type::String,
                sym: SymbolId::NONE,
                postfix: false,
                scala_ref: false,
                stable_pat: false,
            })
            .collect();
        let sc_apply = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(apply),
                args: part_lits,
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        let sel = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Select {
                qual: Box::new(sc_apply),
                name: prefix,
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        *tree = Tree {
            id: tree.id,
            span,
            kind: TreeKind::Apply {
                fun: Box::new(sel),
                args,
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
    }

    pub(crate) fn rewrite_generic_array_new(&mut self, tree: &mut Tree, elem: Type) {
        let span = tree.span;
        let args = match &mut tree.kind {
            TreeKind::Apply { args, .. } => std::mem::take(args),
            _ => Vec::new(),
        };
        let Some(ct_cls) = self
            .st
            .lookup("ClassTag")
            .into_iter()
            .find(|s| self.st.get(*s).kind == SymKind::Class)
        else {
            self.error(
                span,
                "unimplemented: generic Array construction without ClassTag",
            );
            tree.ty = Type::Error;
            return;
        };
        let ct_ty = Type::Class {
            sym: ct_cls,
            args: vec![elem.clone()],
        };
        match self.search_implicit(&ct_ty) {
            ImplicitSearch::Found(id) => {
                let mut recv = self.ref_implicit(id, span);
                self.adapt(&mut recv, &ct_ty);
                let sel = Tree {
                    id: NodeId(0),
                    span,
                    kind: TreeKind::Select {
                        qual: Box::new(recv),
                        name: "newArray".into(),
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
                *tree = Tree {
                    id: tree.id,
                    span,
                    kind: TreeKind::Apply {
                        fun: Box::new(sel),
                        args,
                    },
                    ty: Type::NoType,
                    sym: SymbolId::NONE,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                };
                self.type_expr_inner(tree, &Type::NoType);
            }
            ImplicitSearch::None => {
                // `new Array[T](0)` is not an implicit search in nsc — it is
                // `typedNew`'s own check, with its own wording
                // (`neg/t9401`, `neg/t2775`).
                self.error(
                    span,
                    format!(
                        "cannot find class tag for element type {}",
                        self.st.display_type(&elem)
                    ),
                );
                tree.ty = Type::Error;
            }
            ImplicitSearch::Ambiguous(ids) => {
                self.error(
                    span,
                    format!("ambiguous implicit: {}", self.describe_implicits(&ids)),
                );
                tree.ty = Type::Error;
            }
        }
    }

    /// nsc: a *parameterless* method whose result is a function takes no
    /// argument list of its own, so `def g: Int => Int; g(3)` is `g.apply(3)`
    /// — and so is `f.tupled((1, 2))`, since `tupled` is such a method.
    /// The backend already applies a `Type::Function` callee
    /// (`gen_function_apply`), so handing it the method's result is all this
    /// takes. Only a non-empty argument list is rewritten: `g()` on a
    /// `() => Int` stays the reference nsc's empty-application rules give it.
    pub(crate) fn auto_apply_nullary_function(fun: &mut Tree, nargs: usize) {
        if nargs == 0 {
            return;
        }
        let Type::Method { paramss, ret } = &fun.ty else {
            return;
        };
        if !paramss.is_empty() {
            return;
        }
        let Type::Function { params, .. } = ret.as_ref() else {
            return;
        };
        if params.len() != nargs {
            return;
        }
        fun.ty = (**ret).clone();
    }

    pub(crate) fn rewrite_receiver_apply(&mut self, fun: &mut Tree) {
        if matches!(&fun.kind, TreeKind::New { .. }) {
            return;
        }
        // A `Select` that resolved to a *value* is a receiver like any other:
        // `O.m1(3)` where `m1: Mono` is `O.m1.apply(3)`. Only a module
        // selection is left alone -- `scala.Some(1)` already has its own
        // `apply` path, and rewriting it here would change what codegen
        // emits for every qualified companion call.
        if matches!(&fun.kind, TreeKind::Select { .. })
            && !matches!(
                strip_annotations(&fun.ty),
                Type::Class { .. } | Type::Array(_)
            )
        {
            return;
        }
        // nsc inserts `.apply` for `c(1)` / `xs(i)` when `c` is a value, not a method.
        // Leave method/overload idents alone (`f(1)`).
        // An annotation is not part of what a type *is*: slick's
        // `val (b, m: Map[…] @unchecked) = …` then calls `m(f)`, and without
        // looking through `@unchecked` that reported
        // `value apply is not a member of Map[…] @unchecked`.
        let insert = matches!(
            strip_annotations(&fun.ty),
            Type::Array(_) | Type::Class { .. } | Type::ModuleRef(_)
        );
        if !insert {
            return;
        }
        let span = fun.span;
        let id = fun.id;
        let qual = std::mem::replace(fun, Tree::dummy(TreeKind::Empty));
        *fun = Tree {
            id,
            span,
            kind: TreeKind::Select {
                qual: Box::new(qual),
                name: "apply".into(),
            },
            ty: Type::NoType,
            sym: SymbolId::NONE,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        self.type_select(
            fun,
            &Type::Method {
                paramss: vec![],
                ret: Box::new(Type::NoType),
            },
        );
    }
}
