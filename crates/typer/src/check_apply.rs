#![allow(dead_code)]
//! Typing an application `f(args)` and the collection-shaped rewrites that
//! depend on its receiver.
//!
//! `type_apply_in` is the centre: it types the callee, resolves overloads,
//! types the arguments against the parameters and infers what is still open.
//! The helpers around it rebuild a result type from the receiver -- what
//! `map` on a `Map` or a `SortedSet` gives back, what an `Either` or an
//! `ArrayOps` yields -- which nsc gets from `CanBuildFrom`-era signatures.

use crate::check::*;
use scala_rs_parser::ast::*;

impl Typer {
    /// Every application gets its own set of undetermined type variables: an
    /// argument of *this* call is typed by a nested `type_apply`, whose
    /// variables must not still be in scope when this one weighs its own
    /// alternatives. The body has many exits, so the set is saved and restored
    /// here rather than at each of them.
    /// The element of an expected type that is an array, whichever of the two
    /// spellings it arrives in.
    ///
    /// `Type::Array` is the usual one; `Type::Class { sym: Array }` is what a
    /// run that compiles the library's own `scala/Array.scala` produces, since
    /// the source class shadows the prelude's (`SymbolTable::is_array_class`).
    pub(crate) fn array_elem_expected(&self, ty: &Type) -> Option<Type> {
        match strip_annotations(ty) {
            Type::Class { sym, args } if args.len() == 1 && self.st.is_array_class(*sym) => {
                Some(args[0].clone())
            }
            other => array_elem_of(other),
        }
    }

    /// `new Array(n)` written with no type argument, whose element came out
    /// `Nothing` because nothing had yet said what it should be.
    ///
    /// nsc leaves that constructor's `T` *undetermined* until the expression is
    /// checked against the position it sits in, so `new C[K, V](0, new
    /// Array(0), gen)` (`concurrent/TrieMap.scala`) reads the element off the
    /// constructor parameter. This compiler solves `T` at the `New` itself, so
    /// an argument position — the one place the expected type arrives *after*
    /// the argument has been typed — has to ask again. Keyed on the written
    /// syntax, not on the type: an `Array[Nothing]` that some other expression
    /// really has is not re-typed.
    pub(crate) fn is_open_array_new(&self, a: &Tree) -> bool {
        let TreeKind::Apply { fun, .. } = &a.kind else {
            return false;
        };
        self.array_new_elem_unwritten(fun)
            && matches!(self.array_elem_expected(&a.ty), Some(Type::Nothing))
    }

    /// Whether the `new Array…` this `New` tree heads wrote no element type.
    ///
    /// The tree's *type* cannot answer this once the element has been filled
    /// in from the expected type, so the written form is what is asked. It is
    /// also what decides whether nsc's arity diagnosis names `Array[T]` or the
    /// element itself.
    pub(crate) fn array_new_elem_unwritten(&self, fun: &Tree) -> bool {
        let TreeKind::New { tpt } = &fun.kind else {
            return false;
        };
        matches!(&tpt.kind,
            TreeKind::Ident { name } if name != crate::materialize::RESOLVED_TYPE)
            && self.st.is_array_class(tpt.sym)
    }

    /// Whether re-typing this argument against `p` can give a `new Array(n)`
    /// the element it is still missing.
    pub(crate) fn array_new_wants(&self, a: &Tree, p: &Type) -> bool {
        !matches!(self.array_elem_expected(p), None | Some(Type::Nothing))
            && self.is_open_array_new(a)
    }

    /// Whether the `new C[…]` this tree heads *wrote* a type-argument list.
    ///
    /// The type a `New` head carries cannot answer that. `Type::Class { sym,
    /// args }` looks the same whether the arguments came from the source or
    /// were filled in with the class's own parameters as placeholders, and
    /// [`type_args_are_instantiated`] — which is how the constructor path
    /// decides whether to trust them — refuses any argument that *is* one of
    /// the instantiated class's own type parameters.
    ///
    /// That refusal is right for a placeholder and wrong for a `new C[K, V]`
    /// written from inside `C`, where `K` and `V` name the enclosing
    /// instance's parameters and are the only sensible answer. So ask the
    /// tree, which knows what the programmer typed.
    fn new_wrote_type_args(fun: &Tree) -> bool {
        let TreeKind::New { tpt } = &fun.kind else {
            return false;
        };
        let mut cur = &**tpt;
        // `new C[K, V] @unchecked` keeps the applied type under the annotation.
        while let TreeKind::AnnotatedTypeTree { tpt, .. } = &cur.kind {
            cur = tpt;
        }
        matches!(&cur.kind,
            TreeKind::AppliedTypeTree { args, .. } | TreeKind::TypeApply { args, .. }
                if !args.is_empty())
    }

    /// The prototype a method *reference* argument is typed against when its
    /// parameter is a function type that only the call's inference can
    /// finish: the parameter types the callee already fixes, and a wildcard
    /// result (nsc's `Int => ?` for `foreach[U](f: Int => U)`).
    ///
    /// Only for a bare reference (`println`, `Console.println`): it is the
    /// argument whose typing depends on a function-shaped expectation, since
    /// an overloaded one otherwise settles on its nullary alternative. Every
    /// alternative of an overloaded callee has to agree on the parameter
    /// types, or the prototype would be choosing among the callee's
    /// alternatives instead of the argument's.
    pub(crate) fn method_ref_function_proto(
        &self,
        arg: &Tree,
        fun_ty: &Type,
        fun_sym: SymbolId,
        idx: usize,
    ) -> Option<Type> {
        if !matches!(arg.kind, TreeKind::Ident { .. } | TreeKind::Select { .. }) {
            return None;
        }
        let tps: Vec<SymbolId> = if fun_sym.is_none() {
            Vec::new()
        } else {
            self.st.get(fun_sym).tparams.clone()
        };
        let open_params = |params: &[Type]| {
            params
                .iter()
                .any(|t| t.is_no_type() || t.is_error() || mentions_tparam(t, &tps))
        };
        let one = |alt: &Type| -> Option<Type> {
            let Type::Method { paramss, .. } = alt else {
                return None;
            };
            let p = match param_at(paramss.first()?, idx)? {
                Type::ByName(t) => t.as_ref(),
                t => t,
            };
            let p = match p {
                Type::Class { sym, args } => match self.st.function_class_shape(*sym, args) {
                    Some(f) => f,
                    None => {
                        // A SAM-typed parameter (`foreach[U](f: Fun[String,
                        // U])`): the method eta-expands into the SAM itself,
                        // so the prototype is the class, with the callee's
                        // variables left open.
                        let sam = self.st.sam_sig(p)?;
                        if open_params(&sam.param_tys) {
                            return None;
                        }
                        let wild = vec![Type::Wildcard; tps.len()];
                        return Some(crate::symbol::subst_tparams_slice(&tps, &wild, p));
                    }
                },
                other => other.clone(),
            };
            let Type::Function { params, .. } = p else {
                return None;
            };
            if open_params(&params) {
                return None;
            }
            Some(Type::Function {
                params,
                ret: Box::new(Type::Wildcard),
            })
        };
        match fun_ty {
            Type::Method { .. } => one(fun_ty),
            Type::Overload(alts) if !alts.is_empty() => {
                let first = one(&alts[0])?;
                for a in &alts[1..] {
                    if one(a)? != first {
                        return None;
                    }
                }
                Some(first)
            }
            _ => None,
        }
    }

    /// Whether no alternative of the callee takes a function (or SAM) at
    /// argument position `idx`. `false` whenever that cannot be told -- a
    /// callee that is not a method type, or a position past its parameters.
    fn no_function_formal_at(&self, fun_ty: &Type, idx: usize) -> bool {
        let alts: Vec<&Type> = match fun_ty {
            Type::Method { .. } => vec![fun_ty],
            Type::Overload(alts) if !alts.is_empty() => alts.iter().collect(),
            _ => return false,
        };
        for alt in alts {
            let Type::Method { paramss, .. } = alt else {
                return false;
            };
            let Some(p) = paramss.first().and_then(|ps| param_at(ps, idx)) else {
                return false;
            };
            if p.is_no_type() || p.is_error() || self.expects_function_value(p) {
                return false;
            }
        }
        true
    }

    pub(crate) fn type_apply(&mut self, tree: &mut Tree, pt: &Type) {
        let saved = std::mem::take(&mut self.undet_tvars);
        self.type_apply_in(tree, pt);
        // The expected type is the last constraint on what the arguments left
        // undetermined, exactly as it is for the callee's own parameters:
        // `val l: List[Map[String, Int]] = f(Map.empty)` pins the `K` and `V`
        // that reached the result through `f`'s own `T`.
        self.solve_undet_result(tree, pt);
        // The callee's own parameters that reached the result unsolved are
        // undetermined too, not fixed types: `ConstArray.newBuilder()` is a
        // `ConstArrayBuilder[?T]`, and the `+` applied to it is what says what
        // `?T` is.
        let own = self.undetermined_of(tree);
        self.undet_tvars.extend(own);
        // A variable this call could not solve is still undetermined for the
        // call that encloses it: `take(id(Map.empty))` hands `Map[?K, ?V]`
        // outward, and it is the *outer* parameter that fixes it.
        let leaked: Vec<SymbolId> = self
            .undet_tvars
            .drain(..)
            .filter(|tp| type_mentions_tparam(&tree.ty, *tp))
            .collect();
        self.undet_tvars = saved;
        self.undet_tvars.extend(leaked);
    }

