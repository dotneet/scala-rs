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
        self.flatten_curried_new(tree);
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
            // `new C(b = 2, a = 1)`: named arguments must be put in parameter
            // order before the constructor overload is picked, since the pick
            // is driven by the argument types.
            if Self::has_named_arg(args) {
                let placed = self.reorder_named_ctor_args(args, class_id, fun);
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
            let explicit: Vec<Type> = match &fun.ty {
                Type::Class { args, .. } if type_args_are_instantiated(args, &tps) => args.clone(),
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
            // read its parameter types from. Only for a monomorphic class,
            // only where the arity settles which constructor this is, and only
            // for a fully determined function-shaped parameter.
            let ctor_protos: Vec<Type> = class_id
                .filter(|_| tps.is_empty())
                .map(|c| self.st.get(c).ctor_fields.clone())
                .filter(|fs| fs.len() == args.len())
                .map(|fs| {
                    fs.iter()
                        .map(|f| {
                            let t = self.st.get(*f).ty.clone();
                            if !t.is_no_type()
                                && !t.is_error()
                                && !type_mentions_wildcard(&t)
                                && !mentions_any_tparam(&t)
                                && self.is_function_shaped(&t)
                            {
                                t
                            } else {
                                Type::NoType
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            for (ai, a) in args.iter_mut().enumerate() {
                if let TreeKind::Function { vparams, .. } = &a.kind {
                    if is_annotated_lambda(a) {
                        self.type_expr(a, &Type::NoType);
                        arg_tys.push(a.ty.clone());
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
                    arg_tys.push(a.ty.clone());
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
                        .map(|f| self.st.get(*f).ty.clone())
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
                let mut picked = self.pick_ctor_at(c, &explicit, &arg_tys, None);
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
                    picked = self.pick_ctor_at(c, &explicit, &arg_tys, None);
                }
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
            // Generic inference must use ctor *fields* (`Tuple2._1: A`) even when
            // the picked `<init>` is erased to `(Any, Any)` in the prelude.
            let nargs = args.len();
            let unify_params = if infer && field_tys.len() == nargs && !field_tys.is_empty() {
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
            for (i, a) in args.iter_mut().enumerate() {
                // A repeated parameter covers every argument from its position
                // on, and the argument's type is the *element* type. Indexing
                // the clause by position (as this did) handed argument 0 the
                // raw `T*` -- `new SetTupleParameter[(T1, T2)](c1, c2)` on a
                // `(val children: SetParameter[_]*)` constructor could never
                // typecheck. The method path has always used `param_at`.
                let mut p = if infer && field_tys.len() == nargs && !field_tys.is_empty() {
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
                if a.ty.is_no_type() {
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
        for (ai, a) in args.iter_mut().enumerate() {
            if let TreeKind::Function { vparams, .. } = &a.kind {
                if is_annotated_lambda(a) {
                    self.type_expr(a, &Type::NoType);
                    arg_tys.push(a.ty.clone());
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
                        arg_tys.push(a.ty.clone());
                        continue;
                    }
                }
                if let Some(ps) = self.agreed_lambda_params(&fun_ty_for_pretype, ai, vparams.len())
                {
                    // `Wildcard`, not `NoType`: the parameters are what this
                    // pre-typing fixes, the result is whatever the body says
                    // and must not be checked against anything yet.
                    let pt_arg = Type::Function {
                        params: ps,
                        ret: Box::new(Type::Wildcard),
                    };
                    self.type_expr(a, &pt_arg);
                    arg_tys.push(a.ty.clone());
                    continue;
                }
                arg_tys.push(Type::Function {
                    params: vec![Type::NoType; vparams.len()],
                    ret: Box::new(Type::NoType),
                });
            } else {
                let pt_arg = self.proto_arg_type(&fun_ty_for_pretype, fun.sym, ai, pt);
                if pt_arg.is_no_type() {
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
                    self.type_expr(a, &pt_arg);
                    let with_errs = self.error_count_since(mark);
                    if with_errs > 0
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
                // `take(Array.empty)`: with no expected type the argument keeps
                // its residual implicit clause, `(ClassTag[T])Array[T]`. What
                // the callee sees is the *result*; the clause is filled once
                // the parameter has told it what `T` is.
                self.solve_lower_bounded_undet(a);
                arg_tys.push(self.implicit_only_result(a).unwrap_or_else(|| a.ty.clone()));
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
        let fun_ty = fun.ty.clone();
        self.ensure_apply_supplied(&fun_ty);
        // Before the alternatives are weighed, not only after they all fail:
        // a call that *resolves* still has to solve its type parameters from
        // the arguments' base types, and an argument whose class was never
        // completed has none.
        self.complete_arg_classes(&arg_tys);
        let mut chosen = self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt);
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
                chosen = self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt);
            }
        }
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
                    if let Some(recv @ Type::Class { args, .. }) = recv_ty.as_ref() {
                        // At the *owner's* arguments, not the receiver's own:
                        // an inherited member is declared in the parameters of
                        // the class that declares it, and those line up with
                        // the receiver's only when the receiver is that class.
                        // slick's `BaseJoinQuery[E1, E2, U1, U2, C, B1, B2] <:
                        // Query[+E, U, C[_]]` gave `Query.map`'s result
                        // `Query[G, T, C]` the receiver's third argument --
                        // `U1` -- and `zipWith` was `found: Query[G, T, U]
                        // required: Query[G, T, C]`.
                        let owner = self.st.get(sym).owner;
                        let at_owner = match self.base_type_instance(recv, owner, 0) {
                            Some(Type::Class { args, .. }) => args,
                            _ => args.clone(),
                        };
                        if !at_owner.is_empty() {
                            param_tys = param_tys
                                .iter()
                                .map(|p| self.st.subst_tparams(owner, &at_owner, p))
                                .collect();
                            ret = self.st.subst_tparams(owner, &at_owner, &ret);
                        }
                    }
                    sig_param_tys = param_tys.clone();
                    self.apply_open_views(sym, &param_tys, args, &mut arg_tys);
                    if !self.st.get(sym).tparams.is_empty() {
                        let inst = self.infer_method_tparams_in(
                            sym,
                            &param_tys,
                            &arg_tys,
                            recv_ty.as_ref(),
                        );
                        // nsc leaves `tryBreakable { throw … }`'s T undetermined
                        // (Nothing is bottom). `catchBreak { println }` then
                        // instantiates T from the handler, not from Nothing.
                        let inst: Vec<(SymbolId, Type)> = if self.st.get(sym).name == "tryBreakable"
                        {
                            inst.into_iter()
                                .filter(|(_, t)| !matches!(t, Type::Nothing))
                                .collect()
                        } else {
                            inst
                        };
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
                            Some(targs) if targs.len() == self.st.get(sym).tparams.len() => self
                                .st
                                .get(sym)
                                .tparams
                                .clone()
                                .into_iter()
                                .zip(targs.iter().cloned())
                                .collect(),
                            _ => inst,
                        };
                        // The expected type is a constraint too. Solve it here,
                        // before the implicit clauses are filled: slick's
                        // `def column[T](n: Node)(implicit tt: TypedType[T]): Rep[T]`
                        // gets `T` from nowhere else.
                        let inst = self.add_expected_constraints(sym, &ret, pt, inst);
                        self.check_tparam_bounds(sym, &inst, recv_ty.as_ref(), tree.span, true);
                        if !inst.is_empty() {
                            let tps: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                            let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
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
                            let weak =
                                self.add_expected_constraints_in(sym, &ret, pt, Vec::new(), true);
                            let (ids, vals): (Vec<SymbolId>, Vec<Type>) =
                                weak.into_iter().filter(|(id, _)| open.contains(id)).unzip();
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
                            let undetermined =
                                !sym.is_none() && mentions_tparam(fr, &self.st.get(sym).tparams);
                            let fret = if matches!(fr.as_ref(), Type::TypeParam(_)) || undetermined
                            {
                                Box::new(Type::Any)
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
                            let settled = fp.len() == 1 && {
                                let mut open = Vec::new();
                                collect_tparams(&fp[0], &mut open);
                                open.is_empty()
                                    && !fp[0].is_no_type()
                                    && !matches!(
                                        fp[0],
                                        Type::Named { .. }
                                            | Type::Any
                                            | Type::AnyRef
                                            | Type::TypeMember(_)
                                    )
                            };
                            let fparams =
                                if fp.len() == 1 && !settled && self.st.kind_arity(&elem) == 0 {
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
                    // clause. Type the lambda against `R => Any` so the body
                    // is not checked against a raw type parameter.
                    if let Type::Function { params, ret } = &p {
                        if !params.is_empty() && matches!(ret.as_ref(), Type::TypeParam(_)) {
                            p = Type::Function {
                                params: params.clone(),
                                ret: Box::new(Type::Any),
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
                        let relaxed = match &p {
                            Type::Function { params, ret }
                                if !params.is_empty() && mentions_tparam(ret, &open) =>
                            {
                                let wilds = vec![Type::Wildcard; open.len()];
                                Type::Function {
                                    params: params.clone(),
                                    ret: Box::new(crate::symbol::subst_tparams_slice(
                                        &open, &wilds, ret,
                                    )),
                                }
                            }
                            _ => p.clone(),
                        };
                        let pt_arg = self.open_to_bounds(&relaxed, &open);
                        self.type_expr(a, &pt_arg);
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
                                .find_map(|from| unify_one(tp, &p, from))
                                .filter(|t| {
                                    !t.is_no_type() && !t.is_error() && !type_mentions_tparam(t, tp)
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
                                let t = match unify_one(tp, &ret, pt) {
                                    Some(e)
                                        if e != t
                                            && !e.is_no_type()
                                            && !e.is_error()
                                            && !type_mentions_tparam(&e, tp)
                                            && self.tparam_variance_in(&ret, tp, 1) == Some(0)
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
                            p = crate::symbol::subst_tparams_slice(&ids, &vals, &p);
                            param_tys = param_tys
                                .iter()
                                .map(|q| crate::symbol::subst_tparams_slice(&ids, &vals, q))
                                .collect();
                            ret = crate::symbol::subst_tparams_slice(&ids, &vals, &ret);
                            recv_ty = recv_ty
                                .map(|t| crate::symbol::subst_tparams_slice(&ids, &vals, &t));
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
                            .solve_open_from_arg(&a.ty, &p, &open)
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
                    if let TreeKind::Function { body, .. } = &a.kind {
                        let body_ty = body.ty.widen_constant();
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
                            Type::Function { params, ret } if params.is_empty() => (**ret).clone(),
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
                                fun.ty = crate::symbol::subst_tparams_slice(&tps, &inst, &fun.ty);
                                ret = crate::symbol::subst_tparams_slice(&tps, &inst, &ret);
                            } else if tps.len() == 2 && !elem.is_no_type() && !elem.is_error() {
                                let bs = fr.as_ref().clone();
                                let inst = vec![bs, elem];
                                param_tys = param_tys
                                    .iter()
                                    .map(|t| crate::symbol::subst_tparams_slice(&tps, &inst, t))
                                    .collect();
                                fun.ty = crate::symbol::subst_tparams_slice(&tps, &inst, &fun.ty);
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
                                        if params.is_empty() =>
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
                            .filter(|(_, t)| {
                                // A solution that is *this* call's own variable
                                // is no solution -- `T := T` leaves the result
                                // exactly as it was. The caller's type
                                // parameter is a perfectly good one, though:
                                // `def const[T](v: T): GR[T] = mk(_ => v)`
                                // solves `mk`'s `T` to `const`'s, and
                                // rejecting every `TypeParam` printed the
                                // result as `GR[T] required GR[T]`.
                                !t.is_no_type()
                                    && !t.is_error()
                                    && !matches!(t, Type::Nothing)
                                    && !mentions_tparam(t, &tps)
                            })
                            .collect();
                        if !inst.is_empty() {
                            let ids: Vec<SymbolId> = inst.iter().map(|(id, _)| *id).collect();
                            let args_t: Vec<Type> = inst.iter().map(|(_, t)| t.clone()).collect();
                            ret = crate::symbol::subst_tparams_slice(&ids, &args_t, &ret);
                        }
                    }
                }
                let leftover =
                    self.fill_defaults_and_implicits(tree.span, args, &param_tys, fun, pt);
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
                        if let Some(t2) = self
                            .st
                            .lookup("Tuple2")
                            .into_iter()
                            .find(|id| self.st.get(*id).kind == crate::symbol::SymKind::Class)
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
                } else if method_name == "apply"
                    && !sym.is_none()
                    && self.st.get(self.st.get(sym).owner).name.starts_with("Some")
                {
                    if let Some(a0) = args.first() {
                        ret = Type::Class {
                            sym: self.st.some_sym,
                            args: vec![a0.ty.widen_constant()],
                        };
                    }
                } else if method_name == "map" {
                    if self.is_array_ops_ty(recv_ty.as_ref()) {
                        if let Some(a0) = args.first() {
                            if let Type::Function { ret: fr, .. } = &a0.ty {
                                ret = Type::Array(Box::new(fr.as_ref().widen_constant()));
                            }
                        }
                    } else if let Some(t) = self.either_map_result(recv_ty.as_ref(), args) {
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
                                    (r, None) => r,
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
                        let keeps_view = crate::prelude_viewc::declares_view_result(&method_name)
                            && self.st.get(r).jvm_name == "scala/collection/SeqView";
                        if !keeps_view {
                            if let Some(t) = self.rebuild_from_receiver(r, &ret) {
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
                            Type::Class { args, .. } if args.len() >= 2 => Some(args[1].clone()),
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
                            } else if let Some(r) = self.receiver_collection_root(recv_ty.as_ref())
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
                                if let Some(t) = named.and_then(|n| self.rebuild_widened(r, &n)) {
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
                                    Type::Class { args, .. } if !args.is_empty() => args[0].clone(),
                                    Type::Array(e) => e.as_ref().clone(),
                                    other => other.clone(),
                                };
                                ret = Type::Array(Box::new(elem.widen_constant()));
                            }
                        }
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
                } else if method_name == "apply" && !sym.is_none() {
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
                                    if let Some(map) = self.factory_result_class(&ret, "Map", 2) {
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
                            if let Some(cls) =
                                self.factory_result_class(&ret, owner_n.trim_end_matches('$'), 1)
                            {
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
                            if let Some(cls) =
                                self.st.lookup(cname).into_iter().find(|id| {
                                    self.st.get(*id).kind == crate::symbol::SymKind::Class
                                })
                            {
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
                let arg_tys: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
                let ret = self.subst_dependent_members(&param_tys, &arg_tys, &ret);
                tree.ty = self.instantiate_leftover_tparams(sym, ret, pt, args.len());
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
                        self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt)
                    {
                        fun.sym = sym;
                        tree.sym = sym;
                        let own = (!sym.is_none()).then(|| self.st.get(sym).tparams.clone());
                        for (i, a) in args.iter_mut().enumerate() {
                            let p = param_at(&param_tys, i).cloned().unwrap_or(Type::NoType);
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
                        tree.ty = ret;
                        return;
                    }
                }
                if self.widen_with_companion(fun) {
                    let fun_ty = fun.ty.clone();
                    if let OverloadPick::Found(sym, param_tys, ret) =
                        self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt)
                    {
                        fun.sym = sym;
                        tree.sym = sym;
                        self.adapt_args_to_params(args, &param_tys, sym);
                        tree.ty = ret;
                        return;
                    }
                }
                // The same widening for a receiver that already *is* the
                // companion (`BigDecimal(3L)` through the `scala` package
                // object's alias): the alternatives the prelude did not write
                // by hand are still in the pickle.
                if self.widen_module_from_pickle(fun) {
                    let fun_ty = fun.ty.clone();
                    if let OverloadPick::Found(sym, param_tys, ret) =
                        self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt)
                    {
                        fun.sym = sym;
                        tree.sym = sym;
                        self.adapt_args_to_params(args, &param_tys, sym);
                        tree.ty = ret;
                        return;
                    }
                }
                if self.rewrite_apply_extension(fun) {
                    let fun_ty = fun.ty.clone();
                    match self.resolve_overload(&fun_ty, fun.sym, &arg_tys, pt) {
                        OverloadPick::Found(sym, param_tys, ret) => {
                            fun.sym = sym;
                            tree.sym = sym;
                            self.adapt_args_to_params(args, &param_tys, sym);
                            tree.ty = ret;
                            return;
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
                        OverloadPick::None => {}
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
    }

    /// `xs.map(f)` is retyped as `Coll[B]` for a one-parameter collection whose
    /// prelude signature does not carry the element type through on its own.
    /// A receiver that takes any other number of parameters cannot be written
    /// that way -- a user's `def map[R2](f: R => R2): Act[R2, NoStream, E]`
    /// would lose two arguments -- so its declared result type stands.
    fn takes_one_type_parameter(&self, cls: SymbolId) -> bool {
        self.st.get(cls).tparams.len() == 1
    }

    /// A `scala.collection` class whose real `map` returns its own type
    /// constructor. `Range` (no type parameter of its own) and a user class
    /// that extends one of these are not among them.
    fn maps_to_own_class(&self, cls: SymbolId) -> bool {
        self.st.get(cls).jvm_name.starts_with("scala/collection/")
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
            Type::Class { args, .. } if !args.is_empty() => Some(args[0].clone()),
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

    fn tuple2_sym(&self) -> SymbolId {
        self.st
            .lookup("Tuple2")
            .into_iter()
            .find(|id| self.st.get(*id).kind == crate::symbol::SymKind::Class)
            .unwrap_or(SymbolId::NONE)
    }

    /// Least upper bound, with the numeric widenings nsc applies before lubbing
    /// (`lub(Int, Long) = Long`).
    pub(crate) fn lub_ty(&self, a: &Type, b: &Type) -> Type {
        if let Some(t) = numeric_widen(a, b).or_else(|| numeric_widen(b, a)) {
            return t;
        }
        self.st.lub(a, b)
    }
}