    fn type_apply_in(&mut self, tree: &mut Tree, pt: &Type) {
        // An application the typer already resolved once carries the implicit
        // arguments and defaults that pass filled in. Resolving it again -- a
        // tupled retry re-types the arguments it repacked, and each of them may
        // be an application of its own -- has to start from what the user
        // wrote, or the callee is weighed against an argument list that
        // includes its own implicits.
        if let TreeKind::Apply { args, .. } = &mut tree.kind {
            args.retain(|a| !a.id.is_filled_arg());
        }
        if self.try_expand_reify(tree, pt) {
            return;
        }
        if self.try_rewrite_case_copy(tree, pt) {
            return;
        }
        if self.try_rewrite_assignment_op(tree, pt) {
            return;
        }
        if self.try_rewrite_dynamic_apply(tree, pt) {
            return;
        }
        if self.in_aux_ctor() {
            flatten_curried_ctor_delegation(tree);
        }
        let ctor_del = match &tree.kind {
            TreeKind::Apply { fun, .. } => self.in_aux_ctor() && is_this_or_super_callee(fun),
            _ => false,
        };
        if ctor_del {
            self.type_ctor_delegation(tree);
            return;
        }
        // `new C(a)(b)`: like `extends A(1)(2)`, a curried constructor is one
        // call whose clauses are flat on the JVM. The clauses arrive as nested
        // `Apply`s, and left alone the outer one was an application of the
        // *instance* -- slick's `new SimpleLiteral(name)(tpe)` looked up
        // `apply` on the companion and reported `ambiguous overload`.
        // The length of the first written clause, when clauses were folded: the
        // pick below is held to the alternatives that clause could name, which
        // is the set the fold already measured its arity against.
        let curried_clauses = self.flatten_curried_new(tree);
        let curried_first_len = curried_clauses.as_ref().and_then(|cs| cs.first()).copied();
        // Kept before the borrow below: `record_named_arg_order` keys the
        // application by it.
        let tree_id = tree.id;
        let (fun, args) = match &mut tree.kind {
            TreeKind::Apply { fun, args } => (fun, args),
            _ => return,
        };
        // new C(args)
        if matches!(&fun.kind, TreeKind::New { .. }) {
            self.new_is_applied = true;
            // `type_expr_inner`, not `type_expr`: the head of `new C(args)` is
            // not a value, and adapting it to the *application's* expected
            // type reported `found: ProductResultConverter required:
            // ResultConverter[R, W, U, _]` before the arguments had had their
            // say. The expected type still reaches the head -- it is what the
            // type arguments are read from -- it just no longer has to be
            // satisfied there.
            self.type_expr_inner(fun, pt);
            self.new_is_applied = false;
            if let Some(elem) = array_elem_of(&fun.ty) {
                // `Array`'s constructor takes exactly one argument, and this
                // path never checked that: `new Array[Int](10, 10)` — the
                // multi-dimensional shape removed in 2.10, and `neg/multi-array`
                // in scala/scala's own corpus — was silently accepted. It went
                // unnoticed because the *un*-annotated `new Array(10, 10)` was
                // rejected further down for having no matching constructor at
                // all, which is not this diagnosis and stopped being reached
                // once the element could be inferred.
                //
                // nsc runs this check before it instantiates the element, which
                // is why it names `Array[T]` when the element was not written
                // and `Array[Int]` when it was. The result type is still the
                // array, so the enclosing `val a: Array[Int] = …` does not add
                // a second, derived complaint on top of the one real error.
                if args.len() != 1 {
                    let shown = if self.array_new_elem_unwritten(fun) {
                        "T".to_string()
                    } else {
                        self.st.display_type(&elem)
                    };
                    let msg = if args.len() > 1 {
                        format!(
                            "too many arguments (found {}, expected 1) for constructor Array: \
                             (_length: Int): Array[{shown}]",
                            args.len()
                        )
                    } else {
                        format!(
                            "not enough arguments for constructor Array: \
                             (_length: Int): Array[{shown}].\n\
                             Unspecified value parameter _length."
                        )
                    };
                    self.error(tree.span, msg);
                    for a in args.iter_mut() {
                        self.type_expr(a, &Type::Int);
                    }
                    tree.ty = Type::Array(Box::new(elem));
                    tree.sym = fun.sym;
                    return;
                }
                if needs_classtag_elem(&elem) {
                    self.rewrite_generic_array_new(tree, elem);
                    return;
                }
                for a in args.iter_mut() {
                    self.type_expr(a, &Type::Int);
                    self.adapt(a, &Type::Int);
                }
                tree.ty = Type::Array(Box::new(elem));
                tree.sym = fun.sym;
                return;
            }
            let class_id = fun
                .sym
                .is_none()
                .then(|| self.st.class_sym_of(&fun.ty))
                .flatten()
                .or(Some(fun.sym))
                .filter(|s| !s.is_none());
            let class_id = class_id.or_else(|| self.st.class_sym_of(&fun.ty));
            // Prelude Java classes may have members without their complete
            // constructor overload set. Read the actual classfile before
            // choosing prototypes or overloads, independent of prior uses.
            if let Some(c) = class_id {
                self.ensure_java_loaded(c, fun.span);
                if curried_clauses.is_some() {
                    self.supply_binary_ctors(c);
                }
            }
            // `new C(b = 2, a = 1)`: named arguments must be put in parameter
            // order before the constructor overload is picked, since the pick
            // is driven by the argument types.
            let curried_placed = curried_clauses.as_ref().and_then(|clauses| {
                self.reorder_curried_ctor_args(args, class_id, fun, clauses, tree_id)
            });
            if curried_placed == Some(false) {
                tree.ty = Type::Error;
                return;
            }
            if curried_placed.is_none() && Self::has_named_arg(args) {
                let placed = self.reorder_named_ctor_args(args, class_id, fun, None);
                self.record_named_arg_order(tree_id);
                if !placed {
                    for a in args.iter_mut() {
                        self.type_expr(a, &Type::NoType);
                    }
                    tree.ty = Type::Error;
                    return;
                }
            }
            let tps = class_id
                .map(|c| self.st.get(c).tparams.clone())
                .unwrap_or_default();
            // Keep explicit `new C[T](…)` args; otherwise infer. Do not adapt
            // constructor arguments to raw type parameters (`A`) first.
            //
            // A *written* type-argument list is explicit even when its
            // arguments are the instantiated class's own type parameters,
            // which is exactly what `new C[K, V]` inside `class C[K, V]`
            // writes: `CNode`'s `updatedAt` / `insertedAt` / `removedAt` /
            // `renewed` (`concurrent/TrieMap.scala`) all end in
            // `new CNode[K, V](…)`. Judged on the type alone those look like
            // uninstantiated placeholders, so the whole list was dropped and
            // the parameters re-inferred from the value arguments — which
            // mention neither, so both solved to `Nothing` and each method's
            // inferred result type became `CNode[Nothing, Nothing]`. Every
            // caller then failed; 13 of that file's 46 errors were the single
            // overload `GCAS(cn, cn.renewed(startgen, ct), ct)`.
            // `new o.In[Int](…)`: the head's type carries a prefix
            // (`prefix.rs`); the arguments are on the class under it, and the
            // prefix goes back on the result below.
            let head_view: Option<Vec<scala_rs_parser::RefineDecl>> =
                match (&fun.ty, crate::prefix::view_prefix(&fun.ty)) {
                    (Type::Refined { decls, .. }, Some(_)) => Some(decls.clone()),
                    _ => None,
                };
            let explicit: Vec<Type> = match crate::prefix::strip_view(&fun.ty) {
                Type::Class { args, .. }
                    if type_args_are_instantiated(args, &tps)
                        || (Self::new_wrote_type_args(fun)
                            && !args.is_empty()
                            && (tps.is_empty() || args.len() == tps.len())) =>
                {
                    args.clone()
                }
                _ => Vec::new(),
            };
            let infer = !tps.is_empty() && explicit.is_empty();
            // Like the method path: type non-lambda args now so the ctor
            // overload can be picked, and leave function literals untyped
            // until their parameter type is known (`new S[String](x => …)`).
            let mut arg_tys: Vec<Type> = Vec::new();
            // The primary constructor's own parameter types, for the same
            // reason the method path hands them out (`proto_arg_type`): a
            // function literal *inside* an argument -- slick's
            // `StatementParameters(…, if (…) … else { s => …; … }, …)`, whose
            // case-class `apply` lands on this path -- has nowhere else to
            // read its parameter types from. Only for a monomorphic class (or
            // one whose type arguments were written), only where the arity
            // settles which constructor this is, and only for a fully
            // determined parameter.
            //
            // A class with a *single* constructor has nothing to pick, so
            // every such parameter is the argument's expected type, as it is
            // for nsc (`doTypedApply` types the arguments of a monomorphic
            // method against its formals). An invariant one decides the
            // argument's own inference: `new BitmapIndexedMapNode[K, V1](…,
            // Array(k, v), …)` passes an `Array[Any]` in scalac, and typed
            // with no expected type `Array(k, v)` was an `Array[AnyRef]`.
            // With several constructors a parameter is a hint for a function
            // literal only: `new C(1)` beside `C(Long)` and `C(Int)` must
            // still pick on the argument's own type.
            let outer_prefix = class_id.and_then(|c| self.ctor_outer_prefix(c, fun));
            let single_ctor = class_id.is_some_and(|c| {
                self.st
                    .lookup_member(c, "<init>")
                    .into_iter()
                    .filter(|&id| {
                        self.st.get(id).owner == c
                            && self.st.get(id).kind == crate::symbol::SymKind::Method
                    })
                    .count()
                    == 1
            });
            let mut ctor_protos: Vec<Type> = class_id
                .filter(|_| tps.is_empty() || explicit.len() == tps.len())
                .map(|c| (c, self.st.get(c).ctor_fields.clone()))
                .filter(|(_, fs)| fs.len() == args.len())
                .map(|(c, fs)| {
                    fs.iter()
                        .map(|f| {
                            let t = self.st.get(*f).ty.clone();
                            let t = if tps.is_empty() {
                                t
                            } else {
                                self.st.subst_tparams(c, &explicit, &t)
                            };
                            let t = match &outer_prefix {
                                Some(p) => self.st.subst_as_seen_from(p, &t),
                                None => t,
                            };
                            if !t.is_no_type()
                                && !t.is_error()
                                && !type_mentions_wildcard(&t)
                                && !mentions_any_tparam(&t)
                                && (self.is_function_shaped(&t)
                                    || (single_ctor
                                        && !matches!(t, Type::ByName(_) | Type::Repeated(_))))
                            {
                                t
                            } else {
                                Type::NoType
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            // The same single-constructor rule where the fields above say
            // nothing: a *polymorphic* class whose type arguments are being
            // inferred still has parameters that do not mention them, and
            // those are fully determined -- `new Node(Array.empty,
            // Array.empty, 0)` on `Node[A](content: Array[Any], hashes:
            // Array[Int], n: Int)` builds an `Object[]` and an `int[]` in
            // scalac (`EmptyMapNode` in `HashMap.scala`), and typed with no
            // expected type both searched a `ClassTag` for a variable nothing
            // had solved. The constructor's own signature is read, so a class
            // with no `ctor_fields` (one read from a class file) is covered
            // too. Only a closed type (`is_closed_ctor_proto`), and only the
            // slots still empty.
            if let (Some(c), None) = (class_id, curried_clauses.as_ref()) {
                if let Some(ctor) = self.sole_own_ctor(c) {
                    let first = match &self.st.get(ctor).ty {
                        Type::Method { paramss, .. } => {
                            paramss.first().cloned().unwrap_or_default()
                        }
                        _ => Vec::new(),
                    };
                    ctor_protos.resize(args.len(), Type::NoType);
                    for (i, slot) in ctor_protos.iter_mut().enumerate() {
                        if !slot.is_no_type() {
                            continue;
                        }
                        let declared = match param_at(&first, i) {
                            Some(Type::ByName(t)) => t.as_ref(),
                            Some(t) => t,
                            None => continue,
                        };
                        let t = if !explicit.is_empty() && explicit.len() == tps.len() {
                            self.st.subst_tparams(c, &explicit, declared)
                        } else {
                            declared.clone()
                        };
                        if self.is_closed_ctor_proto(declared, &tps)
                            && !mentions_tparam(&t, &tps)
                            && !t.is_error()
                            && !type_mentions_wildcard(&t)
                        {
                            *slot = t;
                        }
                    }
                }
            }
            for (ai, a) in args.iter_mut().enumerate() {
                if a.byname_thunk {
                    arg_tys.push(a.argument_type());
                    continue;
                }
                if let TreeKind::Function { vparams, .. } = &a.kind {
                    if is_annotated_lambda(a) {
                        self.type_expr(a, &Type::NoType);
                        arg_tys.push(a.argument_type());
                        continue;
                    }
                    arg_tys.push(Type::Function {
                        params: vec![Type::NoType; vparams.len()],
                        ret: Box::new(Type::NoType),
                    });
                } else {
                    let pt_arg = ctor_protos.get(ai).cloned().unwrap_or(Type::NoType);
                    if pt_arg.is_no_type() {
                        self.type_expr(a, &Type::NoType);
                    } else {
                        // A prototype is a hint, never a constraint -- the same
                        // rollback the method path does. slick's
                        // `new StructValue(…, xs.toMap)` has a `TermSymbol =>
                        // Int` parameter, and solving `toMap`'s `K` / `V`
                        // through `Map <: Function1` is not something the
                        // expected type can do here; typed with no prototype it
                        // is a `Map[TermSymbol, Int]` and conforms after all.
                        let saved = a.clone();
                        let mark = self.diags.len();
                        self.type_expr(a, &pt_arg);
                        let complained = self.diags[mark..]
                            .iter()
                            .any(|d| d.level == scala_rs_span::Level::Error);
                        if complained
                            || a.ty.is_error()
                            || a.ty.is_no_type()
                            || !self.st.is_sub_type(&a.ty, &pt_arg)
                        {
                            self.diags.truncate(mark);
                            *a = saved;
                            self.type_expr(a, &Type::NoType);
                        }
                    }
                    arg_tys.push(a.argument_type());
                }
            }
            // Same as the method path: what the arguments left undetermined is
            // this call's to solve, so `new C(Map.empty)` may pick the
            // constructor whose parameter fixes `K` and `V`.
            for a in args.iter() {
                let open = self.undetermined_of(a);
                self.undet_tvars.extend(open);
            }
            let field_tys: Vec<Type> = class_id
                .map(|c| {
                    self.st
                        .get(c)
                        .ctor_fields
                        .iter()
                        .map(|f| {
                            let t = self.st.get(*f).ty.clone();
                            match &outer_prefix {
                                Some(p) => self.st.subst_as_seen_from(p, &t),
                                None => t,
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            // Whether `ctor_params` are already read at `explicit`: what
            // `pick_ctor_at` picks is, what the `ctor_fields` fallback below
            // is not.
            let mut params_at_targs = false;
            let (ctor_sym, ctor_params) = if let Some(c) = class_id {
                // `new C[A, B](x)(ev)`: the constructor's clauses are written
                // in the *class's* type parameters, so an argument is weighed
                // against `TT[B]` and not against `TT[Int]`. `new TypedCase[B,
                // P](…)(bType, om.liftedType(bType))` in slick's `Case.scala`
                // passes a `BaseTypedType[B]` for a `TypedType[B]` parameter,
                // and that conformance only holds once the class's parameters
                // are the call's. `extends A(1)(2)` has read the arguments at
                // its `targs` all along (`pick_ctor_at`); the `new` path did
                // not.
                self.supply_binary_ctors(c);
                let saved_prefix = std::mem::replace(&mut self.ctor_prefix, outer_prefix.clone());
                let mut picked =
                    self.pick_ctor_at_clause(c, &explicit, &arg_tys, None, curried_first_len);
                // An argument whose class is still a `-cp` stub is a subtype of
                // nothing: `find_or_stub_java_class` gives one `parents =
                // [AnyRef]` until the classfile is really read.
                // `new OutputStreamWriter(System.out)` asked before anything
                // had read `java/io/PrintStream`, so it did not conform to
                // `OutputStream`. `arg_score` runs on `&self` and cannot read a
                // classfile; do it here, where the mutable borrow exists, and
                // ask once more -- only a pick that has already failed pays,
                // and `ensure_java_loaded` reads each classfile once.
                if !matches!(picked, OverloadPick::Found(..)) && self.warm_java_args(&arg_tys) {
                    picked =
                        self.pick_ctor_at_clause(c, &explicit, &arg_tys, None, curried_first_len);
                }
                self.ctor_prefix = saved_prefix;
                match picked {
                    OverloadPick::Found(sym, ps, _) => {
                        params_at_targs = !explicit.is_empty();
                        (Some(sym), ps)
                    }
                    OverloadPick::Ambiguous => {
                        self.error(tree.span, "ambiguous overload for constructor");
                        (None, Vec::new())
                    }
                    OverloadPick::None => {
                        if field_tys.len() != arg_tys.len() {
                            self.error(
                                tree.span,
                                format!(
                                    "no matching overload for constructor {} with arguments ({})",
                                    self.st.get(c).name,
                                    arg_tys
                                        .iter()
                                        .map(|t| self.st.display_type(t))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                            );
                        }
                        (None, field_tys.clone())
                    }
                }
            } else {
                (None, Vec::new())
            };
            // nsc's `AccessError` on the constructor it just resolved. It has
            // to happen here: `new C(…)` is not a member selection, so
            // `type_select`'s access check never sees the `<init>` and a
            // `private` / `protected` constructor was accepted from anywhere
            // (`neg/sensitive`, `neg/t4987`, `neg/t6601`,
            // `neg/protected-constructors`). Only on a constructor that was
            // actually picked -- a failed overload has already been reported,
            // and a second message about the alternative it did not pick is
            // noise.
            //
            // Only on a `new` the *program wrote*. Several rewrites build one
            // with `Tree::dummy`, and the access question there belongs to the
            // member they rewrote, not to the constructor they lowered it to:
            // `v.copy(x = 2)` on a `case class C private (x: Int)` becomes
            // `new C(2)`, and nsc asks whether `copy` is accessible (which
            // `case_copy_access_error` does, under
            // `-Xsource-features:case-apply-copy-access`) and never whether
            // the constructor is. Reporting there refused
            // `tests/fixtures/xflags_case_access_bad.scala`, which scalac
            // 2.13.16 compiles cleanly with no flag. A parsed `new` always
            // carries a real span.
            let synthetic = tree.span.is_dummy() || fun.span.is_dummy();
            if let (Some(sym), Some(c)) = (ctor_sym, class_id) {
                if !synthetic {
                    self.ctor_access_error(sym, c, tree.span);
                }
            }
            // Generic inference must use ctor *fields* (`Tuple2._1: A`) even when
            // the picked `<init>` is erased to `(Any, Any)` in the prelude.
            let nargs = args.len();
            // Only the primary constructor's declaration is described by
            // ctor_fields. Same-arity secondary constructors have their own
            // parameter types even when class type arguments are inferred.
            let primary = match (class_id, ctor_sym) {
                (Some(c), Some(ctor)) => {
                    self.st.get(ctor).params == self.st.get(c).ctor_fields
                        || (c.0 < self.st.prelude_end
                            && self
                                .st
                                .get(c)
                                .members
                                .iter()
                                .copied()
                                .find(|&m| self.st.get(m).name == "<init>")
                                == Some(ctor))
                }
                _ => true,
            };
            let use_fields = infer && primary && field_tys.len() == nargs && !field_tys.is_empty();
            let unify_params = if use_fields {
                field_tys.clone()
            } else {
                ctor_params.clone()
            };
            let mut inferred_args: Vec<Type> = Vec::new();
            if let Some(c) = class_id {
                if !explicit.is_empty() {
                    inferred_args = explicit;
                    tree.ty = Type::Class {
                        sym: c,
                        args: inferred_args.clone(),
                    };
                    fun.ty = tree.ty.clone();
                } else if infer {
                    let pt_args: Vec<Type> = match pt {
                        Type::Class { args: a, sym } if *sym == c => a.clone(),
                        Type::Tuple(ts)
                            if numbered_arity(&self.st.get(c).name, "Tuple") == Some(ts.len())
                                && ts.len() == tps.len() =>
                        {
                            ts.clone()
                        }
                        _ => Vec::new(),
                    };
                    // `def mk[R, U](c: RC[R, U]): RC[R, U] = new ProdRC(c)`:
                    // the expected type names a base class, and
                    // `ProdRC[R, U] <: RC[R, U]` reads the arguments off it.
                    // Without this the parameters that no constructor argument
                    // mentions fell through to `Any`.
                    let from_base = if pt_args.is_empty() {
                        self.base_targs_from_pt(c, pt)
                    } else {
                        vec![None; tps.len()]
                    };
                    for (i, tp) in tps.iter().enumerate() {
                        // nsc's default for a type parameter that stays
                        // completely unconstrained (no argument mentions it,
                        // no expected type reaches it) is variance-driven, not
                        // a flat `Any`: `Infer.solvedTypes` instantiates an
                        // untouched type variable to the tightest type that is
                        // always safely widenable later, which is the
                        // parameter's own lower bound (`Nothing` when
                        // unbounded) for a covariant or invariant parameter,
                        // and its upper bound (`Any` when unbounded) for a
                        // contravariant one. `private final class Vector2[+A]`
                        // in `scala/collection/immutable/Vector.scala` has a
                        // `copy` method with no declared return type whose
                        // body is `new Vector2(prefix1, len1, data2, suffix1,
                        // length0)` -- none of those value parameters mention
                        // `A` -- and confirmed against real scalac
                        // (`-Xprint:typer`), the inferred return type is
                        // `Vector2[Nothing]`, not `Vector2[Any]`.
                        // `Vector2[Nothing] <: Vector[B]` for every `B >: A`,
                        // same as `Inv[Nothing]`/`Contra[Any]` below; `Any`
                        // there is unsound at every call site that widens the
                        // result (`override def updated[B >: A](...): Vector[B]`),
                        // which is exactly the `Vector2[Any] required:
                        // Vector[B]` shape `docs/scala-library.md` records.
                        let default_ty = if self.st.get(*tp).flags.contains(Flags::CONTRAVARIANT) {
                            Type::Any
                        } else {
                            Type::Nothing
                        };
                        inferred_args.push(
                            self.unify_tparam_all(*tp, &unify_params, &arg_tys)
                                // A lambda argument is still a placeholder
                                // (`(<notype>) => <notype>`) at this point.
                                // Solving `B` from it would hide the expected
                                // type, and the lambda would then be typed
                                // against nothing: `val p: (Int, Int => Int) =
                                // (1, n => n + 1)` reported `missing parameter
                                // type for expanded function`.
                                .filter(|t| !mentions_no_type(t))
                                .or_else(|| pt_args.get(i).cloned())
                                .or_else(|| from_base.get(i).cloned().flatten())
                                .unwrap_or(default_ty),
                        );
                    }
                    tree.ty = Type::Class {
                        sym: c,
                        args: inferred_args.clone(),
                    };
                    fun.ty = tree.ty.clone();
                } else {
                    tree.ty = fun.ty.clone();
                }
            } else {
                tree.ty = fun.ty.clone();
            }
            if let Some(decls) = head_view {
                if matches!(tree.ty, Type::Class { .. }) {
                    tree.ty = Type::Refined {
                        parents: vec![tree.ty.clone()],
                        decls,
                    };
                    fun.ty = tree.ty.clone();
                }
            }
            for (i, a) in args.iter_mut().enumerate() {
                // A repeated parameter covers every argument from its position
                // on, and the argument's type is the *element* type. Indexing
                // the clause by position (as this did) handed argument 0 the
                // raw `T*` -- `new SetTupleParameter[(T1, T2)](c1, c2)` on a
                // `(val children: SetParameter[_]*)` constructor could never
                // typecheck. The method path has always used `param_at`.
                let mut p = if use_fields {
                    param_at(&field_tys, i).cloned().unwrap_or(Type::NoType)
                } else {
                    param_at(&ctor_params, i).cloned().unwrap_or(Type::NoType)
                };
                if let Some(c) = class_id {
                    // ... but only if it is not already: `new Box[(T, T2), …]`
                    // written *inside* `Box[T, U]` substitutes a type that
                    // mentions the parameter it replaces, so a second pass
                    // turns `T` into `((T, T2), T2)`.
                    if !inferred_args.is_empty() && !params_at_targs {
                        p = self.st.subst_tparams(c, &inferred_args, &p);
                    }
                }
                if a.ty.is_no_type() || self.array_new_wants(a, &p) {
                    self.type_expr(a, &p);
                }
                if !p.is_no_type() {
                    if !mentions_tparam(&p, &tps) {
                        a.ty = self.instantiate_parameterless(a.sym, a.ty.clone(), &p);
                        self.instantiate_undet_arg(a, &p);
                    }
                    self.adapt(a, &p);
                }
            }
            self.check_instantiated_self_type(&tree.ty, tree.span);
            // String's source type has a structural representation, while
            // constructor selection uses its exact java.lang.String symbol.
            if class_id == Some(self.st.string_sym) {
                tree.ty = Type::String;
            }
            tree.sym = ctor_sym.or(class_id).unwrap_or(SymbolId::NONE);
            if let Some(csym) = ctor_sym {
                let mut ctor_ty = self.st.get(csym).ty.clone();
                if let Some(c) = class_id {
                    if !inferred_args.is_empty() {
                        ctor_ty = self.st.subst_tparams(c, &inferred_args, &ctor_ty);
                    }
                }
                let ctor_fun = Tree {
                    id: fun.id,
                    span: fun.span,
                    kind: TreeKind::Ident {
                        name: "<init>".into(),
                    },
                    ty: ctor_ty,
                    sym: csym,
                    postfix: false,
                    scala_ref: false,
                    stable_pat: false,
                    byname_thunk: false,
                    byname_type_marker: false,
                };
                // The picked constructor still speaks the class's own type
                // parameters. `new TypedRep[Int]()` has to search for
                // `TT[Int]`, not for the declared `TT[T]`.
                let ctor_params: Vec<Type> = match class_id {
                    Some(c) if !inferred_args.is_empty() => ctor_params
                        .iter()
                        .map(|p| self.st.subst_tparams(c, &inferred_args, p))
                        .collect(),
                    _ => ctor_params,
                };
                // Only when the call is *short*. A constructor's clauses reach
                // this path already flattened, so `new C(x)(ev)` on
                // `C(x)(implicit ev)` has nothing left to fill -- and filling
                // anyway appended a second, searched `ev` after the one the
                // user wrote: `new K[B]("s")(tb)` typechecked and then failed
                // the verifier with three arguments for two parameters.
                if args.len() < ctor_params.len() {
                    let _ = self.fill_defaults_and_implicits(
                        tree.span,
                        args,
                        &ctor_params,
                        &ctor_fun,
                        pt,
                    );
                }
            }
            return;
        }

        let dummy_method = Type::Method {
            paramss: vec![],
            ret: Box::new(Type::NoType),
        };
        // Expected type Method so nullary methods (`unary_-`, `def f: Int` called as `f()`)
        // are not auto-applied before this Apply is typed.
        self.typing_callee = true;
        let saved_arity = self.callee_arity.replace(args.len());
        self.type_expr(fun, &dummy_method);
        self.callee_arity = saved_arity;
        self.typing_callee = false;

        // A function-typed value is applied through FunctionN.apply. Its
        // symbol still points at the declaration that produced the value (for
        // example `g` in `g(1)`), but that declaration's parameter lists are
        // unrelated to this call once the callee is itself an application:
        // `g(1)("x")` must read the second parameter list from the function
        // result, not re-read `g`'s first `Int` parameter. Going through the
        // method overload path in that shape typed the second argument as
        // `Int` and emitted a runtime checkcast from `String` to `Integer`.
        if matches!(fun.kind, TreeKind::Apply { .. }) {
            if let Type::Function { params, ret } = fun.ty.clone() {
                // By-name parameters use the ordinary application path: it
                // recognizes source thunks and preserves their result type.
                // Treating a thunk literal as an ordinary function argument
                // here wraps it once more (`=> Int` becomes `() => (() =>
                // Int)`).
                if !params.iter().any(|p| matches!(p, Type::ByName(_)))
                    && args.len() != params.len()
                {
                    self.error(
                        tree.span,
                        format!(
                            "wrong number of arguments (found {}, expected {})",
                            args.len(),
                            params.len()
                        ),
                    );
                }
                if params.iter().any(|p| matches!(p, Type::ByName(_))) {
                    // Fall through to the normal method/function path.
                } else {
                    for (i, a) in args.iter_mut().enumerate() {
                        let p = params.get(i).cloned().unwrap_or(Type::NoType);
                        if a.ty.is_no_type() || matches!(a.kind, TreeKind::Function { .. }) {
                            self.type_expr(a, &p);
                        }
                        if !p.is_no_type() {
                            self.adapt(a, &p);
                        }
                    }
                    tree.ty = (*ret).clone();
                    tree.sym = SymbolId::NONE;
                    return;
                }
            }
        }
        self.rewrite_receiver_apply(fun);
        Self::auto_apply_nullary_function(fun, args.len());
        let placed = self.reorder_named_args(args, fun);
        self.record_named_arg_order(tree_id);
        if !placed {
            for a in args.iter_mut() {
                self.type_expr(a, &Type::NoType);
            }
            tree.ty = Type::Error;
            return;
        }

        let mut recv_ty = match &fun.kind {
            TreeKind::Select { qual, .. } => Some(qual.ty.clone()),
            _ => None,
        };
        let fun_name = fun.name().unwrap_or("").to_string();

        // Type non-lambda args first so overload resolution has info; lambdas
        // wait for an expected Function type (for-comprehension desugaring).
        let mut arg_tys = Vec::new();
        let saved_taking_args = std::mem::replace(&mut self.typing_call_args, true);
        let fun_ty_for_pretype = fun.ty.clone();
        let nargs = args.len();
        if args
            .iter()
            .any(|a| matches!(a.kind, TreeKind::Function { .. }))
        {
            let signatures = match &fun_ty_for_pretype {
                Type::Overload(alts) => alts.clone(),
                other => vec![other.clone()],
            };
            for ty in signatures {
                if let Type::Method { paramss, .. } = ty {
                    for param in paramss.iter().flatten() {
                        self.complete_java_type(param, fun.span);
                    }
                }
            }
        }
        // A single-clause callee handed more arguments than it has
        // parameters (and no repeated one to absorb them): the arguments are
        // typed without prototypes, see `pt_arg` below.
        let too_many_args = match &fun_ty_for_pretype {
            Type::Method { paramss, .. } => paramss.first().is_some_and(|ps| {
                ps.len() < args.len() && !ps.last().is_some_and(|t| matches!(t, Type::Repeated(_)))
            }),
            _ => false,
        };
        for (ai, a) in args.iter_mut().enumerate() {
            if a.byname_thunk {
                arg_tys.push(a.argument_type());
                continue;
            }
            if let TreeKind::Function { vparams, .. } = &a.kind {
                // A source Function0 supplied to a by-name value parameter
                // can contribute its whole function type before lower bounds
                // are solved. Its body is not the by-name thunk's result.
                let source_arity = vparams.len();
                let source_fn0_value = source_arity == 0
                    && matches!(
                        &fun_ty_for_pretype,
                        Type::Method { paramss, .. } if paramss.first()
                            .and_then(|ps| param_at(ps, ai))
                            .is_some_and(|p| matches!(p, Type::ByName(_)))
                    );
                if source_fn0_value {
                    let saved = a.clone();
                    let mark = self.diags.len();
                    self.type_expr(a, &Type::NoType);
                    if self.error_count_since(mark) == 0
                        && !a.ty.is_error()
                        && !mentions_no_type(&a.ty)
                    {
                        arg_tys.push(a.argument_type());
                        continue;
                    }
                    *a = saved;
                    self.diags.truncate(mark);
                }
                if is_annotated_lambda(a) {
                    // The parameters are written, but the body still deserves
                    // what the expected type settles about the result (nsc's
                    // `protoTypeArgs`): cats' `Kleisli((fe: Either[A, B]) => fe
                    // match { case Left(a) => F.map(f(a))(Left.apply _) … })` at
                    // a declared `Kleisli[F, Either[A, B], Either[C, D]]` types
                    // each branch against `F[Either[C, D]]`; typed against
                    // nothing, the branches met at `AnyRef`. Only the result
                    // is taken, and only when it is fully settled.
                    let proto = self.proto_arg_type(
                        &fun_ty_for_pretype,
                        fun.sym,
                        ai,
                        nargs,
                        pt,
                        recv_ty.as_ref(),
                        false,
                    );
                    let body_pt = match &proto {
                        Type::Function { params, ret }
                            if params.len() == source_arity && !type_has_wildcard(ret) =>
                        {
                            proto.clone()
                        }
                        _ => Type::NoType,
                    };
                    let saved = a.clone();
                    let mark = self.diags.len();
                    self.type_expr(a, &body_pt);
                    if !body_pt.is_no_type() && self.error_count_since(mark) > 0 {
                        // A hint the literal does not fit is no hint: type it
                        // as before and let the call report what is wrong.
                        *a = saved;
                        self.diags.truncate(mark);
                        self.type_expr(a, &Type::NoType);
                    }
                    arg_tys.push(a.argument_type());
                    continue;
                }
                // nsc `Infer.pretypeArgs`: when every alternative wants the
                // same function *parameter* types at this position, the
                // literal can be typed before the alternatives are weighed,
                // and its result type is then what picks one. `StringOps` has
                // `map(Char => Char): String` and `map[B](Char => B):
                // IndexedSeq[B]`; without this `"abc".map(_.toString)` sees
                // `(<notype>) => <notype>`, both alternatives are applicable
                // and the more specific `Char => Char` wins wrongly.
                // The same pre-typing for a `{ case … }` literal weighed
                // against `PartialFunction` alternatives. `StringOps.collect`
                // is the `map` pair again --
                // `collect(PartialFunction[Char, Char]): String` and
                // `collect[B](PartialFunction[Char, B]): IndexedSeq[B]` --
                // but a PF parameter is a *class*, so `agreed_lambda_params`
                // bailed and the more specific `Char` alternative won
                // regardless of what the case bodies return
                // (`"abc".collect { case c => c.toInt }` was
                // `type mismatch; found: Int required: Char`).
                let pf_literal = matches!(
                    &a.kind,
                    TreeKind::Function { vparams, body } if is_case_block_literal(vparams, body)
                );
                if pf_literal {
                    if let Some(pt_arg) = self.agreed_pf_param(&fun_ty_for_pretype, ai) {
                        self.type_expr(a, &pt_arg);
                        arg_tys.push(a.argument_type());
                        continue;
                    }
                }
                if let Some(ps) = self.agreed_lambda_params(&fun_ty_for_pretype, ai, source_arity) {
                    // `Wildcard`, not `NoType`: the parameters are what this
                    // pre-typing fixes, the result is whatever the body says
                    // and must not be checked against anything yet.
                    let pt_arg = Type::Function {
                        params: ps,
                        ret: Box::new(Type::Wildcard),
                    };
                    self.type_expr(a, &pt_arg);
                    arg_tys.push(a.argument_type());
                    continue;
                }
                arg_tys.push(Type::Function {
                    params: vec![Type::NoType; source_arity],
                    ret: Box::new(Type::NoType),
                });
            } else {
                let strict_proto = self.proto_arg_type(
                    &fun_ty_for_pretype,
                    fun.sym,
                    ai,
                    nargs,
                    pt,
                    recv_ty.as_ref(),
                    false,
                );
                let inherited_hint = self
                    .provisional_arg_sites
                    .contains(&(self.file_index, tree_id));
                let from_declaration = if inherited_hint {
                    self.proto_arg_type(
                        &fun_ty_for_pretype,
                        fun.sym,
                        ai,
                        nargs,
                        &Type::NoType,
                        recv_ty.as_ref(),
                        false,
                    )
                } else {
                    strict_proto.clone()
                };
                let has_fixed_shape = match &fun_ty_for_pretype {
                    Type::Method { paramss, .. } => paramss
                        .first()
                        .and_then(|ps| param_at(ps, ai))
                        .is_some_and(|p| {
                            let p = match p {
                                Type::ByName(t) => t.as_ref(),
                                t => t,
                            };
                            matches!(
                                p,
                                Type::Class { .. } | Type::Array(_) | Type::Function { .. }
                            )
                        }),
                    _ => false,
                };
                let provisional = !has_fixed_shape
                    && (strict_proto.is_no_type()
                        || (inherited_hint && strict_proto != from_declaration));
                let pt_arg = if too_many_args {
                    // More arguments than the one clause has parameters: nsc
                    // tries the tupled application (`tryTupleApply`) before
                    // typing any argument against a formal, because position
                    // `i` of the call is not parameter `i`. Typed against the
                    // tuple formal, `update("Reopen", "reopen")`'s first
                    // argument was converted (slick's `anyToShapedValue`) into
                    // something the tupled retry could no longer repack.
                    Type::NoType
                } else if provisional || strict_proto.is_no_type() {
                    self.proto_arg_type(
                        &fun_ty_for_pretype,
                        fun.sym,
                        ai,
                        nargs,
                        pt,
                        recv_ty.as_ref(),
                        true,
                    )
                } else {
                    strict_proto
                };
                // A nested call with nothing stricter to go on gets the
                // lenient prototype (`lenient_proto_arg_type`). Still a hint:
                // the fallback below re-types it with none when it does not
                // fit.
                let mut lenient = false;
                let pt_arg = if pt_arg.is_no_type()
                    && matches!(a.kind, TreeKind::Apply { .. } | TreeKind::TypeApply { .. })
                {
                    let l = self.lenient_proto_arg_type(&fun_ty_for_pretype, fun.sym, ai, pt);
                    lenient = !l.is_no_type();
                    l
                } else {
                    pt_arg
                };
                let method_ref_proto = if pt_arg.is_no_type() && !too_many_args {
                    self.method_ref_function_proto(a, &fun_ty_for_pretype, fun.sym, ai)
                } else {
                    None
                };
                if let Some(proto) = method_ref_proto {
                    // A method named as the argument of a function-typed
                    // parameter whose result is still a variable
                    // (`xs.foreach(println)`, `foreach[U](f: A => U)`). nsc
                    // types it against `Int => ?` and `inferExprAlternative`
                    // keeps the alternative that eta-expands to that; typed
                    // against nothing, the overload's nullary `println()` was
                    // auto-applied and the argument became `Unit`. A
                    // prototype that does not fit is thrown away, as below.
                    let saved = a.clone();
                    let mark = self.diags.len();
                    self.type_expr(a, &proto);
                    if self.error_count_since(mark) > 0
                        || a.ty.is_error()
                        || a.ty.is_no_type()
                        || !self.st.is_sub_type(&a.ty, &proto)
                    {
                        *a = saved;
                        self.diags.truncate(mark);
                        self.type_expr(a, &Type::NoType);
                    }
                } else if pt_arg.is_no_type() {
                    self.type_expr(a, &Type::NoType);
                } else {
                    // A prototype is a hint, never a constraint. An argument
                    // the expected type does not actually fit -- one whose
                    // implicit clause is still open, say -- is typed again as
                    // if there had been none, and its diagnostics go with it.
                    //
                    // But only when dropping the prototype actually *helps*.
                    // An argument that failed for a reason of its own keeps
                    // failing without one, and now with none of its parameter
                    // types known: gitbucket's
                    // `post(path, form)(writableUsersOnly { (form, repo) => … })`
                    // has one `not found` inside the lambda, and re-typing it
                    // with no expected type turned that into a `form: Any`
                    // and one `value … is not a member of Any` for every field
                    // the body reads (153 of them across the benchmark).
                    let saved = a.clone();
                    let mark = self.diags.len();
                    // A wildcard the lenient prototype put in (`proto_arg_type`)
                    // is this call's "not decided yet", not an existential the
                    // program wrote; say so while the argument is typed, as the
                    // lambda path below does, so calls inside it do not read
                    // their own parameters out of it.
                    let declared_p = match &fun_ty_for_pretype {
                        Type::Method { paramss, .. } => {
                            paramss.first().and_then(|ps| param_at(ps, ai)).cloned()
                        }
                        _ => None,
                    };
                    let relaxed_here = type_has_wildcard(&pt_arg)
                        && !declared_p.as_ref().is_some_and(type_has_wildcard);
                    if relaxed_here {
                        self.relaxed_pt_depth += 1;
                    }
                    self.type_expr_arg_prototype(a, &pt_arg, provisional);
                    if relaxed_here {
                        self.relaxed_pt_depth -= 1;
                    }
                    let with_errs = self.error_count_since(mark);
                    // A lenient prototype's wildcards are "not decided", never
                    // an answer: an argument that took one into its own type
                    // (cats' `leftWiden(rightFunctor.widen(fac))` came back
                    // `F[_, D]`) is typed again without the hint.
                    let leaked = lenient && type_has_wildcard(&a.ty);
                    if leaked
                        || with_errs > 0
                        || a.ty.is_error()
                        || a.ty.is_no_type()
                        || !self.st.is_sub_type(&a.ty, &pt_arg)
                    {
                        let with_tree = std::mem::replace(a, saved);
                        let with_diags: Vec<_> = self.diags.split_off(mark);
                        self.type_expr(a, &Type::NoType);
                        if self.error_count_since(mark) >= with_errs.max(1) {
                            // The retry is no better; keep the typing that at
                            // least knew what the parameters were.
                            self.diags.truncate(mark);
                            self.diags.extend(with_diags);
                            *a = with_tree;
                        }
                    }
                }
                // An unapplied method whose parameter is no function type in
                // any alternative (`foo[F](f: F)`, `Option(add)`, `(add, 1)`):
                // nsc types it against that formal, so it is "missing
                // argument list" in 2.13 and the eta-expansion under
                // `-Xsource:3`, before the alternatives are weighed.
                if self.unapplied_method_value(a).is_some()
                    && self.no_function_formal_at(&fun_ty_for_pretype, ai)
                {
                    self.adapt_method_value(a);
                }
                // `take(Array.empty)`: with no expected type the argument keeps
                // its residual implicit clause, `(ClassTag[T])Array[T]`. What
                // the callee sees is the *result*; the clause is filled once
                // the parameter has told it what `T` is.
                self.solve_lower_bounded_undet(a);
                arg_tys.push(
                    self.implicit_only_result(a)
                        .or_else(|| self.implicit_eta_shape(a))
                        .unwrap_or_else(|| a.argument_type()),
                );
            }
        }
        self.typing_call_args = saved_taking_args;
        // What the arguments left undetermined. Typing them with no expected
        // type is what makes overload resolution possible, and it is also what
        // leaves `Map.empty` as `Map[K, V]`; those parameters are this call's
        // to solve, so record them before the alternatives are weighed.
        for a in args.iter() {
            let open = self.undetermined_of(a);
            self.undet_tvars.extend(open);
        }

        if !self.library_abi {
            if let TreeKind::Select { name, qual } = &fun.kind {
                if name == "+"
                    && (matches!(qual.ty, Type::String)
                        || arg_tys.first().is_some_and(|t| matches!(t, Type::String)))
                {
                    tree.ty = Type::String;
                    fun.sym = SymbolId::NONE;
                    return;
                }
            }
        }

        if fun_name == "flatMap" && self.is_array_ops_ty(recv_ty.as_ref()) {
            self.bind_array_ops_flat_map(fun, args, recv_ty.as_ref(), &mut arg_tys);
        }
        // Explicit `.apply` on a polymorphic factory result has the same
        // receiver variables as an inserted apply. Solve them before overload
        // applicability replaces open variables with their bounds.
        let factory_apply = match &fun.kind {
            TreeKind::Select { qual, name } if name == "apply" => {
                !self.undetermined_of(qual).is_empty()
            }
            _ => false,
        };
        if factory_apply {
            if let Type::Method { paramss, ret } = &fun.ty {
                if paramss.len() == 1 {
                    let mut params = paramss[0].clone();
                    let mut ret = (**ret).clone();
                    self.instantiate_inserted_apply(
                        fun,
                        &mut params,
                        &mut ret,
                        &arg_tys,
                        pt,
                        tree.span,
                    );
                }
            }
        }
        let fun_ty = fun.ty.clone();
        self.ensure_apply_supplied(&fun_ty, fun.span);

        self.complete_overload_owners(fun);
        // Before the alternatives are weighed, not only after they all fail:
        // a call that *resolves* still has to solve its type parameters from
        // the arguments' base types, and an argument whose class was never
        // completed has none.
        self.complete_arg_classes(&arg_tys);
        // nsc `Typers.preSelectOverloaded`: what an argument *tree* is -- a
        // `{ case … }` literal, some other function literal, or a value --
        // throws out alternatives before their types are weighed. `arg_tys`
        // has one entry per argument, in order, so the two line up.
        let shapes = crate::check_overload::arg_shapes(args);
        // nsc `Infer.inferPolyAlternatives`: what the caller *wrote* as type
        // arguments instantiates every alternative before applicability is
        // weighed. Reaching the pick only afterwards (`pending_targs` below)
        // left each alternative to infer its own instantiation from the value
        // arguments, so `getValue[Integer](p: PropertyType[Integer], 4, false)`
        // solved `T` to the `lub(Integer, Int)` -- `Any` -- and `PropertyType`
        // is invariant, so *both* alternatives were rejected and the call was
        // `no matching overload` where scalac boxes the `4` and picks one.
        let written_targs = explicit_type_args(fun).unwrap_or_default();
        let mut chosen =
            self.resolve_overload_targs(&fun_ty, fun.sym, &arg_tys, pt, &shapes, &written_targs);
        if matches!(chosen, OverloadPick::None) {
            // A *view* can make an argument applicable, but the test for one
            // (`arg_conforms` -> `search_conversion`) runs on `&self` and so
            // cannot read a class file. `Option.option2Iterable` exists only
            // in the library pickle, so `Seq("a") ++ anOption` had no
            // conversion to find -- unless some earlier line in the same file
            // had selected a member on an `Option`, which warms the scope
            // through `search_extension` and made the very same call compile.
            // Warm the arguments' implicit scopes here, where the mutable
            // borrow exists, and ask once more. Only a call that has already
            // failed pays for it, and only the first time per class.
            let tys = arg_tys.clone();
            let mut fresh = false;
            for t in &tys {
                fresh |= self.warm_own_scope_once(t);
            }
            if fresh {
                chosen = self.resolve_overload_targs(
                    &fun_ty,
                    fun.sym,
                    &arg_tys,
                    pt,
                    &shapes,
                    &written_targs,
                );
            }
        }
        'resolve: loop {
            match chosen {
                OverloadPick::Found(sym, mut param_tys, mut ret) => {
                    let mut sig_param_tys = param_tys.clone();
                    // The callee's own type parameters this call has already
                    // solved -- populated below, once inference has run. A
                    // *self*-recursive call (`def show[A, B](tree: Tree[A, B]):
                    // Tree[A, B] = show(tree.left, ...)`) legitimately solves its
                    // own `A`/`B` to themselves: the argument's type is written in
                    // terms of the very type parameters being solved for, so the
                    // fixed point is the correct answer, not a failure to solve.
                    // Recording *which* type parameters were solved (regardless of
                    // what they solved to) is what tells `open_tparams_of` below
                    // not to re-open them to their bounds just because the
                    // substitution left their own symbol mentioned in the
                    // parameter type -- which an identity solution always does.
                    let mut solved_own_tparams: Vec<SymbolId> = Vec::new();
                    // The overload set was recorded under the symbol the
                    // *selection* left on the callee, which the pick below
                    // overwrites. Keep it: it is the key to the alternatives as
                    // seen from this receiver.
                    let group_key = fun.sym;
                    self.select_overloaded_module_apply(fun, sym);
                    if !sym.is_none() {
                        fun.sym = sym;
                        tree.sym = sym;
                        // Codegen's `peel_fun` walks through a `TypeApply` to the
                        // `Select`/`Ident` underneath and reads *that* node's
                        // symbol, so an overload resolved here has to reach it too.
                        // `Array.ofDim[Double](2, 3)` picked the two-dimensional
                        // alternative here and still emitted a call to the
                        // one-dimensional `ofDim(I, ClassTag)Object`.
                        if let TreeKind::TypeApply { fun: inner, .. } = &mut fun.kind {
                            if inner.sym != sym {
                                inner.sym = sym;
                                inner.ty = self.st.get(sym).ty.clone();
                            }
                        }
                        // nsc (SLS 6.26.3): explicit type arguments *are* the
                        // instantiation. `TypeApply` applies them itself when it
                        // can name one alternative, but it cannot when several
                        // take the same number of type parameters -- all five
                        // `Array.ofDim` alternatives take one -- so the reference
                        // arrives here still overloaded and the arguments the user
                        // wrote have reached nothing. Without this,
                        // `Array.ofDim[Double](2, 2)` stayed `Array[Array[T]]` and
                        // every use of an element reported `required: T`.
                        let pending_targs = matches!(&fun.ty, Type::Overload(_))
                            .then(|| explicit_type_args(fun))
                            .flatten();
                        // Remaining clauses (`Using.resources(a, b)(f)`) read `fun.ty`.
                        // Leave a Method type, not the Overload that selected this alt.
                        //
                        // The alternative *as seen from the receiver*, not the raw
                        // declaration: `fill_defaults_and_implicits` reads the
                        // later clauses off this type, and the declaration states
                        // them in the declaring class's own parameters. cats-effect's
                        // `GenTemporalOps_[F[_], A].timeoutTo` is overloaded on
                        // `Duration` / `FiniteDuration`, so its
                        // `(implicit F: GenTemporal[F, _])` reached the search with
                        // `GenTemporalOps_`'s `F` instead of the caller's, and no
                        // candidate could ever match it (slick's
                        // `ConcurrencyControl.scala`). A non-overloaded member kept
                        // the substituted type all along, which is why only
                        // overloaded ones were affected.
                        if matches!(&fun.ty, Type::Overload(_)) {
                            fun.ty = self
                                .overload_member_types
                                .get(&group_key.0)
                                .and_then(|alts| {
                                    alts.iter().find(|(s, _)| *s == sym).map(|(_, t)| t.clone())
                                })
                                .filter(|t| matches!(t, Type::Method { .. }))
                                .unwrap_or_else(|| self.st.get(sym).ty.clone());
                        }
                        // Overload resolution already returns the member as seen
                        // from its receiver. Applying owner arguments again
                        // turns FK[E, F]'s G[A] into F[A], then wrongly E[A]
                        // when the argument names the owner's own F parameter.

                        sig_param_tys = param_tys.clone();
                        self.apply_open_views(sym, &param_tys, args, &mut arg_tys);
                        if !self.st.get(sym).tparams.is_empty() {
                            let inst = self.infer_method_tparams_in(
                                sym,
                                &param_tys,
                                &arg_tys,
                                recv_ty.as_ref(),
                            );
                            // nsc's `adjustTypeArgs`: a `Nothing` the arguments
                            // inferred is *retracted* -- the parameter stays
                            // undetermined for the expected type or the enclosing
                            // expression to decide -- unless the parameter occurs
                            // covariantly in the result, where `Nothing` is the
                            // best answer there is. `tryBreakable { throw … }`'s
                            // `T` (invariant in `TryBlock[T]`) is one the
                            // following `catchBreak { println }` decides;
                            // `inv(fail()).or("a")` on `def inv[T](body: => T):
                            // Inv[T]` is the same shape, and `.or("a")` says
                            // `T = String`. The rule used to be spelled for
                            // `tryBreakable` by name.
                            let inst: Vec<(SymbolId, Type)> = inst
                                .into_iter()
                                .filter(|(tp, t)| {
                                    !matches!(t, Type::Nothing)
                                        || !self.nothing_solution_retracted(*tp, &ret, pt)
                                })
                                .collect();
                            // A function literal has not been typed yet; the
                            // placeholder `(<notype>) => <notype>` standing in for
                            // it is not a solution, and taking it for one hides
                            // the expected type from the parameter the lambda
                            // fills.
                            let inst: Vec<(SymbolId, Type)> = inst
                                .into_iter()
                                .filter(|(_, t)| !mentions_no_type(t))
                                .collect();
                            // Explicit type arguments that could not be applied at
                            // the `TypeApply` (see `pending_targs`) override what
                            // the arguments alone could infer.
                            let inst = match &pending_targs {
                                Some(targs) if targs.len() == self.st.get(sym).tparams.len() => {
                                    self.st
                                        .get(sym)
                                        .tparams
                                        .clone()
                                        .into_iter()
                                        .zip(targs.iter().cloned())
                                        .collect()
                                }
                                _ => inst,
                            };
                            // A typed sibling supplies a lower constraint, not a
                            // final solution for a result still produced by an
                            // untyped lambda. Keep those variables open until all
                            // bodies participate in the second inference pass.
                            let inst: Vec<_> = inst
                                .into_iter()
                                .filter(|(tp, _)| {
                                    pending_targs.is_some()
                                        || param_tys.iter().zip(&arg_tys).any(|(p, a)| {
                                            !mentions_no_type(a)
                                                && matches!(
                                                    self.tparam_variance_in(p, *tp, 1),
                                                    Some(0 | -1)
                                                )
                                        })
                                        || !param_tys.iter().zip(&arg_tys).any(|(p, a)| {
                                            if !mentions_no_type(a) {
                                                return false;
                                            }
                                            match p {
                                                Type::Function { params, ret } => {
                                                    self.tparam_variance_in(ret, *tp, 1) == Some(1)
                                                        && !params
                                                            .iter()
                                                            .any(|p| type_mentions_tparam(p, *tp))
                                                }
                                                _ => false,
                                            }
                                        })
                                })
                                .collect();
                            // The expected type is a constraint too. Solve it here,
                            // before the implicit clauses are filled: slick's
                            // `def column[T](n: Node)(implicit tt: TypedType[T]): Rep[T]`
                            // gets `T` from nowhere else.
                            let inst = self.add_expected_constraints(sym, &ret, pt, inst);

                            // nsc reads the expected type *after* the arguments
                            // are typed. Here the pass runs first, so a solution
                            // the expected type only knows as `_` -- the stand-in
                            // an enclosing call put there for a variable it has
                            // not decided -- would fix a parameter an argument
                            // still to be typed is about to state exactly. Only
                            // inside such an argument (`relaxed_pt_depth`): a
                            // wildcard the program wrote says as much as any
                            // other type, and `build { case (sq, cs) => … }` at
                            // a declared `Cache[(Seq[String], Class[_]), String]`
                            // has nothing else to give the pattern its types
                            // (`pos/t12899`). A parameter no remaining argument
                            // mentions keeps the wildcard either way.
                            let inst: Vec<(SymbolId, Type)> = inst
                                .into_iter()
                                .filter(|(tp, t)| {
                                    self.relaxed_pt_depth == 0
                                        || !type_has_wildcard(t)
                                        || !param_tys.iter().zip(&arg_tys).any(|(p, a)| {
                                            mentions_no_type(a) && type_mentions_tparam(p, *tp)
                                        })
                                })
                                .collect();
                            self.check_tparam_bounds(sym, &inst, recv_ty.as_ref(), tree.span, true);
                            if !inst.is_empty() {
                                let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                                let args_t: Vec<Type> =
                                    inst.iter().map(|(_, t)| t.clone()).collect();
                                solved_own_tparams = tps.clone();
                                param_tys = param_tys
                                    .iter()
                                    .map(|p| crate::symbol::subst_tparams_slice(&tps, &args_t, p))
                                    .collect();
                                ret = crate::symbol::subst_tparams_slice(&tps, &args_t, &ret);
                                // The later clauses are read back off `fun.ty` by
                                // `fill_defaults_and_implicits`; leaving it raw
                                // would search `ClassTag[T]` after `T` is known.
                                fun.ty = crate::symbol::subst_tparams_slice(&tps, &args_t, &fun.ty);
                            }
                            // The parameter types the signature really declares.
                            // What follows rewrites `param_tys` to get the lambda
                            // arguments typed (`A => Any` instead of `A => B`, or
                            // the expected type's `Int => Int` for a parameter
                            // that is only an upper bound), which loses the very
                            // parameter the second inference pass has to solve.
                            sig_param_tys = param_tys.clone();
                            // A parameter that only occurs *covariantly* in the
                            // result is a mere upper bound, so it must not fix the
                            // result type (nsc leaves `def cov[T]: List[T]`
                            // checked against `List[Any]` at `T = Nothing`). It is
                            // still what an argument has to be checked against,
                            // though: `Tuple2(1, n => n + 1)` expected to be
                            // `(Int, Int => Int)` can only give `n` a type this
                            // way. Applied to the parameter types alone; the
                            // result is re-inferred from the typed arguments.
                            let open: Vec<SymbolId> = self
                                .st
                                .get(sym)
                                .tparams
                                .iter()
                                .copied()
                                .filter(|tp| !inst.iter().any(|(id, _)| id == tp))
                                .collect();
                            if !open.is_empty() && args.iter().any(is_bare_lambda) {
                                let weak = self.add_expected_constraints_in(
                                    sym,
                                    &ret,
                                    pt,
                                    Vec::new(),
                                    true,
                                );
                                // Not a solution the expected type only knows as
                                // `_`: that wildcard is the stand-in an enclosing
                                // call left for a variable *it* has not decided,
                                // and writing it into the parameter both tells
                                // the literal nothing and hides the variable from
                                // `open_tparams_of` below, so the literal's own
                                // answer never reaches the result. cats'
                                // `F.map(f(a0).value) { case … }` inside
                                // `EitherT`/`IorT`/`OptionT`'s `tailRecM` came
                                // out `F[_]` this way. Again only inside a
                                // relaxed expected type -- a wildcard the
                                // program wrote is a type like any other.
                                let drop_wild = self.relaxed_pt_depth > 0;
                                let (ids, vals): (Vec<SymbolId>, Vec<Type>) = weak
                                    .into_iter()
                                    .filter(|(id, v)| {
                                        open.contains(id) && !(drop_wild && type_has_wildcard(v))
                                    })
                                    .unzip();
                                if !ids.is_empty() {
                                    param_tys = param_tys
                                        .iter()
                                        .map(|p| crate::symbol::subst_tparams_slice(&ids, &vals, p))
                                        .collect();
                                }
                            }
                        }
                    }
                    if let Some(elem) = recv_ty.as_ref().and_then(|t| self.elem_type(t)) {
                        if matches!(
                            fun_name.as_str(),
                            "map" | "flatMap" | "foreach" | "withFilter" | "pipe" | "tap"
                        ) && !param_tys.is_empty()
                        {
                            if let Type::Function {
                                params: fp,
                                ret: fr,
                            } = &param_tys[0]
                            {
                                // `List.flatMap[B](f: A => IterableOnce[B])`: B is
                                // only determined by the lambda body, so the body
                                // must not be checked against `IterableOnce[B]`.
                                //
                                // A `Wildcard`, not `Any`: nsc's `typedFunction`
                                // types the body against `WildcardType` when the
                                // result is not fully defined, and `type_function`
                                // reads a `Wildcard` result as "no expected type".
                                // Against `Any` the body *was* checked -- `x => if
                                // (x > 1) 1L else 0` boxed both branches and
                                // `List(1, 2).map(...)` came out `List[AnyVal]`
                                // where scalac's weak-conformance lub says
                                // `List[Long]` (a silent runtime difference:
                                // `List(Integer, Long)` element classes).
                                let undetermined = !sym.is_none()
                                    && mentions_tparam(fr, &self.st.get(sym).tparams);
                                let fret =
                                    if matches!(fr.as_ref(), Type::TypeParam(_)) || undetermined {
                                        Box::new(Type::Wildcard)
                                    } else {
                                        fr.clone()
                                    };
                                // The first type argument is the element only when
                                // it is a *proper* type. cats' syntax classes are
                                // `Ops[F[_], A]`, so `args[0]` is the constructor
                                // `F`, and taking it for the element gave
                                // `Ops[Box, Int].flatMap`'s lambda the parameter
                                // type `Box` where the declaration says `Int`.
                                //
                                // And only for a *one-argument* function whose
                                // parameter the declaration has not already
                                // settled. `LazyZip2[A, B, C].map(f: (A, B) => R)`
                                // takes two, and replacing them with one element
                                // type made `xs.lazyZip(ys).map((a, b) => …)`
                                // "found (String, Int) => String, required
                                // (String) => Any". `Iterator[A].grouped(n)` hands
                                // back an `Iterator.GroupedIterator[B]` whose
                                // element type is `Seq[B]`, not `B`: the first type
                                // argument is the element for the collections this
                                // rule was written for, and a guess about which
                                // must not overrule a parameter type the signature
                                // states outright. Before that,
                                // `it.grouped(2).map { case Seq(i, t) => … }` typed
                                // its lambda against `Int` and emitted a
                                // `checkcast` that is a `VerifyError` at run time.
                                //
                                // A *rigid* type parameter is settled too. Read
                                // through the receiver, `class Vd[+E, +A] { def
                                // map[B](f: A => B) }` states its parameter as
                                // the caller's own `A`, which is in scope here
                                // and cannot be a variable; the guess then
                                // overruled it with `args[0]` -- the `E` -- and
                                // every `fa.map(f)` on a two-parameter covariant
                                // class reported `found: (A) => B  required:
                                // (E) => Any`. The guess is for a parameter
                                // still written in the *declaring* class's own
                                // parameter, which is not in scope at the call
                                // site.
                                //
                                // That holds inside a larger type as well:
                                // `Node[K, V]`'s own
                                // `foreach[U](f: ((K, V)) => U)`, called as
                                // `_next.foreach(f)` in mutable `HashMap`, states
                                // `(K, V)` with both parameters in scope, and the
                                // guess (`Node` is in `scala/collection/` but no
                                // collection, so its first argument `K`) made it
                                // "found: ((K, V)) => U  required: (K) => Any".
                                let rigid = |tp: &SymbolId| {
                                    self.tparam_in_scope(*tp)
                                        && !self.undet_tvars.contains(tp)
                                        && !self.st.get(sym).tparams.contains(tp)
                                };
                                let settled = fp.len() == 1 && {
                                    let mut open = Vec::new();
                                    collect_tparams(&fp[0], &mut open);
                                    open.iter().all(rigid)
                                        && !fp[0].is_no_type()
                                        && !matches!(
                                            fp[0],
                                            Type::Named { .. }
                                                | Type::Any
                                                | Type::AnyRef
                                                | Type::TypeMember(_)
                                        )
                                };
                                let fparams = if fp.len() == 1
                                    && !settled
                                    && self.st.kind_arity(&elem) == 0
                                {
                                    vec![elem.clone()]
                                } else {
                                    fp.clone()
                                };
                                param_tys[0] = Type::Function {
                                    params: fparams,
                                    ret: fret,
                                };
                            }
                        }
                        if fun_name == "collect" && !param_tys.is_empty() {
                            if let Type::Class { sym, args } = &param_tys[0] {
                                if is_partial_function_sym(&self.st, *sym) {
                                    let to = args.get(1).cloned().unwrap_or(Type::Any);
                                    param_tys[0] = Type::Class {
                                        sym: *sym,
                                        args: vec![elem, to],
                                    };
                                }
                            }
                        }
                    }
                    // Excluding `solved_own_tparams`: a type parameter this call's
                    // inference already solved -- even to itself, the fixed point
                    // a self-recursive call's own type parameters land on -- is
                    // not "open" here. Leaving it in made `open_tparams_of` below
                    // see `A`/`B` still mentioned in `Tree[A, B]` (substituting a
                    // solution back onto itself is a no-op) and relax them to
                    // their bounds, so a self-recursive call like RedBlackTree's
                    // `def lookup[A, B](tree: Tree[A, B], x: A): Tree[A, B] = ...
                    // lookup(tree.left, x)` (`scala/collection/immutable/
                    // RedBlackTree.scala`) checked `tree.left: Tree[A, B]` against
                    // an expected `Tree[Any, Any]` and failed.
                    let own_tparams = (!sym.is_none()).then(|| {
                        self.st
                            .get(sym)
                            .tparams
                            .iter()
                            .copied()
                            .filter(|tp| !solved_own_tparams.contains(tp))
                            .collect::<Vec<_>>()
                    });
                    for (i, a) in args.iter_mut().enumerate() {
                        let mut p = param_at(&param_tys, i).cloned().unwrap_or(Type::NoType);
                        // `Using.resource(r)(x => 10)`: A only appears in a later
                        // clause. Type the lambda against `R => _` so the body
                        // is not checked against a raw type parameter -- and not
                        // against `Any` either, which is a real expected type
                        // that boxes an `if (c) 1L else 0` body instead of
                        // letting the weak-conformance lub make it a `Long`
                        // (nsc's `typedFunction` uses `WildcardType` there).
                        // The parameter may be spelled as the `Function1[A, B]`
                        // class by a pickled signature; same type, same rule.
                        let p_fn = match &p {
                            Type::Class { sym, args } => self.st.function_class_shape(*sym, args),
                            _ => None,
                        };
                        if let Type::Function { params, ret } = p_fn.as_ref().unwrap_or(&p) {
                            if !params.is_empty() && matches!(ret.as_ref(), Type::TypeParam(_)) {
                                p = Type::Function {
                                    params: params.clone(),
                                    ret: Box::new(Type::Wildcard),
                                };
                            }
                        }
                        // The callee's own type parameters that this call has not
                        // solved are variables too (nsc's `undetparams`):
                        // `xs.collect { case … }` is checked against
                        // `PartialFunction[Int, ?B]`. A variable constrains
                        // nothing, so the literal is *typed* against the parameter
                        // with the variables opened up to their bounds.
                        let open = self.open_tparams_of(&p, own_tparams.as_deref());
                        if a.ty.is_no_type() {
                            // A variable inside the *result* of a function-typed
                            // parameter is one the argument itself decides:
                            // `def h[B](f: Int => Bx[B])` states `B` nowhere else.
                            // Opened to its bound the body was checked against
                            // `Bx[Any]`, and an invariant `Bx[Int]` is not that --
                            // the argument was rejected before the second
                            // inference pass could read `B` off it. A wildcard is
                            // what "not decided yet" means in a position
                            // `is_sub_type` already understands, and unlike
                            // relaxing the whole result to `Any` it still tells
                            // the body that it must be a `Bx`. Only the expected
                            // type is relaxed: `p` itself stays the declaration,
                            // so `solve_open_from_arg` below still reads `B` off
                            // the typed argument. slick's `DBIOAction.flatMap[R2,
                            // S2, E2](f: R => DBIOAction[R2, S2, E2])` is this
                            // shape.
                            // A pickled signature spells the parameter as the
                            // `Function1[A, B]` class (`Option.map`, `Try.map`);
                            // it is the same type, and it has to be relaxed the
                            // same way, or `B` was opened to its bound `Any` and
                            // the literal's `if (c) 1.0 else 0` body was boxed
                            // against it instead of taking the numeric lub.
                            let p_shape = match &p {
                                Type::Class { sym, args } => self
                                    .st
                                    .function_class_shape(*sym, args)
                                    .unwrap_or_else(|| p.clone()),
                                _ => p.clone(),
                            };
                            let relaxed = match &p_shape {
                                Type::Function { params, ret } if mentions_tparam(ret, &open) => {
                                    let wilds = vec![Type::Wildcard; open.len()];
                                    // A function result can determine its own
                                    // input types too, as in Deferred(() => saved)
                                    // with saved: A => B. Opening its input to
                                    // Any (or _) reverses the constraint through
                                    // contravariance before A can be inferred.
                                    let result = if matches!(ret.as_ref(),
                                        Type::Function { params, .. }
                                            if params.iter().any(|p| mentions_tparam(p, &open)))
                                    {
                                        Type::Wildcard
                                    } else {
                                        crate::symbol::subst_tparams_slice(&open, &wilds, ret)
                                    };
                                    Type::Function {
                                        params: params.clone(),
                                        ret: Box::new(result),
                                    }
                                }
                                _ => p.clone(),
                            };
                            // A literal's *parameter* whose expected type is
                            // such a variable would be typed at the bound. nsc
                            // reads it off the literal's own body first when
                            // that body applies something to the parameter
                            // (`typedFunctionUndoingEtaExpansion`).
                            let relaxed = match &relaxed {
                                Type::Function { params, ret } => {
                                    match self.undo_eta_param_types(a, params, &open) {
                                        Some(params) => Type::Function {
                                            params,
                                            ret: ret.clone(),
                                        },
                                        None => relaxed,
                                    }
                                }
                                _ => relaxed,
                            };
                            let pt_arg = self.open_to_bounds(&relaxed, &open);
                            // A wildcard this substitution just put in is our
                            // own "not decided yet", not an existential the
                            // program wrote. Say so for as long as the argument
                            // is being typed, so the calls inside it do not read
                            // their own type parameters out of it.
                            let relaxed_here = type_has_wildcard(&pt_arg) && !type_has_wildcard(&p);
                            if relaxed_here {
                                self.relaxed_pt_depth += 1;
                            }
                            self.type_expr(a, &pt_arg);
                            if relaxed_here {
                                self.relaxed_pt_depth -= 1;
                            }
                        }
                        // nsc adapts an argument before it constrains the call. An
                        // argument that still carries an all-implicit clause is not
                        // a value yet, and the witness is what pins the *argument
                        // method's* own parameters: `one(paths.toMap)` fixed this
                        // call's `A2` from the residual
                        // `(A <:< (K, V))Map[K, V]`, and only then found the
                        // witness -- so the parameter it had to conform to stayed
                        // `Map[K, V]` while the argument had become
                        // `Map[String, Int]`. Filling it here lets the open-variable
                        // substitution below carry `K`/`V` into the parameter, the
                        // result and the receiver, exactly as for any other
                        // argument that pins one.
                        if !a.ty.is_no_type()
                            && !a.ty.is_error()
                            && self.implicit_only_result(a).is_some()
                        {
                            let pt_arg = self.open_to_bounds(&p, &open);
                            self.adapt_implicit_apply(a, &pt_arg);
                        }
                        // A *receiver* carries undetermined variables too:
                        // `ConstArray.newBuilder()` is a `ConstArrayBuilder[?T]`,
                        // and the argument of the call made on it (`b + from`) is
                        // what fixes `?T`. nsc keeps them in `Context.undetparams`
                        // until something does; without this the parameter stayed
                        // a bare `T` and every `+` reported a mismatch.
                        if !p.is_no_type() && !a.ty.is_no_type() && !a.ty.is_error() {
                            let open_recv: Vec<SymbolId> = self
                                .undet_tvars
                                .iter()
                                .copied()
                                .filter(|tp| type_mentions_tparam(&p, *tp))
                                .collect();
                            let mut ids = Vec::new();
                            let mut vals = Vec::new();
                            // An `Array` argument reaches a collection parameter
                            // through one of `Predef`'s wrappings, and it is the
                            // *wrapped* type that lines up with the parameter:
                            // `Map() ++ arrayOfPairs` has to read `?K` and `?V`
                            // out of `mutable.ArraySeq[(Int, String)]`, not out of
                            // an `Array` no `IterableOnce[…]` can be matched
                            // against (`seqfn_view.rs`).
                            let mut froms = vec![a.ty.widen_constant()];
                            if let Type::Array(elem) = &a.ty {
                                froms.extend(
                                    self.array_wrap_candidates(elem).into_iter().map(|(_, v)| v),
                                );
                            }
                            for tp in open_recv {
                                let hit = froms
                                    .iter()
                                    .find_map(|from| unify_one(&self.st, tp, &p, from))
                                    .filter(|t| {
                                        !t.is_no_type()
                                            && !t.is_error()
                                            && !type_mentions_tparam(t, tp)
                                    });
                                if let Some(t) = hit {
                                    // nsc's `instantiateExpecting`: where the
                                    // variable occurs *invariantly* in the result,
                                    // the expected type outranks what the argument
                                    // said -- as long as the argument's own
                                    // solution still conforms to it. `Set() ++
                                    // dbType.map(SqlType(_))` checked against
                                    // `Set[ColumnOption[_]]` reads `?A` off the
                                    // argument as `SqlType` and an invariant `Set`
                                    // then rejects the whole call
                                    // (`jdbc/JdbcModelBuilder.scala:279`). The
                                    // callee's *own* parameters already get this
                                    // treatment in `add_expected_constraints`; a
                                    // receiver's did not.
                                    // A bare wildcard in the expected type is the
                                    // enclosing call's own undecided variable (a
                                    // lenient prototype's `WildcardType`), and
                                    // everything conforms to it; it is not a
                                    // better answer than the argument's. cats'
                                    // `leftWiden(rightFunctor.widen(fac))` typed
                                    // `widen` at `F[_, D]` and took `X := _`.
                                    let t = match unify_one(&self.st, tp, &ret, pt) {
                                        Some(e)
                                            if e != t
                                                && !e.is_no_type()
                                                && !e.is_error()
                                                && !matches!(
                                                    e,
                                                    Type::Wildcard | Type::BoundedWildcard { .. }
                                                )
                                                && !type_mentions_tparam(&e, tp)
                                                && self.tparam_variance_in(&ret, tp, 1)
                                                    == Some(0)
                                                && self.st.is_sub_type(&t, &e) =>
                                        {
                                            e
                                        }
                                        _ => t,
                                    };
                                    ids.push(tp);
                                    vals.push(t);
                                }
                            }
                            if !ids.is_empty() {
                                // Receiver variables retain their declaration
                                // bounds when the member's arguments solve them.
                                let mut owners: Vec<_> =
                                    ids.iter().map(|id| self.st.get(*id).owner).collect();
                                owners.sort_by_key(|id| id.0);
                                owners.dedup();
                                for owner in owners {
                                    if owner.is_none() {
                                        continue;
                                    }
                                    let inst: Vec<_> = ids
                                        .iter()
                                        .zip(&vals)
                                        .filter(|(id, _)| self.st.get(**id).owner == owner)
                                        .map(|(id, ty)| (*id, ty.clone()))
                                        .collect();
                                    self.check_tparam_bounds(owner, &inst, None, a.span, true);
                                }
                                p = crate::symbol::subst_tparams_slice(&ids, &vals, &p);
                                param_tys = param_tys
                                    .iter()
                                    .map(|q| crate::symbol::subst_tparams_slice(&ids, &vals, q))
                                    .collect();
                                ret = crate::symbol::subst_tparams_slice(&ids, &vals, &ret);
                                recv_ty = recv_ty
                                    .map(|t| crate::symbol::subst_tparams_slice(&ids, &vals, &t));
                                // The later clauses are read back off `fun.ty`
                                // (`fill_defaults_and_implicits`), as for the
                                // callee's own solutions above. cats'
                                // `first(fa).dimap((_: (C, A)).swap)(_.swap)`
                                // solves `first`'s `C` from `dimap`'s first
                                // clause, and the second clause still expected a
                                // `((B, C)) => S1` over the unsolved `C`: the
                                // literal came back `found: ((B, C)) =>
                                // Tuple2[C, B]  required: ((B, C)) => (C, B)`,
                                // two different `C`s printed alike.
                                fun.ty = crate::symbol::subst_tparams_slice(&ids, &vals, &fun.ty);
                                self.undet_tvars.retain(|tp| !ids.contains(tp));
                            }
                        }
                        // The alternative is picked; the argument's own
                        // undetermined variables can be solved now. `take(Map
                        // .empty)` on `take(m: Map[String, Int])` turns `Map[K, V]`
                        // into `Map[String, Int]` here -- the same step the
                        // constructor path already takes. A parameter that is
                        // itself still an open type parameter of the callee pins
                        // nothing, so it is left for the callee's own inference.
                        if !p.is_no_type()
                            && !own_tparams
                                .as_deref()
                                .is_some_and(|tps| mentions_tparam(&p, tps))
                        {
                            a.ty = self.instantiate_parameterless(a.sym, a.ty.clone(), &p);
                            self.instantiate_undet_arg(a, &p);
                        }
                        // Adapt against the *solved* parameter, not against one
                        // whose open variables have been erased to `Any`: the
                        // argument is what tells this call what `?B` is, and the
                        // erasure is what let a wrong `Any` reach the result three
                        // times before. A variable the argument does not pin is
                        // still open, and the check falls back to its bound.
                        if !p.is_no_type() {
                            let p_check = self
                                .solve_open_from_arg(&a.argument_type(), &p, &open)
                                .unwrap_or_else(|| self.open_to_bounds(&p, &open));
                            // Now that the parameter is known, an argument that
                            // still carries an all-implicit clause can have it
                            // filled: `take(Array.empty)` searches
                            // `ClassTag[String]`, not `ClassTag[T]`.
                            if self.implicit_only_result(a).is_some() {
                                self.adapt_implicit_apply(a, &p_check);
                            }
                            self.adapt(a, &p_check);
                        }
                        if let TreeKind::Function { body, .. } = &mut a.kind {
                            let mut body_ty = body.ty.widen_constant();
                            // A relaxed result such as `Eval[_]` gives a
                            // generic constructor the wildcard as its own
                            // type argument, so the lambda carries
                            // `Eval[_]` back into the enclosing call. Retry
                            // only that body without the provisional
                            // expectation; if it yields a concrete type,
                            // that is the result the method type parameter
                            // must be inferred from. Failed retries keep the
                            // original typed tree and diagnostics.
                            if type_has_wildcard(&body_ty) {
                                let saved_body = body.clone();
                                let mark = self.diags.len();
                                self.type_expr(body, &Type::NoType);
                                let candidate = body.ty.widen_constant();
                                if self.error_count_since(mark) == 0
                                    && !candidate.is_no_type()
                                    && !candidate.is_error()
                                    && !type_has_wildcard(&candidate)
                                {
                                    body_ty = candidate;
                                } else {
                                    *body = saved_body;
                                    self.diags.truncate(mark);
                                }
                            }
                            if let Type::Function { params, ret } = &a.ty {
                                // A wildcard here is one the relaxation above put
                                // in, standing for "the body decides"; leaving it
                                // in the argument's type carries it into the
                                // call's own result (`Act[_, _, Effect with _]`).
                                if (matches!(
                                    ret.as_ref(),
                                    Type::Any | Type::NoType | Type::TypeParam(_)
                                ) || type_mentions_wildcard(ret))
                                    && !body_ty.is_no_type()
                                    && !body_ty.is_error()
                                {
                                    let params = params.clone();
                                    a.ty = Type::Function {
                                        params,
                                        ret: Box::new(body_ty),
                                    };
                                }
                            }
                        }
                    }
                    let using_infer = if !sym.is_none() {
                        let s = self.st.get(sym);
                        let n_tps = s.tparams.len();
                        let name = s.name.clone();
                        let owner_jvm = self.st.get(s.owner).jvm_name.clone();
                        n_tps > 0
                            && (name == "resource"
                                || name == "resources"
                                || (name == "apply" && owner_jvm.contains("Using")))
                    } else {
                        false
                    };
                    if using_infer {
                        let now_args: Vec<Type> = args
                            .iter()
                            .map(|a| match &a.ty {
                                Type::Function { params, ret } if params.is_empty() => {
                                    (**ret).clone()
                                }
                                t => t.clone(),
                            })
                            .collect();
                        let orig_params: Vec<Type> = match &fun.ty {
                            Type::Method { paramss, .. } if !paramss.is_empty() => paramss[0]
                                .iter()
                                .map(|p| match p {
                                    Type::ByName(inner) => (**inner).clone(),
                                    other => other.clone(),
                                })
                                .collect(),
                            _ => param_tys.clone(),
                        };
                        let inst = self.infer_method_tparams(sym, &orig_params, &now_args);
                        let inst: Vec<(SymbolId, Type)> = inst
                            .into_iter()
                            .filter(|(_, t)| {
                                !matches!(t, Type::Nothing | Type::NoType)
                                    && !matches!(t, Type::TypeParam(_))
                            })
                            .collect();
                        if !inst.is_empty() {
                            let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                            let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
                            param_tys = param_tys
                                .iter()
                                .map(|p| crate::symbol::subst_tparams_slice(&tps, &args_t, p))
                                .collect();
                            ret = crate::symbol::subst_tparams_slice(&tps, &args_t, &ret);
                            fun.ty = crate::symbol::subst_tparams_slice(&tps, &args_t, &fun.ty);
                        }
                    }
                    let nparams = param_tys.len();
                    if args.len() > nparams && split_repeated(&param_tys).1.is_none() {
                        self.error(
                            tree.span,
                            format!(
                                "too many arguments: expected {}, found {}",
                                nparams,
                                args.len()
                            ),
                        );
                    }
                    if !sym.is_none()
                        && self.st.get(sym).name == "collect"
                        && self.is_array_ops_ty(recv_ty.as_ref())
                    {
                        if let Some(a0) = args.first() {
                            let to = match &a0.ty {
                                Type::Class { args, .. } if args.len() >= 2 => args[1].clone(),
                                Type::Function { ret, .. } => (**ret).clone(),
                                _ => Type::NoType,
                            };
                            let to = to.widen_constant();
                            let tps = self.st.get(sym).tparams.clone();
                            if tps.len() == 1 && !to.is_no_type() && !to.is_error() {
                                let inst = vec![to];
                                param_tys = param_tys
                                    .iter()
                                    .map(|t| crate::symbol::subst_tparams_slice(&tps, &inst, t))
                                    .collect();
                                fun.ty = crate::symbol::subst_tparams_slice(&tps, &inst, &fun.ty);
                                ret = crate::symbol::subst_tparams_slice(&tps, &inst, &ret);
                            }
                        }
                    }
                    if !sym.is_none()
                        && self.st.get(sym).name == "flatMap"
                        && self.is_array_ops_ty(recv_ty.as_ref())
                    {
                        if let Some(a0) = args.first() {
                            if let Type::Function { ret: fr, .. } = &a0.ty {
                                let elem = match fr.as_ref() {
                                    Type::Class { args, .. } if !args.is_empty() => args[0].clone(),
                                    Type::Array(e) => e.as_ref().clone(),
                                    other => other.clone(),
                                };
                                let elem = elem.widen_constant();
                                let tps = self.st.get(sym).tparams.clone();
                                if tps.len() == 1 && !elem.is_no_type() && !elem.is_error() {
                                    let inst = vec![elem];
                                    param_tys = param_tys
                                        .iter()
                                        .map(|t| crate::symbol::subst_tparams_slice(&tps, &inst, t))
                                        .collect();
                                    fun.ty =
                                        crate::symbol::subst_tparams_slice(&tps, &inst, &fun.ty);
                                    ret = crate::symbol::subst_tparams_slice(&tps, &inst, &ret);
                                } else if tps.len() == 2 && !elem.is_no_type() && !elem.is_error() {
                                    let bs = fr.as_ref().clone();
                                    let inst = vec![bs, elem];
                                    param_tys = param_tys
                                        .iter()
                                        .map(|t| crate::symbol::subst_tparams_slice(&tps, &inst, t))
                                        .collect();
                                    fun.ty =
                                        crate::symbol::subst_tparams_slice(&tps, &inst, &fun.ty);
                                    ret = crate::symbol::subst_tparams_slice(&tps, &inst, &ret);
                                }
                            }
                        }
                    }
                    if !sym.is_none()
                        && self.st.get(sym).name == "catchBreak"
                        && self.is_try_block_ty(recv_ty.as_ref())
                    {
                        if let Some(a0) = args.first() {
                            // `tryBreakable`'s T and `TryBlock`'s T are different
                            // symbols. If the op was `Nothing`, ret is still a
                            // method type param; fill it from the handler body.
                            let t = match &a0.kind {
                                TreeKind::Function { body, .. } => body.ty.widen_constant(),
                                _ => unwrap_fn0_or_byname(&a0.ty).widen_constant(),
                            };
                            if !t.is_no_type()
                                && !t.is_error()
                                && matches!(ret, Type::TypeParam(_) | Type::Nothing)
                            {
                                ret = t;
                            }
                        }
                    }
                    // The first pass typed lambda arguments with no expected type,
                    // so a method type parameter that only shows up in a lambda's
                    // *result* (`Either.fold[C]`, `Try.fold[U]`, `Option.fold[B]`,
                    // `def map[R2](f: R => R2): Act[R2, NoStream, E]`) is still
                    // uninstantiated. Now that the arguments carry their real
                    // types, infer it once more.
                    if !sym.is_none() {
                        let tps = self.st.get(sym).tparams.clone();
                        if mentions_tparam(&ret, &tps) {
                            let now: Vec<Type> = args
                                .iter()
                                .enumerate()
                                .map(|(i, a)| {
                                    // A by-name argument is carried as a thunk;
                                    // `=> T` is solved against what the thunk
                                    // yields, not against `() => T`.
                                    match (param_at(&sig_param_tys, i), &a.ty) {
                                        (Some(Type::ByName(_)), Type::Function { params, ret })
                                            if a.byname_thunk && params.is_empty() =>
                                        {
                                            (**ret).clone()
                                        }
                                        _ => a.ty.clone(),
                                    }
                                })
                                .collect();
                            let inst: Vec<(SymbolId, Type)> = self
                                .infer_method_tparams(sym, &sig_param_tys, &now)
                                .into_iter()
                                .filter(|(id, t)| {
                                    // A solution that is *this* call's own variable
                                    // is no solution -- `T := T` leaves the result
                                    // exactly as it was. The caller's type
                                    // parameter is a perfectly good one, though:
                                    // `def const[T](v: T): GR[T] = mk(_ => v)`
                                    // solves `mk`'s `T` to `const`'s, and
                                    // rejecting every `TypeParam` printed the
                                    // result as `GR[T] required GR[T]`.
                                    //
                                    // `Nothing` is held back for the expected
                                    // type to improve on. With no expected
                                    // type, nsc's `adjustTypeArgs` keeps a
                                    // `Nothing` solution undetermined only
                                    // where the variable is not covariant in
                                    // the result -- `tryBreakable { throw e }`
                                    // stays a `TryBlock[?T]` for `catchBreak`
                                    // to decide -- and instantiates it
                                    // otherwise: `onError { e => throw e }` on
                                    // `onError[T](h: Throwable => T):
                                    // PartialFunction[Throwable, T]` is a
                                    // `PartialFunction[Throwable, Nothing]`.
                                    // Leaving `T` in that result made the
                                    // enclosing `try p catch onError { … }` an
                                    // `AnyRef` (`sys/process/ProcessImpl.scala`).
                                    let nothing_ok = pt.is_no_type()
                                        && self.tparam_variance_in(&ret, *id, 1) == Some(1);
                                    !t.is_no_type()
                                        && !t.is_error()
                                        && (!matches!(t, Type::Nothing) || nothing_ok)
                                        && !mentions_tparam(t, &tps)
                                })
                                .collect();
                            if !inst.is_empty() {
                                let ids: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                                let args_t: Vec<Type> =
                                    inst.iter().map(|(_, t)| t.clone()).collect();
                                ret = crate::symbol::subst_tparams_slice(&ids, &args_t, &ret);
                            }
                        }
                    }
                    let leftover =
                        self.fill_defaults_and_implicits(tree.span, args, &param_tys, fun, pt);
                    // nsc's `applyImplicitArgs`: `if (args contains EmptyTree)
                    // setError(tree)`. The witness that was not found is often
                    // the only thing that could have said what one of the
                    // callee's type parameters is -- slick's
                    // `map[F, T, G](f)(implicit shape: Shape[_, F, T, G]):
                    // Query[G, T, C]` is exactly that -- so handing back the
                    // declared result type leaks `T` into the program and every
                    // selection on it reports again. 19 `value _N is not a
                    // member of T` and 13 `value map is not a member of O2` in
                    // gitbucket were that cascade.
                    let impl_missing = std::mem::take(&mut self.implicit_arg_missing);
                    if !self.implicit_undet_solved.is_empty() {
                        let sol = std::mem::take(&mut self.implicit_undet_solved);
                        let ids: Vec<SymbolId> = sol.iter().map(|(i, _)| *i).collect();
                        let ts: Vec<Type> = sol.iter().map(|(_, t)| t.clone()).collect();
                        ret = crate::symbol::subst_tparams_slice(&ids, &ts, &ret);
                    }
                    let method_name = if !sym.is_none() {
                        self.st.get(sym).name.clone()
                    } else {
                        fun_name.clone()
                    };
                    // `::` is `[B >: A](elem: B): List[B]` (see prelude_lowbound);
                    // its result comes from ordinary lower-bounded inference.
                    if method_name == "->" {
                        if let Some(a0) = args.first() {
                            if let Some(t2) =
                                self.st.lookup("Tuple2").into_iter().find(|id| {
                                    self.st.get(*id).kind == crate::symbol::SymKind::Class
                                })
                            {
                                let k = match &fun.kind {
                                    TreeKind::Select { qual, .. } => match &qual.kind {
                                        TreeKind::Apply { args: wargs, .. } => wargs
                                            .first()
                                            .map(|a| a.ty.widen_constant())
                                            .unwrap_or_else(|| qual.ty.widen_constant()),
                                        _ => qual.ty.widen_constant(),
                                    },
                                    _ => Type::Any,
                                };
                                ret = Type::Class {
                                    sym: t2,
                                    args: vec![k, a0.ty.widen_constant()],
                                };
                            }
                        }
                    } else if method_name == "map" && self.map_result_uses_element(sym, &ret, args)
                    {
                        if self.is_array_ops_ty(recv_ty.as_ref()) {
                            if let Some(a0) = args.first() {
                                if let Type::Function { ret: fr, .. } = &a0.ty {
                                    ret = Type::Array(Box::new(fr.as_ref().widen_constant()));
                                }
                            }
                        } else if let Some(t) = self.either_map_result(recv_ty.as_ref(), args) {
                            ret = t;
                        } else if let Some(t) =
                            self.map_with_filter_non_pair_result(recv_ty.as_ref(), args, false)
                        {
                            ret = t;
                        } else if !self.is_with_filter_ty(recv_ty.as_ref()) {
                            // The argument need not be written as a function: a
                            // `Map[K, V]` is one (`on.map(columnIndexes)`), and the
                            // element type of the result is still what it returns.
                            let a0_fn = args.first().and_then(|a| match &a.ty {
                                Type::Function { .. } => Some(a.ty.clone()),
                                other => self.function_view(other),
                            });
                            if let Some(a0) = a0_fn.as_ref() {
                                if let Type::Function { ret: fr, .. } = a0 {
                                    // The declared result wins when it names another
                                    // class: `Range.map` is an `IndexedSeq`, not a
                                    // `Range`.
                                    let declared = match &ret {
                                        Type::Class { sym, args } if args.len() == 1 => Some(*sym),
                                        _ => None,
                                    };
                                    // `Map.map` is `MapOps.map[K2, V2]` when the
                                    // lambda returns a pair: the declaration read
                                    // off `IterableOps` says `Iterable[B]`, and
                                    // `BuildFrom` puts the receiver's own two-
                                    // parameter class back.
                                    let pair_rebuild = declared.and_then(|d| {
                                        let r = self.receiver_collection_root(recv_ty.as_ref())?;
                                        (self.st.get(r).tparams.len() == 2).then_some(()).and_then(
                                            |()| {
                                                self.rebuild_widened(
                                                    r,
                                                    &Type::Class {
                                                        sym: d,
                                                        args: vec![fr.as_ref().widen_constant()],
                                                    },
                                                )
                                            },
                                        )
                                    });
                                    let recv_cls = self
                                        .receiver_collection_root(recv_ty.as_ref())
                                        .filter(|&c| self.takes_one_type_parameter(c));
                                    let cls = match (recv_cls, declared) {
                                        // `IndexedSeq` does not redeclare `map`, so
                                        // the declaration it inherits says `Seq[B]`
                                        // -- but the real signature returns the
                                        // receiver's own type constructor
                                        // (`IterableOps.CC[B]`), and
                                        // `xs.toSeq.map(f)` on an `IndexedSeq` is an
                                        // `IndexedSeq`. Only a `scala.collection`
                                        // class gets that: a user class that merely
                                        // extends `Seq` inherits `Seq`'s `CC` and
                                        // really does map to a `Seq`.
                                        (Some(r), Some(d))
                                            if r != d
                                                && self.maps_to_own_class(r)
                                                && self
                                                    .base_type_instance(
                                                        &Type::Class {
                                                            sym: r,
                                                            args: vec![],
                                                        },
                                                        d,
                                                        0,
                                                    )
                                                    .is_some() =>
                                        {
                                            Some(r)
                                        }
                                        (_, Some(d)) => Some(d),
                                        // The declaration named no
                                        // single-argument class, so there is
                                        // nothing to read the element type
                                        // out of and the receiver's own class
                                        // is the only candidate left. Only a
                                        // `scala.collection` class may take
                                        // it: slick's `Query[+E, U, C[_]]`
                                        // declares `map` as `Query[G, T, C]`,
                                        // and rebuilding *that* against a
                                        // one-parameter receiver turned
                                        // `TableQuery[Accounts].map(_.name)`
                                        // into `TableQuery[Rep[String]]` --
                                        // a type slick's own bound
                                        // (`E <: AbstractTable[_]`) forbids.
                                        (Some(r), None) if self.maps_to_own_class(r) => Some(r),
                                        (_, None) => None,
                                    };
                                    if let Some(t) = pair_rebuild {
                                        ret = t;
                                    } else if let Some(cls) = cls {
                                        ret = Type::Class {
                                            sym: cls,
                                            args: vec![fr.as_ref().widen_constant()],
                                        };
                                    }
                                }
                            }
                        }
                    } else if returns_receiver_collection(&method_name) {
                        // 2.13 declares these as returning `C` (or `CC[B]`) --
                        // the receiver's own collection. The prelude cannot spell
                        // `C`, so `Vector[Phase].filterNot(p)` came back as the
                        // inherited `Seq[Phase]` and `phases ++ ps` as
                        // `IndexedSeq[Phase]`. The element types are the declared
                        // result's; only the class is the receiver's. Same shape
                        // as the `map` rule above, and gated the same way: a
                        // `scala.collection` class that really is a subclass of
                        // what the declaration named.
                        if let Some(r) = self.receiver_collection_root(recv_ty.as_ref()) {
                            // `SeqView` is the one collection whose `C` is not
                            // itself: `trait SeqView[+A] extends SeqOps[A, View,
                            // View[A]] with View[A]`, so `filter` & friends return
                            // a `View`. `javap scala.collection.SeqView` lists the
                            // members it really does override (`map`, `take`,
                            // `drop`, `reverse`, `sorted`, …) and `filter` is not
                            // among them. Rebuilding to the receiver typed
                            // `xs.view.filter(p)` as a `SeqView[A]`, and the
                            // `checkcast` codegen puts on the result threw
                            // `ClassCastException` on the `scala.collection.
                            // View$Filter` the call really returns.
                            let keeps_view =
                                crate::prelude_viewc::declares_view_result(&method_name)
                                    && self.st.get(r).jvm_name == "scala/collection/SeqView";
                            if !keeps_view {
                                let widened_concat =
                                    matches!(method_name.as_str(), "++" | "$plus$plus")
                                        && !sym.is_none()
                                        && !self.st.get(sym).tparams.is_empty();
                                // A map can widen its values while preserving
                                // the key type and its existing Ordering[K].
                                let result_key = match &ret {
                                    Type::Class { args, .. } if args.len() == 2 => {
                                        Some(args[0].clone())
                                    }
                                    // Inherited IterableOps represents a map's
                                    // element as one pair rather than two class
                                    // arguments. Rebuilding the map unwraps it.
                                    Type::Class { args, .. } if args.len() == 1 => self
                                        .pair_args(&args[0])
                                        .and_then(|pair| pair.first().cloned()),
                                    _ => None,
                                };
                                let preserves_keys = match (
                                    recv_ty
                                        .as_ref()
                                        .and_then(|recv| self.base_type_instance(recv, r, 0)),
                                    result_key,
                                ) {
                                    (Some(Type::Class { args: keys, .. }), Some(key))
                                        if keys.len() == 2 =>
                                    {
                                        keys[0] == key
                                    }
                                    _ => false,
                                };
                                let rebuilt = if widened_concat && !preserves_keys {
                                    self.rebuild_widened(r, &ret)
                                } else {
                                    self.rebuild_from_receiver(r, &ret)
                                };
                                if let Some(t) = rebuilt {
                                    ret = t;
                                }
                                // `updated` / `:+` / `+:` / `padTo` take
                                // `[B >: A]` and return `CC[B]`: the element
                                // is at least the receiver's. The declarations
                                // that reach here lose that bound (a prelude
                                // stand-in's `updated(Int, Any): Vector[A]`, or
                                // a pickled `B` solved from the argument alone),
                                // and `Vector(1, 2).updated(0, "s")` came back a
                                // `Vector[Int]` whose `(0)` was unboxed as an
                                // `Int` -- ClassCastException.
                                if let Some(t) = self.widen_to_receiver_elem(
                                    &method_name,
                                    sym,
                                    recv_ty.as_ref(),
                                    &ret,
                                    args,
                                ) {
                                    ret = t;
                                }
                            }
                        }
                    } else if method_name == "pipe" {
                        if let Some(a0) = args.first() {
                            if let Type::Function { ret: fr, .. } = &a0.ty {
                                let t = fr.as_ref().widen_constant();
                                if !t.is_no_type() && !t.is_error() {
                                    ret = t;
                                }
                            }
                        }
                    } else if method_name == "collect" {
                        if let Some(a0) = args.first() {
                            let to = match &a0.ty {
                                Type::Class { args, .. } if args.len() >= 2 => {
                                    Some(args[1].clone())
                                }
                                Type::Function { ret, .. } => Some((**ret).clone()),
                                _ => None,
                            };
                            if let Some(to) = to {
                                if self.is_array_ops_ty(recv_ty.as_ref()) {
                                    ret = Type::Array(Box::new(to.widen_constant()));
                                } else if let Some(cls) = self
                                    .receiver_collection_root(recv_ty.as_ref())
                                    .filter(|&c| self.takes_one_type_parameter(c))
                                {
                                    ret = Type::Class {
                                        sym: cls,
                                        args: vec![to.widen_constant()],
                                    };
                                } else if let Some(r) =
                                    self.receiver_collection_root(recv_ty.as_ref())
                                {
                                    // `MapOps.collect[K2, V2](pf): CC[K2, V2]` --
                                    // the `Map` counterpart of `map` above.
                                    let named = match &ret {
                                        Type::Class { sym, args } if args.len() == 1 => {
                                            Some(Type::Class {
                                                sym: *sym,
                                                args: vec![to.widen_constant()],
                                            })
                                        }
                                        _ => None,
                                    };
                                    if let Some(t) = named.and_then(|n| self.rebuild_widened(r, &n))
                                    {
                                        ret = t;
                                    }
                                }
                            }
                        }
                    } else if method_name == "zip" {
                        if self.is_array_ops_ty(recv_ty.as_ref()) {
                            if let Some(a0) = args.first() {
                                if let Some(b) = self.elem_type(&a0.ty) {
                                    let a = recv_ty
                                        .as_ref()
                                        .and_then(|t| self.elem_type(t))
                                        .unwrap_or(Type::Any);
                                    let t2 = self.tuple2_sym();
                                    if !t2.is_none() {
                                        ret = Type::Array(Box::new(Type::Class {
                                            sym: t2,
                                            args: vec![a, b.widen_constant()],
                                        }));
                                    }
                                }
                            }
                        } else if let Some(r) = self.receiver_collection_root(recv_ty.as_ref()) {
                            // `zip[B](that): CC[(A, B)]`.
                            if let Some(t) = self.rebuild_from_receiver(r, &ret) {
                                ret = t;
                            }
                        }
                    } else if method_name == "flatMap" {
                        if self.is_array_ops_ty(recv_ty.as_ref()) {
                            if let Some(a0) = args.first() {
                                if let Type::Function { ret: fr, .. } = &a0.ty {
                                    let elem = match fr.as_ref() {
                                        Type::Class { args, .. } if !args.is_empty() => {
                                            args[0].clone()
                                        }
                                        Type::Array(e) => e.as_ref().clone(),
                                        other => other.clone(),
                                    };
                                    ret = Type::Array(Box::new(elem.widen_constant()));
                                }
                            }
                        } else if let Some(t) =
                            self.map_with_filter_non_pair_result(recv_ty.as_ref(), args, true)
                        {
                            ret = t;
                        } else if let Some(a0) = args.first() {
                            // Only where ordinary inference left the result open.
                            // `List.flatMap[B](f: A => IterableOnce[B]): List[B]`
                            // is a real signature, and once `B` is solved the
                            // lambda's own type has been widened to the parameter
                            // type -- taking it for the result turned a
                            // `List[String]` into an `IterableOnce[String]`.
                            let open = sym.is_none()
                                || mentions_tparam(&ret, &self.st.get(sym).tparams)
                                || ret.is_no_type();
                            if open {
                                if let Type::Function { ret: fr, .. } = &a0.ty {
                                    ret = (**fr).clone();
                                }
                            }
                            // `flatMap` is `CC[B]` like `map` is: the class is the
                            // receiver's, whatever class the inherited declaration
                            // named. `IndexedSeq.flatMap` said `Seq[B]`, and a
                            // `Map`'s said `Iterable[(K2, V2)]`.
                            if let Some(r) = self.receiver_collection_root(recv_ty.as_ref()) {
                                if let Some(t) = self.rebuild_widened(r, &ret) {
                                    ret = t;
                                }
                            }
                        }
                    } else if method_name == "withFilter" {
                        if !self.is_with_filter_ty(Some(&ret)) {
                            if let Some(r) = recv_ty.clone() {
                                // Only where the declared result is the receiver
                                // *widened* -- `Iterable.withFilter` reached
                                // through a `List`. A `withFilter` returning
                                // something the receiver is not (slick's
                                // `ConstArray.withFilter(p): ConstArrayOp[T]`)
                                // keeps its own result; replacing it made the
                                // following `foreach` resolve to `ConstArray`'s
                                // and `checkcast`ed the anonymous `ConstArrayOp`
                                // to a `ConstArray`.
                                if self.receiver_conforms_to(&r, &ret) {
                                    ret = r;
                                }
                            }
                        }
                    } else if method_name == "updated" {
                        if let Some(cls) = recv_ty.as_ref().and_then(|t| self.st.class_sym_of(t)) {
                            let n = self.st.get(cls).name.as_str();
                            if n == "Map" && args.len() >= 2 {
                                ret = Type::Class {
                                    sym: cls,
                                    args: vec![
                                        args[0].ty.widen_constant(),
                                        args[1].ty.widen_constant(),
                                    ],
                                };
                            } else if n == "Vector" && args.len() >= 2 {
                                ret = Type::Class {
                                    sym: cls,
                                    args: vec![args[1].ty.widen_constant()],
                                };
                            }
                        }
                    } else if method_name == ":+" {
                        if let Some(cls) = recv_ty.as_ref().and_then(|t| self.st.class_sym_of(t)) {
                            if self.st.get(cls).name == "Vector" {
                                if let Some(a0) = args.first() {
                                    ret = Type::Class {
                                        sym: cls,
                                        args: vec![a0.ty.widen_constant()],
                                    };
                                }
                            }
                        }
                    } else if method_name == "apply"
                        && !sym.is_none()
                        && explicit_type_args(fun).is_none()
                    {
                        // Factory result reconstruction is inference only;
                        // explicit type arguments already determined the result.
                        let owner_n = self.st.get(self.st.get(sym).owner).name.clone();
                        if owner_n == "Map$" {
                            if let Some(a0) = args.first() {
                                // `Map(kvs: _*)` hands over the pairs marked
                                // `Repeated`; the pair is what names `K` and `V`.
                                let a0ty = match &a0.ty {
                                    Type::Repeated(e) => e.as_ref(),
                                    other => other,
                                };
                                if let Type::Class { args: targs, .. } = a0ty {
                                    if targs.len() == 2 {
                                        // Every pair bounds `K` and `V`, not only
                                        // the first: `Map(1 -> 2, 3 -> 4.5)` is a
                                        // `Map[Int, AnyVal]`. Reading the first
                                        // made it a `Map[Int, Int]`, and `m(3)`
                                        // unboxed a `Double` as an `Int`. The
                                        // bounds sit inside `Tuple2`, so the join
                                        // is the plain lub, never the weak one.
                                        let mut targs = targs.clone();
                                        for a in args.iter().skip(1) {
                                            let aty = match &a.ty {
                                                Type::Repeated(e) => e.as_ref(),
                                                other => other,
                                            };
                                            if let Type::Class { args: more, .. } = aty {
                                                if more.len() == 2 {
                                                    targs = vec![
                                                        self.st.lub(&targs[0], &more[0]),
                                                        self.st.lub(&targs[1], &more[1]),
                                                    ];
                                                }
                                            }
                                        }
                                        let targs = &targs;
                                        if let Some(map) = self.factory_result_class(&ret, "Map", 2)
                                        {
                                            let targs = self
                                                .factory_targs_from_pt(map, targs, pt)
                                                .unwrap_or_else(|| targs.clone());
                                            ret = Type::Class {
                                                sym: map,
                                                args: targs,
                                            };
                                        }
                                    }
                                }
                            }
                        } else if owner_n == "Vector$"
                            || owner_n == "List$"
                            || owner_n == "Set$"
                            || owner_n == "Seq$"
                            || owner_n == "LazyList$"
                        {
                            // `List(Circle(1), Rect(2, 3))` is a `List[Shape]`:
                            // the element type is the lub of every argument.
                            // `Seq(xs: _*)` passes the sequence through, and its
                            // tree carries the *element* type marked `Repeated`;
                            // taking it as written made the call a `Seq[Int*]`.
                            if let Some(elem) = args
                                .iter()
                                .map(|a| match a.ty.widen_constant() {
                                    Type::Repeated(e) => (*e).clone(),
                                    other => other,
                                })
                                .reduce(|acc, t| self.lub_ty(&acc, &t))
                            {
                                if let Some(cls) = self.factory_result_class(
                                    &ret,
                                    owner_n.trim_end_matches('$'),
                                    1,
                                ) {
                                    // `List(circle, rect)` is a `List[Shape]`, so the
                                    // element type is the lub of every argument.
                                    let args1 = self
                                        .factory_targs_from_pt(cls, std::slice::from_ref(&elem), pt)
                                        .unwrap_or_else(|| vec![elem]);
                                    ret = Type::Class {
                                        sym: cls,
                                        args: args1,
                                    };
                                }
                            }
                        } else if owner_n == "Left$" || owner_n == "Right$" {
                            if let Some(inst) =
                                self.instantiate_either_ctor_apply(&owner_n, &ret, args, pt)
                            {
                                ret = inst;
                            }
                        } else if owner_n == "Try$" || owner_n == "Success$" {
                            if let Some(a0) = args.first() {
                                let elem = unwrap_fn0_or_byname(&a0.ty);
                                let elem = match &a0.kind {
                                    TreeKind::Function { body, .. }
                                        if matches!(elem, Type::Any | Type::AnyRef) =>
                                    {
                                        if body.ty.is_no_type() || body.ty.is_error() {
                                            elem
                                        } else {
                                            body.ty.clone()
                                        }
                                    }
                                    _ => elem,
                                };
                                let cname = owner_n.trim_end_matches('$');
                                if let Some(cls) = self.st.lookup(cname).into_iter().find(|id| {
                                    self.st.get(*id).kind == crate::symbol::SymKind::Class
                                }) {
                                    ret = Type::Class {
                                        sym: cls,
                                        args: vec![elem.widen_constant()],
                                    };
                                }
                            }
                        }
                    }
                    // `partition` is `(C, C)` and `groupBy` / `groupMap` are
                    // `Map[K, C]`: the receiver's own collection sits *inside* the
                    // declared result, so the `BuildFrom` rebuild has to reach in.
                    let nested_recv = recv_ty.clone().or_else(|| self.curried_receiver_ty(fun));
                    if let Some(r) = self.receiver_collection_root(nested_recv.as_ref()) {
                        if let Some(t) = self.rebuild_inside(r, &ret, &method_name) {
                            ret = t;
                        }
                    }
                    let ret = leftover.unwrap_or(ret);
                    let arg_tys: Vec<Type> = args.iter().map(Tree::argument_type).collect();
                    let ret = self.subst_dependent_members(&param_tys, &arg_tys, &ret);
                    let params: Vec<SymbolId> =
                        self.st.get(sym).paramss.iter().flatten().copied().collect();
                    let ret = self.subst_dependent_paths(&params, args, ret);
                    let ret = self.instantiate_leftover_tparams(sym, ret, pt, args.len());
                    // nsc's `applyImplicitArgs` ends `if (args contains
                    // EmptyTree) setError(tree)`. The witness that was not
                    // found is often the only thing that could have said what
                    // one of the callee's type parameters is -- slick's
                    // `map[F, G, T](f)(implicit shape: Shape[_, F, T, G]):
                    // Query[G, T, C]` is exactly that -- so handing the
                    // declared result type back leaks `T` into the program and
                    // every selection on it reports again. 19 `value _N is not
                    // a member of T` and 13 `value map is not a member of O2`
                    // in gitbucket were that cascade.
                    //
                    // During signature inference only an unresolved result
                    // type is poisoned. A fully determined result is retained
                    // for later body typing; missing evidence must still be
                    // diagnosed there. Qualified reflection calls materialize
                    // tags in their receiver's universe (check_args), including
                    // c.Expr[Any](...) without a wildcard universe import.
                    let leaks = !sym.is_none()
                        && self
                            .st
                            .get(sym)
                            .tparams
                            .iter()
                            .any(|&tp| crate::check::type_mentions_tparam(&ret, tp));
                    tree.ty = if impl_missing && leaks {
                        Type::Error
                    } else {
                        ret
                    };
                }
                OverloadPick::Ambiguous => {
                    // An argument that already failed cannot pick an alternative;
                    // the cause is reported at the argument.
                    if !arg_tys.iter().any(|t| t.is_error()) {
                        self.error(
                            tree.span,
                            format!(
                                "ambiguous overload for {} with arguments ({})",
                                fun_name,
                                arg_tys
                                    .iter()
                                    .map(|t| self.st.display_type(t))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        );
                    }
                    tree.ty = Type::Error;
                }
                OverloadPick::None => {
                    // `u.Constant("x")` where `def Constant: ConstantExtractor`:
                    // the arguments belong to the *result*'s `apply`, not to the
                    // parameterless def. nsc inserts the `apply`; without it every
                    // extractor in `scala.reflect` (`Literal`, `Constant`,
                    // `TermName`, ...) is unusable, and so is any `def m: T` whose
                    // `T` has an `apply`.
                    if self.insert_apply_on_nullary(fun) {
                        let fun_ty = fun.ty.clone();
                        if let OverloadPick::Found(sym, param_tys, ret) =
                            self.resolve_overload_shaped(&fun_ty, fun.sym, &arg_tys, pt, &shapes)
                        {
                            fun.sym = sym;
                            tree.sym = sym;
                            let own = (!sym.is_none()).then(|| self.st.get(sym).tparams.clone());
                            // The inserted `apply` is a method like any other, and
                            // its *own* type parameters are this call's to solve.
                            // This branch used to hand the declaration's result
                            // back raw, so `P.sequential(fta)` -- a parameterless
                            // `def sequential: F ~> M` auto-applied, whose
                            // `FunctionK.apply[A](fa: F[A]): G[A]` is reached only
                            // here -- was `M[A]` with `A` still the declaration's
                            // own parameter (cats `Parallel.scala`, 24 of its 39
                            // errors read `found: M[A] required: M[T[A]]`).
                            let mut param_tys = param_tys;
                            let mut ret = ret;
                            self.instantiate_inserted_apply(
                                fun,
                                &mut param_tys,
                                &mut ret,
                                &arg_tys,
                                pt,
                                tree.span,
                            );
                            for (i, a) in args.iter_mut().enumerate() {
                                let p = param_at(&param_tys, i).cloned().unwrap_or(Type::NoType);
                                let open = self.open_tparams_of(&p, own.as_deref());
                                if matches!(a.kind, TreeKind::Function { .. }) || a.ty.is_no_type()
                                {
                                    let pt_arg = self.open_to_bounds(&p, &open);
                                    self.type_expr(a, &pt_arg);
                                }
                                if !p.is_no_type() {
                                    let p_check = self
                                        .solve_open_from_arg(&a.argument_type(), &p, &open)
                                        .unwrap_or_else(|| self.open_to_bounds(&p, &open));
                                    self.adapt(a, &p_check);
                                }
                            }
                            // A lambda argument had no type when the first pass
                            // ran; now it has one, so a parameter the first pass
                            // could not reach gets a second chance.
                            if own.as_deref().is_some_and(|t| !t.is_empty())
                                && mentions_tparam(&ret, own.as_deref().unwrap_or(&[]))
                            {
                                let now: Vec<Type> = args.iter().map(Tree::argument_type).collect();
                                self.instantiate_inserted_apply(
                                    fun,
                                    &mut param_tys,
                                    &mut ret,
                                    &now,
                                    pt,
                                    tree.span,
                                );
                            }
                            // The inserted `apply` has default and implicit
                            // clauses like any other method, and this branch
                            // filled neither: the call was emitted with the
                            // implicit argument simply absent, which the JVM
                            // reports as a `VerifyError` rather than the typer
                            // reporting anything at all. cats' `IorT.liftF` is
                            // `right(fb)`, and `RightPartiallyApplied.apply`
                            // is `(fb: F[B])(implicit F: Functor[F])`.
                            let leftover = self
                                .fill_defaults_and_implicits(tree.span, args, &param_tys, fun, pt);
                            // And what the implicit search solved is part of
                            // the result, exactly as on the ordinary path
                            // below: slick's `Query(r)` is `Query.apply[E, U,
                            // R](value: E)(implicit unpack: Shape[_, E, U,
                            // R]): Query[R, U, Seq]`, with `U` and `R` known
                            // only to the `Shape` found, and
                            // `Query(xs.length).first` came out as `U`.
                            if !self.implicit_undet_solved.is_empty() {
                                let sol = std::mem::take(&mut self.implicit_undet_solved);
                                let ids: Vec<SymbolId> = sol.iter().map(|(i, _)| *i).collect();
                                let ts: Vec<Type> = sol.iter().map(|(_, t)| t.clone()).collect();
                                ret = crate::symbol::subst_tparams_slice(&ids, &ts, &ret);
                            }
                            tree.ty = leftover.unwrap_or(ret);
                            return;
                        }
                    }
                    if self.widen_with_companion(fun) {
                        let fun_ty = fun.ty.clone();
                        chosen =
                            self.resolve_overload_shaped(&fun_ty, fun.sym, &arg_tys, pt, &shapes);
                        if matches!(chosen, OverloadPick::Found(..)) {
                            // A newly loaded alternative needs the same inference,
                            // bounds, defaults and implicit clauses as an ordinary
                            // selection; its declared result is not an instantiated
                            // result type.
                            continue 'resolve;
                        }
                    }
                    // The same widening for a receiver that already *is* the
                    // companion (`BigDecimal(3L)` through the `scala` package
                    // object's alias): the alternatives the prelude did not write
                    // by hand are still in the pickle.
                    if self.widen_module_from_pickle(fun) {
                        let fun_ty = fun.ty.clone();
                        chosen =
                            self.resolve_overload_shaped(&fun_ty, fun.sym, &arg_tys, pt, &shapes);
                        if matches!(chosen, OverloadPick::Found(..)) {
                            // A newly loaded alternative needs the same inference,
                            // bounds, defaults and implicit clauses as an ordinary
                            // selection; its declared result is not an instantiated
                            // result type.
                            continue 'resolve;
                        }
                    }
                    // nsc `adaptToArguments` keeps a view of the receiver only
                    // when the viewed member applies to the arguments *as
                    // written*. When none does, the call is put back as it was,
                    // so that the tupling retry below and the diagnostic talk
                    // about the original callee: `Map(1 -> 2, "x")` kept
                    // `BuildFrom.toBuildFrom(Map)`, whose one-parameter `apply`
                    // then accepted the two arguments tupled, and the program
                    // printed a `MapBuilderImpl`.
                    let before_view = fun.clone();
                    if self.rewrite_apply_extension(fun) {
                        recv_ty = match &fun.kind {
                            TreeKind::Select { qual, .. } => Some(qual.ty.clone()),
                            _ => None,
                        };
                        let fun_ty = fun.ty.clone();
                        match self.resolve_overload_shaped(&fun_ty, fun.sym, &arg_tys, pt, &shapes)
                        {
                            pick @ OverloadPick::Found(..) => {
                                // A view changes the receiver, not the application
                                // rules. Preserve type inference and trailing clauses.
                                chosen = pick;
                                continue 'resolve;
                            }
                            OverloadPick::Ambiguous => {
                                self.error(
                                    tree.span,
                                    format!(
                                        "ambiguous overload for {} with arguments ({})",
                                        fun_name,
                                        arg_tys
                                            .iter()
                                            .map(|t| self.st.display_type(t))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    ),
                                );
                                tree.ty = Type::Error;
                                return;
                            }
                            OverloadPick::None => {
                                *fun = before_view;
                            }
                        }
                    }
                    // Before any adaptation of the *arguments*: the alternative
                    // that fits may simply not have been read yet.
                    if self.retry_module_apply_from_pickle(tree, pt) {
                        return;
                    }
                    // Last resort, after every other rewrite: nsc packs an
                    // argument list that fits no alternative into one tuple, so
                    // `Some((a, b), c)` means `Some(((a, b), c))`.
                    if self.retry_tupled_args(tree, pt) {
                        return;
                    }
                    // nsc: `c(1)` looks up `apply`, never `update`. Assignment
                    // `c(i) = v` is the only path that rewrites to `update`.
                    let has_apply = match strip_annotations(&fun_ty) {
                        Type::Method { .. } | Type::Overload(_) | Type::Function { .. } => true,
                        Type::Array(_) => true,
                        Type::Class { sym, .. } | Type::ModuleRef(sym) => {
                            !self.st.lookup_member(*sym, "apply").is_empty()
                        }
                        _ => false,
                    };
                    // `"abcdef"(1)`: the callee has no `apply` of its own, but an
                    // implicit conversion of it does (`augmentString` ->
                    // `StringOps.apply`). nsc types `c(1)` as `c.apply(1)`, so
                    // re-type it in that shape and let the ordinary `Select` path
                    // insert the conversion. `s.apply(1)` already worked; only the
                    // indexing sugar reached this error.
                    if !has_apply && !fun_ty.is_error() && self.retry_apply_extension(tree, pt) {
                        return;
                    }
                    if fun_ty.is_error() {
                        // The receiver already failed; do not report it twice.
                    } else if !has_apply {
                        self.error(
                            tree.span,
                            format!(
                                "value apply is not a member of {}",
                                self.st.display_type(&fun_ty)
                            ),
                        );
                    } else if !arg_tys.iter().any(|t| t.is_error()) {
                        self.error(
                            tree.span,
                            format!(
                                "no matching overload for {} with arguments ({})",
                                self.st.display_type(&fun_ty),
                                arg_tys
                                    .iter()
                                    .map(|t| self.st.display_type(t))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        );
                    }
                    tree.ty = Type::Error;
                }
            }
            break;
        }
    }

    /// `xs.map(f)` is retyped as `Coll[B]` for a one-parameter collection whose
    /// prelude signature does not carry the element type through on its own.
    /// A receiver that takes any other number of parameters cannot be written
    /// that way -- a user's `def map[R2](f: R => R2): Act[R2, NoStream, E]`
    /// would lose two arguments -- so its declared result type stands.
    fn map_result_uses_element(&self, method: SymbolId, result: &Type, args: &[Tree]) -> bool {
        if method.is_none() || self.st.get(method).pickled_origin.is_empty() {
            return true;
        }
        let Type::Class {
            args: result_args, ..
        } = result
        else {
            return true;
        };
        let Some(Type::Function { ret: element, .. }) =
            args.first().and_then(|arg| match &arg.ty {
                Type::Function { .. } => Some(arg.ty.clone()),
                other => self.function_view(other),
            })
        else {
            return true;
        };
        // Collection rebuilding can change the constructor around an element,
        // but cannot reinterpret a different parameter as that element. A
        // fixed-key map's one parameter is its value, not the whole key/value
        // pair; a key-changing map can also select a two-parameter result.
        result_args.len() == 1 && result_args[0].widen_constant() == element.widen_constant()
    }

    fn takes_one_type_parameter(&self, cls: SymbolId) -> bool {
        self.st.get(cls).tparams.len() == 1
    }

    /// A `scala.collection` class whose real `map` returns its own type
    /// constructor. `Range` (no type parameter of its own) and a user class
    /// that extends one of these are not among them.
    ///
    /// Living in `scala.collection` is not enough, and the difference is the
    /// element. Every real collection passes its *own* type parameters down as
    /// the element `IterableOnce` receives -- `Vector[A]` is an
    /// `IterableOnce[A]`, `TreeMap[K, V]` an `IterableOnce[(K, V)]` -- so
    /// putting the receiver's class back around the element the declaration
    /// computed is the same type nsc's `BuildFrom` arrives at.
    /// `Iterator.GroupedIterator[B]` is an `IterableOnce[Seq[B]]`: its `CC` is
    /// `Iterator`, and rebuilding gave `it.sliding(n).map(f)` the type
    /// `GroupedIterator[B]`, which claims an element of `Seq[B]` for a value
    /// whose elements are `B`. A class whose element is not its own parameter
    /// keeps the `CC` its declaration names.
    fn maps_to_own_class(&self, cls: SymbolId) -> bool {
        if !self.st.get(cls).jvm_name.starts_with("scala/collection/") {
            return false;
        }
        let Some(io) = crate::classpath::find_by_jvm(&self.st, "scala/collection/IterableOnce")
        else {
            return true;
        };
        if cls == io || self.st.class_reaches(cls, io) != Some(true) {
            return true;
        }
        let tps = self.st.get(cls).tparams.clone();
        let args: Vec<Type> = tps.iter().map(|t| Type::TypeParam(*t)).collect();
        let bta = self.st.base_type_args(cls, &args);
        let Some([elem]) = bta.get(&io.0).map(|a| &a[..]) else {
            return true;
        };
        let own = |t: &Type| matches!(t, Type::TypeParam(p) if tps.contains(p));
        own(elem) || self.pair_args(elem).is_some_and(|p| p.iter().all(own))
    }

    /// The two components of a pair type, however it is spelled.
    fn pair_args(&self, ty: &Type) -> Option<Vec<Type>> {
        match ty {
            Type::Class { sym, args } if args.len() == 2 && self.st.get(*sym).name == "Tuple2" => {
                Some(args.clone())
            }
            Type::Tuple(args) if args.len() == 2 => Some(args.clone()),
            _ => None,
        }
    }

    /// 2.13's `BuildFrom`, as a type function on the *declared* result.
    ///
    /// Every transformation on `IterableOps` / `MapOps` / `SortedOps` is
    /// declared to return the receiver's own type constructor -- `C`, `CC[B]`,
    /// `CC[K2, V2]`. Neither the prelude nor the pickle can spell those, so
    /// the declaration that reaches the typer names the class it was *read
    /// from*: `Seq[B]` for a member inherited from `SeqOps`, `Iterable[(K, V)]`
    /// for one inherited from `IterableOps` by a `Map`. This puts the
    /// receiver's own class back, keeping the element types the declaration
    /// computed.
    ///
    /// A `Map`-like receiver takes two parameters where `IterableOps` passes
    /// one pair; that is exactly the difference between `IterableOps.map[B]`
    /// and `MapOps.map[K2, V2]` (`javap -p -s scala.collection.MapOps`:
    /// `<K2, V2> CC map(Function1<Tuple2<K, V>, Tuple2<K2, V2>>)`), so the pair
    /// is unwrapped here. A lambda that does *not* return a pair keeps the
    /// `Iterable[B]` the declaration named, which is what nsc infers too.
    /// The result of an element-widening member (`updated`, `:+`, `+:`,
    /// `padTo`, …; `[B >: A]` returning `CC[B]`) joined with the receiver's
    /// element type and the argument that supplies the new element. `None`
    /// leaves the result alone: another member, or a shape this does not know.
    fn widen_to_receiver_elem(
        &self,
        method: &str,
        sym: SymbolId,
        recv: Option<&Type>,
        ret: &Type,
        args: &[Tree],
    ) -> Option<Type> {
        let (elem_arg, arity) = match method {
            "updated" | "padTo" => (1, 2),
            ":+" | "$colon$plus" | "appended" | "+:" | "$plus$colon" | "prepended" => (0, 1),
            _ => return None,
        };
        // Only the library's own shape: a generic member (`[B >: A]`) or a
        // prelude stand-in for one, at its own arity. A collection-package
        // class's unrelated `updated` (`HashSet`'s internal
        // `SetNode.updated(element, originalHash, elementHash, shift)`) is left
        // alone.
        if sym.is_none() || args.len() != arity {
            return None;
        }
        let s = self.st.get(sym);
        let stand_in = sym.0 < self.st.prelude_end && s.pickled_origin.is_empty();
        if !stand_in && s.tparams.is_empty() {
            return None;
        }
        let arg_ty = args.get(elem_arg)?.ty.widen_constant();
        if arg_ty.is_no_type() || arg_ty.is_error() {
            return None;
        }
        let Type::Class { sym, args: rargs } = ret else {
            return None;
        };
        let recv_root = self.receiver_collection_root(recv)?;
        let base = self.base_type_instance(recv?, recv_root, 0)?;
        let Type::Class { args: bargs, .. } = base else {
            return None;
        };
        // A receiver whose own element is still undetermined
        // (`Vector.empty :+ a`) contributes its lower bound, `Nothing`, not
        // a variable for the lub to climb past.
        let bargs: Vec<Type> = bargs.iter().map(|t| self.minimize_undet(t)).collect();
        let open_here = |t: &Type| {
            let mut open = Vec::new();
            collect_tparams(t, &mut open);
            open.iter().any(|tp| !self.tparam_in_scope(*tp))
        };
        if bargs.iter().any(open_here) || open_here(&arg_ty) {
            return None;
        }
        // The declared element still names the member's own `B` when the
        // call has not been instantiated yet; `B` *is* the join, so it adds
        // nothing to it.
        let join = |declared: &Type, recv_elem: &Type| {
            let base = self.st.lub(recv_elem, &arg_ty);
            if open_here(declared) {
                base
            } else {
                self.st.lub(declared, &base)
            }
        };
        match (rargs.len(), bargs.len()) {
            (1, 1) => Some(Type::Class {
                sym: *sym,
                args: vec![join(&rargs[0], &bargs[0])],
            }),
            // `MapOps.updated[V1 >: V](key: K, value: V1): Map[K, V1]`: the
            // key is the receiver's own.
            (2, 2) if method == "updated" => Some(Type::Class {
                sym: *sym,
                args: vec![bargs[0].clone(), join(&rargs[1], &bargs[1])],
            }),
            _ => None,
        }
    }

    fn rebuild_from_receiver(&self, recv_root: SymbolId, declared: &Type) -> Option<Type> {
        let Type::Class {
            sym: d,
            args: dargs,
        } = declared
        else {
            return None;
        };
        let d = *d;
        if dargs.is_empty() || d == recv_root || !self.maps_to_own_class(recv_root) {
            return None;
        }
        // Only a real subclass rebuilds: a user class that merely extends `Seq`
        // inherits `Seq`'s `CC` and really does map to a `Seq`.
        self.base_type_instance(
            &Type::Class {
                sym: recv_root,
                args: vec![],
            },
            d,
            0,
        )?;
        let want = self.st.get(recv_root).tparams.len();
        if want == dargs.len() {
            return Some(Type::Class {
                sym: recv_root,
                args: dargs.clone(),
            });
        }
        if want == 2 && dargs.len() == 1 {
            if let Some(pair) = self.pair_args(&dargs[0]) {
                return Some(Type::Class {
                    sym: recv_root,
                    args: pair,
                });
            }
        }
        None
    }

    /// `map` / `flatMap` / `collect` on a *sorted* map are
    /// `SortedMapOps.map[K2, V2](f)(implicit ord: Ordering[K2]): CC[K2, V2]`
    /// (`javap -p -s scala.collection.SortedMapOps`:
    /// `(Lscala/Function1;Lscala/math/Ordering;)Lscala/collection/Map;`).
    /// Without that witness the call lands on `MapOps.map`, which builds a
    /// plain `Map` — narrowing the static type to `TreeMap` there is a
    /// `ClassCastException` waiting at the assignment. The `C`-returning
    /// members (`filter`, `take`, `-`, `+`, `updated`) need no witness and are
    /// narrowed as usual.
    fn needs_ordering_to_rebuild(&self, cls: SymbolId) -> bool {
        [
            "scala/collection/SortedMap",
            "scala/collection/immutable/SortedMap",
            "scala/collection/SortedSet",
            "scala/collection/immutable/SortedSet",
        ]
        .iter()
        .filter_map(|jvm| crate::classpath::find_by_jvm(&self.st, jvm))
        .any(|sorted| {
            cls == sorted
                || self
                    .base_type_instance(
                        &Type::Class {
                            sym: cls,
                            args: vec![],
                        },
                        sorted,
                        0,
                    )
                    .is_some()
        })
    }

    /// [`Self::rebuild_from_receiver`] for the members that *widen* the element
    /// type (`CC[B]` / `CC[K2, V2]`), which a sorted collection cannot do
    /// without an `Ordering`.
    fn rebuild_widened(&self, recv_root: SymbolId, declared: &Type) -> Option<Type> {
        if self.needs_ordering_to_rebuild(recv_root) {
            return None;
        }
        self.rebuild_from_receiver(recv_root, declared)
    }

    /// [`Self::rebuild_from_receiver`] for a member selected *without* an
    /// argument list.
    ///
    /// `tail`, `init`, `reverse`, `distinct` are declared `C` and
    /// `zipWithIndex` / `flatten` are declared `CC[B]`, exactly like the
    /// members the application path already rebuilds -- but a parameterless
    /// selection never reaches that path, and what the declaration says
    /// depends on which class the pickle was asked about. `PickleSupply`
    /// answers a completion by walking the *linearization* and substituting,
    /// then installs the result on the class that was asked: once some
    /// `aSeq.tail` has put `IterableOps.tail: C` on `immutable.Seq` as
    /// `Seq[A]`, `aVector.tail` finds that by inheritance and is a `Seq[A]`.
    /// Whether `xs.tail` on a `Vector` typed as a `Vector` therefore depended
    /// on whether a `Seq` receiver appeared earlier in the run -- cats'
    /// `NonEmptySeq`, `NonEmptyLazyList`, `stream.scala` and `arraySeq.scala`
    /// all read `found: Iterable[A] required: LazyList[A]` and friends for
    /// this reason, and a one-line file with the two selections in the other
    /// order compiled.
    ///
    /// Same gates as the application path: only a `scala/collection/` class,
    /// only a real subclass of what the declaration named, and `SeqView` keeps
    /// the `View` results it really declares.
    pub(crate) fn rebuild_parameterless_collection(
        &self,
        sym: SymbolId,
        name: &str,
        recv_ty: &Type,
        ty: &mut Type,
    ) {
        // `zipWithIndex` and `flatten` widen the element type (`CC[B]`), which
        // a sorted collection cannot rebuild without an `Ordering`.
        let widens = matches!(name, "zipWithIndex" | "flatten");
        if !widens && !crate::check::returns_receiver_collection(name) {
            return;
        }
        // `flatten` is `(implicit asIterable: A => IterableOnce[B]): CC[B]`, so
        // what the selection carries is a method type whose only clause is
        // implicit. Rebuilding its *result* is the same answer the value gets
        // once the witness is filled in, and no other clause shape is touched:
        // a member that really takes arguments is the application path's.
        let inner = match ty {
            Type::Class { .. } => ty,
            Type::Method { ret, .. }
                if !sym.is_none()
                    && self.only_implicit_clauses(sym)
                    && matches!(**ret, Type::Class { .. }) =>
            {
                ret.as_mut()
            }
            _ => return,
        };
        let Some(r) = self.receiver_collection_root(Some(recv_ty)) else {
            return;
        };
        // A *view* is the one collection whose `C` is not itself:
        // `trait SeqView[+A] extends SeqOps[A, View, View[A]] with View[A]`,
        // and `javap scala.collection.SeqView` lists the fifteen members it
        // really does override to return a `SeqView` -- `tail`, `init` and
        // `distinct` are not among them. Narrowing `ls.view.tail` to a
        // `SeqView` compiled and then threw `ClassCastException` on the
        // `scala.collection.View$Drop` the call really returns
        // (`test/files/run/t4332b.scala`). The application path's
        // `declares_view_result` list is not enough here: it holds only the
        // members that take an argument, which is all it ever sees.
        if self.is_view_class(r) {
            return;
        }
        let rebuilt = if widens {
            self.rebuild_widened(r, inner)
        } else {
            self.rebuild_from_receiver(r, inner)
        };
        if let Some(t) = rebuilt {
            *inner = t;
        }
    }

    /// Whether `cls` is `scala.collection.View` or one of its subclasses.
    ///
    /// Every view's `C` and `CC` are `View[A]` / `View`, whatever the view's
    /// own class is, so the `BuildFrom` rebuild must not touch one.
    fn is_view_class(&self, cls: SymbolId) -> bool {
        let Some(view) = crate::classpath::find_by_jvm(&self.st, "scala/collection/View") else {
            return self.st.get(cls).jvm_name.ends_with("View");
        };
        cls == view
            || self
                .base_type_instance(
                    &Type::Class {
                        sym: cls,
                        args: vec![],
                    },
                    view,
                    0,
                )
                .is_some()
    }

    /// The receiver's own collection class, for the `BuildFrom` rebuild.
    fn receiver_collection_root(&self, recv_ty: Option<&Type>) -> Option<SymbolId> {
        recv_ty
            .and_then(|t| self.st.class_sym_of(t))
            .map(|c| self.collection_root(c))
    }

    /// `xs.partition(p)` is `(C, C)` and `xs.groupBy(f)` is `Map[K, C]`: the
    /// receiver's collection is *inside* the result, not the result itself.
    fn rebuild_inside(&self, recv_root: SymbolId, ret: &Type, method_name: &str) -> Option<Type> {
        // A pair result reaches here either as `Tuple2[C, C]` or as the
        // structural `(C, C)`, depending on whether the signature came from
        // the prelude or from the jar.
        let (n, args): (String, &Vec<Type>) = match ret {
            Type::Class { sym, args } => (self.st.get(*sym).name.clone(), args),
            Type::Tuple(args) => ("Tuple2".to_string(), args),
            _ => return None,
        };
        let positions: Vec<usize> = match method_name {
            "partition" | "span" | "splitAt" if n == "Tuple2" && args.len() == 2 => vec![0, 1],
            "groupBy" | "groupMap" if n == "Map" && args.len() == 2 => vec![1],
            _ => return None,
        };
        let mut out = args.clone();
        let mut hit = false;
        for i in positions {
            if let Some(t) = self.rebuild_from_receiver(recv_root, &args[i]) {
                out[i] = t;
                hit = true;
            }
        }
        if !hit {
            return None;
        }
        Some(match ret {
            Type::Class { sym, .. } => Type::Class {
                sym: *sym,
                args: out,
            },
            _ => Type::Tuple(out),
        })
    }

    /// The collection a *curried* call was made on. `xs.groupMap(k)(f)` types
    /// its second clause with the first `Apply` as the callee, so the plain
    /// `Select` receiver is out of reach; walk down to it.
    fn curried_receiver_ty(&self, fun: &Tree) -> Option<Type> {
        let mut t = fun;
        loop {
            match &t.kind {
                TreeKind::Select { qual, .. } => return Some(qual.ty.clone()),
                TreeKind::Apply { fun, .. } | TreeKind::TypeApply { fun, .. } => t = fun,
                _ => return None,
            }
        }
    }

    fn collection_root(&self, id: SymbolId) -> SymbolId {
        let n = self.st.get(id).name.as_str();
        if n == "Some" || n == "None$" || n == "None" {
            self.st.option_sym
        } else if n == "$colon$colon" || n == "Nil$" || n == "Nil" || n == "::" {
            self.st.list_sym
        } else if n == "Left" || n == "Right" {
            self.scala_class_named("Either").unwrap_or(id)
        } else if n == "Success" || n == "Failure" || n == "Try$WithFilter" {
            self.scala_class_named("Try").unwrap_or(id)
        } else {
            id
        }
    }

    /// `e.map(f)` on an `Either[A, B]` keeps the left type: `Either[A, C]`.
    /// `e.left.map(f)` on a `LeftProjection[A, B]` keeps the right type.
    /// Returns `None` for every other receiver so the generic single-parameter
    /// collection rule still applies.
    fn either_map_result(&self, recv_ty: Option<&Type>, args: &[Tree]) -> Option<Type> {
        let Type::Function { ret: fr, .. } = &args.first()?.ty else {
            return None;
        };
        let to = fr.as_ref().widen_constant();
        let (sym, targs) = match recv_ty? {
            Type::Class { sym, args } if args.len() == 2 => (*sym, args),
            _ => return None,
        };
        let name = self.st.get(sym).name.as_str();
        let either = self.scala_class_named("Either")?;
        if is_right_biased_either(&self.st, sym) {
            Some(Type::Class {
                sym: either,
                args: vec![targs[0].clone(), to],
            })
        } else if name == "LeftProjection" {
            Some(Type::Class {
                sym: either,
                args: vec![to, targs[1].clone()],
            })
        } else {
            None
        }
    }

    /// A class symbol from the `scala` package by name (`Either`, `Try`, …).
    fn scala_class_named(&self, name: &str) -> Option<SymbolId> {
        self.st
            .get(self.st.scala_pkg)
            .members
            .iter()
            .copied()
            .find(|id| {
                self.st.get(*id).name == name
                    && self.st.get(*id).kind == crate::symbol::SymKind::Class
            })
    }

    /// Whether `recv`'s class is `decl`'s class or a subclass of it — the
    /// shape in which replacing a declared result type by the receiver's is a
    /// *narrowing* rather than a jump to an unrelated class. An unknown class
    /// on either side answers `true`, which keeps the existing behaviour for
    /// everything the prelude supplies without a class symbol.
    fn receiver_conforms_to(&self, recv: &Type, decl: &Type) -> bool {
        let (Some(rc), Some(dc)) = (self.st.class_sym_of(recv), self.st.class_sym_of(decl)) else {
            return true;
        };
        rc == dc
            || self
                .st
                .base_type_seq(&Type::Class {
                    sym: rc,
                    args: vec![],
                })
                .iter()
                .any(|b| self.st.class_sym_of(b) == Some(dc))
    }

    fn is_with_filter_ty(&self, ty: Option<&Type>) -> bool {
        let Some(ty) = ty else {
            return false;
        };
        let Some(id) = self.st.class_sym_of(ty) else {
            return false;
        };
        let n = self.st.get(id).name.as_str();
        // `StringOps$WithFilter`: without it the rule below replaced
        // `"abc".withFilter(p)`'s result with the *receiver* (`StringOps`,
        // which erases to `String`), and the following `.map` compiled to a
        // `checkcast java/lang/String` on a real `StringOps$WithFilter`.
        n == "WithFilter"
            || n == "Option$WithFilter"
            || n == "Try$WithFilter"
            || n == "StringOps$WithFilter"
            || n == "MapOps$WithFilter"
    }

    // MapOps.WithFilter has two result constructors: CC[K, V] for pair
    // transformations and IterableCC[B] for the inherited generic overload.
    // flatMap takes B from IterableOnce[B], never from the lambda's concrete
    // collection constructor (for example List[B]).
    fn map_with_filter_non_pair_result(
        &self,
        recv: Option<&Type>,
        args: &[Tree],
        flatten: bool,
    ) -> Option<Type> {
        if !self.library_abi {
            return None;
        }
        let Type::Class { sym, args: wf_args } = recv? else {
            return None;
        };
        if self.st.get(*sym).jvm_name != "scala/collection/MapOps$WithFilter" {
            return None;
        }
        let fun = match &args.first()?.ty {
            t @ Type::Function { .. } => t.clone(),
            t => self.function_view(t)?,
        };
        let Type::Function { ret, .. } = fun else {
            return None;
        };
        let elem = if flatten { self.elem_type(&ret)? } else { *ret };
        if self.pair_args(&elem).is_some() {
            return None;
        }
        let Type::Class { sym: ctor, .. } = wf_args.get(2)? else {
            return None;
        };
        Some(Type::Class {
            sym: *ctor,
            args: vec![elem.widen_constant()],
        })
    }

    fn is_array_ops_ty(&self, ty: Option<&Type>) -> bool {
        ty.and_then(|t| self.st.class_sym_of(t))
            .is_some_and(|id| self.st.get(id).name == "ArrayOps")
    }

    fn is_try_block_ty(&self, ty: Option<&Type>) -> bool {
        ty.and_then(|t| self.st.class_sym_of(t)).is_some_and(|id| {
            self.st.get(id).name == "TryBlock"
                && self.st.jvm_internal(id) == "scala/util/control/Breaks$TryBlock"
        })
    }

    pub(crate) fn elem_type(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Class { sym, args }
                if args.len() == 2 && is_tuple2_elem_map(&self.st.get(*sym).name) =>
            {
                Some(Type::Class {
                    sym: self.tuple2_sym(),
                    args: args.clone(),
                })
            }
            // 2.13's `Either` is right-biased: `map` / `flatMap` / `foreach`
            // see the `B` of `Either[A, B]`, not the `A`.
            Type::Class { sym, args }
                if args.len() == 2 && is_right_biased_either(&self.st, *sym) =>
            {
                Some(args[1].clone())
            }
            Type::Class { sym, .. } if self.st.get(*sym).name == "Range" => Some(Type::Int),
            Type::Class { sym, .. } if self.st.get(*sym).name == "BitSet" => Some(Type::Int),
            Type::Class { sym, args } if !args.is_empty() => {
                // The fallback is only for collection classes whose
                // `IterableOnce` parent has not been loaded yet. Applying it
                // to every generic receiver made `Ior[A, B].map` read the
                // left parameter as the mapped value and rejected a perfectly
                // valid `B => C` function.
                //
                // Classes known to iterate their first type parameter skip
                // the parent walk: walking a deeply nested `List` through
                // every `IterableOnce` parent is needlessly expensive during
                // repeated `asInstanceOf` chains. The rest of the collection
                // package must take the walk first -- `Iterator.sliding`
                // returns `GroupedIterator[B]`, which iterates `Seq[B]`, and
                // reading its argument as the element rejected
                // `it.sliding(2).map(f)` for a valid `f: Seq[A] => B`.
                let name = self.st.get(*sym).name.as_str();
                let jvm = self.st.get(*sym).jvm_name.as_str();
                if is_first_arg_elem_collection(jvm)
                    || matches!(
                        name,
                        "Traversable" | "Iterable" | "Seq" | "IndexedSeq" | "LinearSeq"
                    )
                {
                    return Some(args[0].clone());
                }
                self.iterable_once_elem(*sym, args).or_else(|| {
                    (jvm.starts_with("scala/collection/") || jvm.starts_with("scala/ArrayOps"))
                        .then(|| args[0].clone())
                })
            }
            // cats' syntax layer hands back `Ops[F, A] { type TypeClassType =
            // FlatMap[F] }`; the arguments live on the parent.
            Type::Refined { parents, .. } => parents.iter().find_map(|p| self.elem_type(p)),
            Type::ModuleRef(id) => {
                let name = self.st.get(*id).name.as_str();
                if name == "Nil$" || name == "None$" {
                    Some(Type::Nothing)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// The argument a collection receiver passes to `IterableOnce`, which is
    /// its element type whatever its own first type argument happens to be.
    ///
    /// The first type argument is the element for every collection this guess
    /// was written for, and it is not the element in general.
    /// `Iterator[A].sliding(n)` hands back
    /// `Iterator.GroupedIterator[B] extends AbstractIterator[Seq[B]]`, whose
    /// element is `Seq[B]`; guessing `B` typed the lambda of
    /// `it.sliding(n).map(f)` against `A` and reported `found: (Seq[A]) => B
    /// required: (A) => Any` for a function that is exactly right. nsc reads
    /// the parameter off `IterableOnceOps.map`'s declaration seen from the
    /// receiver, and this is that answer for the one position the guess needs.
    ///
    /// `None` when the receiver is not an `IterableOnce` at all -- `Option`,
    /// `Future`, `Try` and cats' `Ops[F, A]` all reach this and keep the
    /// caller's existing fallback.
    fn iterable_once_elem(&self, sym: SymbolId, args: &[Type]) -> Option<Type> {
        let io = crate::classpath::find_by_jvm(&self.st, "scala/collection/IterableOnce")?;
        // Cheap enough to run before the linearizing walk, and it is `false`
        // for `Option` / `Future` / cats' `Ops`, which is nearly every caller.
        if sym == io || self.st.class_reaches(sym, io) != Some(true) {
            return None;
        }
        let bta = self.st.base_type_args(sym, args);
        let at = bta.get(&io.0)?;
        match &at[..] {
            [e] => Some(e.clone()),
            _ => None,
        }
    }

    fn tuple2_sym(&self) -> SymbolId {
        self.st
            .lookup("Tuple2")
            .into_iter()
            .find(|id| self.st.get(*id).kind == crate::symbol::SymKind::Class)
            .unwrap_or(SymbolId::NONE)
    }

    /// Solve the type parameters of an `apply` that was inserted on a
    /// parameterless result (`insert_apply_on_nullary`), and rewrite the
    /// parameter types and the result to the solution.
    ///
    /// The main application path does this at the point it picks an
    /// alternative; the inserted-`apply` retry never did, so a generic `apply`
    /// reached only that way handed its declaration's own type parameters back
    /// as the call's type. That is the whole of `cats.Parallel`'s
    /// `P.sequential(fta)` / `P.parallel(f(a))`: `F ~> M` is a *value*, so the
    /// `FunctionK.apply[A](fa: F[A]): G[A]` behind it is auto-applied here.
    ///
    /// Only what the arguments (and then the expected type) actually pin is
    /// substituted -- a parameter nothing decides is left standing, exactly as
    /// the main path leaves it, so nothing that used to typecheck by staying
    /// abstract stops doing so.
    pub(crate) fn instantiate_inserted_apply(
        &mut self,
        fun: &mut Tree,
        param_tys: &mut Vec<Type>,
        ret: &mut Type,
        arg_tys: &[Type],
        pt: &Type,
        span: scala_rs_span::Span,
    ) {
        let sym = fun.sym;
        let recv = match &fun.kind {
            TreeKind::Select { qual, .. } => Some(qual.ty.clone()),
            _ => None,
        };
        // Bounds belong to the inserted apply's receiver, not the original
        // parameterless method's receiver. Infer lower bounds at that type
        // and validate both bounds before substituting the solution: adapting
        // to already substituted parameters cannot catch an invalid solution.
        let inst = self.infer_method_tparams_in(sym, param_tys, arg_tys, recv.as_ref());
        // A function literal is still a placeholder here; taking it for a
        // solution would hide the expected type from the lambda.
        let inst: Vec<(SymbolId, Type)> = inst
            .into_iter()
            .filter(|(_, t)| !mentions_no_type(t) && !t.is_error() && !t.is_no_type())
            .collect();
        let mut inst = self.add_expected_constraints(sym, ret, pt, inst);
        // The factory's variables belong to the receiver rather than apply.
        // Infer them from value arguments, then use a compatible expected
        // result before searching the implicit clause.
        for tp in self.undet_tvars.clone() {
            if inst.iter().any(|(id, _)| *id == tp) {
                continue;
            }
            let owner = self.st.get(tp).owner;
            let from_args = self
                .infer_method_tparams(owner, param_tys, arg_tys)
                .into_iter()
                .find_map(|(id, t)| (id == tp).then_some(t))
                .filter(|t| !mentions_no_type(t) && !t.is_error() && !type_mentions_tparam(t, tp));
            let expected = unify_one(&self.st, tp, ret, pt)
                .filter(|t| !mentions_no_type(t) && !t.is_error() && !type_mentions_tparam(t, tp));
            let solution = match (from_args, expected) {
                (Some(arg), Some(expected)) if self.st.is_sub_type(&arg, &expected) => {
                    Some(expected)
                }
                (Some(arg), _) => Some(arg),
                (None, expected) => expected,
            };
            if let Some(t) = solution {
                if self.undet_solution_in_bounds(tp, &t) {
                    inst.push((tp, t));
                }
            }
        }
        self.check_tparam_bounds(sym, &inst, recv.as_ref(), span, true);
        if inst.is_empty() {
            return;
        }
        let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
        let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
        *param_tys = param_tys
            .iter()
            .map(|p| crate::symbol::subst_tparams_slice(&tps, &args_t, p))
            .collect();
        *ret = crate::symbol::subst_tparams_slice(&tps, &args_t, ret);
        fun.ty = crate::symbol::subst_tparams_slice(&tps, &args_t, &fun.ty);
        if let TreeKind::Select { qual, .. } = &mut fun.kind {
            qual.ty = crate::symbol::subst_tparams_slice(&tps, &args_t, &qual.ty);
        }
    }

    /// Least upper bound, with the numeric widenings nsc applies before lubbing
    /// (`lub(Int, Long) = Long`).
    pub(crate) fn lub_ty(&self, a: &Type, b: &Type) -> Type {
        if let Some(t) = numeric_widen(a, b).or_else(|| numeric_widen(b, a)) {
            return t;
        }
        self.st.lub(a, b)
    }

    /// Can a constructor parameter's *declared* type be handed to its
    /// argument as the expected type without reading it through a prefix?
    /// Only a type built from classes, arrays, tuples and functions whose
    /// type parameters are the constructed class's own (`tps`, which the
    /// caller substitutes or refuses). A type member, a singleton or an
    /// enclosing class's parameter means something different at every
    /// `new o.C(…)`, and nothing on this path does the as-seen-from.
    pub(crate) fn is_closed_ctor_proto(&self, ty: &Type, tps: &[SymbolId]) -> bool {
        match ty {
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
            | Type::JavaObject
            | Type::AnyVal
            | Type::Null
            | Type::Nothing => true,
            Type::TypeParam(id) => tps.contains(id),
            Type::Array(t) => self.is_closed_ctor_proto(t, tps),
            Type::Class { args, .. } | Type::Tuple(args) => {
                args.iter().all(|a| self.is_closed_ctor_proto(a, tps))
            }
            Type::Function { params, ret } => {
                params.iter().all(|p| self.is_closed_ctor_proto(p, tps))
                    && self.is_closed_ctor_proto(ret, tps)
            }
            _ => false,
        }
    }
}
