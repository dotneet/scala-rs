#![allow(dead_code)]
//! `match`, its cases, and pattern typing.
//!
//! Types the scrutinee and each pattern against it: constructor patterns and
//! the type arguments they take from the scrutinee, `unapply` and
//! `unapplySeq` extractors including their sequence and named-argument forms,
//! bindings, and `super` selections used from a case body. Ends with the
//! exhaustiveness check over a sealed hierarchy.

use crate::check::*;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

impl Typer {
    pub(crate) fn type_match(&mut self, tree: &mut Tree, pt: &Type) {
        let (sel, cases) = match &mut tree.kind {
            TreeKind::Match { selector, cases } => (selector, cases),
            _ => return,
        };
        self.type_expr(sel, &Type::NoType);
        let sel_ty = sel.ty.clone();
        let mut res = Type::Nothing;
        let mut branch_tys = Vec::new();
        for c in cases.iter_mut() {
            self.st.push_scope();
            self.type_pattern(&mut c.pat, &sel_ty);
            if !c.guard.is_empty() {
                self.type_expr(&mut c.guard, &Type::Boolean);
            }
            self.type_expr(&mut c.body, pt);
            res = self.lub_branches(&res, &c.body.ty);
            branch_tys.push(c.body.ty.clone());
            self.st.pop_scope();
        }
        let span = tree.span;
        tree.ty = self.branch_result_ty(pt, &branch_tys, res);
        if let TreeKind::Match { selector, cases } = &tree.kind {
            // The pattern-matching function a `for` generator desugars to is
            // guarded by the `withFilter` the parser puts in front of it, so
            // nsc marks it synthetic and never reports it as inexhaustive.
            // Its scrutinee is the parser's own `x$forN` / `x$forfN`, a name
            // no source writes.
            let for_desugaring = selector
                .name()
                .is_some_and(|n| n.starts_with("x$for") && n.len() > 5);
            if !for_desugaring {
                self.check_match_exhaustive(span, &sel_ty, cases);
            }
            if tree_has_switch(selector) && !match_can_switch(&sel_ty, cases) {
                self.warning(
                    selector.span,
                    "could not emit switch for @switch annotated match",
                );
            }
        }
    }

    pub(crate) fn type_case(&mut self, c: &mut CaseDef, pt: &Type) {
        self.st.push_scope();
        self.type_pattern(&mut c.pat, &Type::Any);
        if !c.guard.is_empty() {
            self.type_expr(&mut c.guard, &Type::Boolean);
        }
        self.type_expr(&mut c.body, pt);
        self.st.pop_scope();
    }

    /// `::` is entered both as the class `$colon$colon` and as an alias symbol
    /// carrying its type. Patterns need the real class, which holds the
    /// constructor fields.
    /// The class a **qualified** constructor pattern (`p.C(x)`) names, read
    /// off the already-typed callee rather than by looking its last segment up
    /// in the lexical scope.
    ///
    /// `None` for a bare `Ident`, which the lexical lookup handles, and for a
    /// callee that resolved to something other than a class or its companion
    /// -- an `unapply` reached through a value, say -- where the extractor
    /// arms are the ones that apply.
    fn qualified_pattern_class(&self, fun: &Tree) -> Option<SymbolId> {
        if !matches!(fun.kind, TreeKind::Select { .. }) || fun.sym.is_none() {
            return None;
        }
        match self.st.get(fun.sym).kind {
            SymKind::Class => Some(self.follow_class_alias(fun.sym)),
            SymKind::Module | SymKind::ModuleClass => {
                let mcls = self.st.module_class_of(fun.sym);
                let base = self.st.get(mcls).name.trim_end_matches('$').to_string();
                let owner = self.st.get(mcls).owner;
                self.st
                    .get(owner)
                    .members
                    .iter()
                    .copied()
                    .find(|&m| self.st.get(m).kind == SymKind::Class && self.st.get(m).name == base)
                    .map(|c| self.follow_class_alias(c))
            }
            _ => None,
        }
    }

    fn follow_class_alias(&self, id: SymbolId) -> SymbolId {
        if !self.st.get(id).ctor_fields.is_empty() {
            return id;
        }
        if let Type::Class { sym, .. } = &self.st.get(id).ty {
            if *sym != id && self.st.get(*sym).kind == SymKind::Class {
                return *sym;
            }
        }
        id
    }

    /// Recover a constructor pattern's type arguments from the scrutinee:
    /// matching `Option[Int]` against `Some` gives `Some[Int]`. Walks the
    /// pattern class's base types to find the scrutinee's class.
    fn pattern_class_args(&self, class_id: SymbolId, sel_ty: &Type) -> Vec<Type> {
        let tps = self.st.get(class_id).tparams.clone();
        if tps.is_empty() {
            return Vec::new();
        }
        // A tuple scrutinee is `Type::Tuple`, which has no class symbol; its
        // element types are the pattern class's arguments directly.
        if let Type::Tuple(ts) = sel_ty {
            if ts.len() == tps.len() {
                return ts.clone();
            }
        }
        let Some(sel_sym) = self.st.class_sym_of(sel_ty) else {
            return Vec::new();
        };
        let self_ty = Type::Class {
            sym: class_id,
            args: tps.iter().map(|t| Type::TypeParam(*t)).collect(),
        };
        let mut cands = vec![self_ty];
        let mut work: Vec<Type> = self
            .st
            .get(class_id)
            .parents
            .iter()
            .map(|p| self.st.subst_tparams(class_id, &[], p))
            .collect();
        let mut seen = std::collections::HashSet::new();
        while let Some(p) = work.pop() {
            let Some(psym) = self.st.class_sym_of(&p) else {
                continue;
            };
            if !seen.insert(psym.0) {
                continue;
            }
            for q in self.st.get(psym).parents.clone() {
                work.push(self.st.subst_as_seen_from(&p, &q));
            }
            cands.push(p);
        }
        for c in cands {
            if self.st.class_sym_of(&c) != Some(sel_sym) {
                continue;
            }
            let args: Vec<Type> = tps
                .iter()
                .map(|tp| unify_one(*tp, &c, sel_ty).unwrap_or(Type::Any))
                .collect();
            if args.iter().any(|a| !matches!(a, Type::Any)) {
                return args;
            }
        }
        Vec::new()
    }

    fn type_pattern(&mut self, pat: &mut Tree, sel_ty: &Type) {
        // Read before the borrow below: the parser sets it on a backquoted
        // name, which is a stable identifier pattern however it is spelled.
        let stable_hint = pat.stable_pat;
        match &mut pat.kind {
            TreeKind::Wildcard => {
                pat.ty = sel_ty.clone();
            }
            TreeKind::Literal { lit } => {
                // `Null` conforms to no value type, so `case null` against a
                // primitive scrutinee is the mismatch nsc reports. Without
                // this the case was accepted and silently never taken.
                let primitive_sel = || {
                    matches!(
                        sel_ty.widen_constant(),
                        Type::Int
                            | Type::Long
                            | Type::Double
                            | Type::Float
                            | Type::Short
                            | Type::Byte
                            | Type::Char
                            | Type::Boolean
                            | Type::Unit
                    )
                };
                if matches!(lit, Lit::Null) && primitive_sel() {
                    let want = self.st.display_type(sel_ty);
                    self.error(
                        pat.span,
                        format!("type mismatch; found: Null(null)  required: {want}"),
                    );
                }
                pat.ty = match lit {
                    Lit::Int(_) => Type::Int,
                    Lit::Boolean(_) => Type::Boolean,
                    Lit::String(_) => Type::String,
                    Lit::Long(_) => Type::Long,
                    Lit::Double(_) => Type::Double,
                    Lit::Char(_) => Type::Char,
                    Lit::Null => Type::Null,
                    Lit::Unit => Type::Unit,
                    _ => Type::Any,
                };
            }
            TreeKind::Ident { name } => {
                // Stable id vs variable: if name is a known module/val, treat as stable.
                let mut found = self.st.lookup(name);
                // A scope that binds the name only in the *type* namespace
                // does not hide a term of that name further out --
                // `Typer::type_ident` already applies this rule, and a
                // stable-id pattern did its own lookup without it. slick's
                // `object syntax { type HNil = heterogeneous.HNil.type }` is
                // imported into `HList.scala`, so `case (HNil, _)` picked the
                // alias, which is not a term at all: `HList$.concat` came out
                // as `throw new RuntimeException("cannot load HNil")` where
                // nsc matches the object.
                if !found.iter().any(|&s| self.st.is_term_namespace_sym(s)) {
                    let terms = self.st.lookup_term(name);
                    if !terms.is_empty() {
                        found = terms;
                    }
                }
                // A backquoted name is stable however it is spelled; the
                // parser has already marked it.
                let is_varid = !stable_hint
                    && name
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_lowercase() || c == '_');
                // SLS 8.1.5 wants a *stable* id here. `found[0]` can be a
                // `def` of the same name, which nsc rejects rather than
                // calling, so pick the value or module if the scope has one.
                let stable = found
                    .iter()
                    .copied()
                    .find(|s| matches!(self.st.get(*s).kind, SymKind::Term | SymKind::Module))
                    .or_else(|| found.first().copied());
                // A name that answers only in the *type* namespace is not a
                // value, and nsc says so rather than matching against
                // something: `not found: value X / Identifiers that begin with
                // uppercase are not pattern variables but match the value in
                // scope`. Taking it anyway put a symbol the backend has no way
                // to load into the pattern, and `load_symbol` fell through to
                // `throw new RuntimeException("cannot load X")` -- a stub in
                // the middle of a method that compiled without a word.
                if let Some(sym) = stable.filter(|_| !is_varid) {
                    if !self.st.is_term_namespace_sym(sym) {
                        self.error(
                            pat.span,
                            format!(
                                "not found: value {name}\n\
                                 Identifiers that begin with uppercase are not pattern \
                                 variables but match the value in scope."
                            ),
                        );
                        pat.ty = Type::Error;
                        return;
                    }
                }
                if let Some(sym) = stable.filter(|_| !is_varid) {
                    // SLS 8.1.5: the identifier of a stable-id pattern has to
                    // be *stable*. A `var` is not, and nsc rejects it rather
                    // than comparing against whatever the variable holds when
                    // the match runs (`neg/t3816`). Only reachable through a
                    // backquoted lowercase name or an uppercase one; an
                    // ordinary lowercase name is a fresh binding and never
                    // gets here.
                    if self.st.get(sym).flags.contains(Flags::MUTABLE) {
                        let shown = if stable_hint {
                            format!("`{name}`")
                        } else {
                            name.clone()
                        };
                        self.error(
                            pat.span,
                            format!("stable identifier required, but {shown} found"),
                        );
                    }
                    pat.sym = sym;
                    pat.ty = self.st.get(sym).ty.clone();
                    // A `val` read back from a classfile is a nullary *method*
                    // (its accessor), so its type is `Type::Method`. Left that
                    // way, `uncurry`'s `eta_if_method` eta-expands the pattern
                    // into `() => …`, which `gen_pattern` does not recognise
                    // and therefore compiles to no test at all: `import
                    // Int.MaxValue; 5 match { case MaxValue => … }` took the
                    // first case. Reduce it to the result type here -- a
                    // stable id pattern is the value, never the function.
                    if let Type::Method { paramss, ret } = &pat.ty {
                        if paramss.iter().all(|c| c.is_empty()) {
                            pat.ty = (**ret).clone();
                        }
                    }
                    // Tell the backend this is a comparison and not a binding:
                    // a resolved `val` and a fresh pattern variable are both
                    // `SymKind::Term`, so the symbol alone cannot say.
                    pat.stable_pat = true;
                } else {
                    let n = name.clone();
                    let id =
                        self.st
                            .alloc(n.clone(), self.st.owner, SymKind::Term, Flags::PARAM, "");
                    self.st.get_mut(id).ty = sel_ty.clone();
                    self.st.enter_in_current(&n, id);
                    pat.sym = id;
                    pat.ty = sel_ty.clone();
                }
            }
            TreeKind::Select { .. } => {
                // Stable identifier pattern (`Color.RED`, `java.lang.Thread.State.NEW`).
                // A `Byte`/`Short`/`Char` scrutinee is an `int` on the stack and
                // the pattern is only compared with `==`, so nsc accepts an
                // `Int` constant there (`case DatabaseMetaData.functionNoTable`
                // against `r.nextShort()`). Demanding conformance to the
                // scrutinee would reject it for no runtime reason.
                let pt = match sel_ty.widen_constant() {
                    Type::Byte | Type::Short | Type::Char => Type::Int,
                    _ => relax_abstract_targs(sel_ty),
                };
                // nsc does not demand conformance here, only that the two
                // types could have a common instance: `case Ids.other =>`
                // (an `Other`) against an `ST[Int]` scrutinee compiles,
                // because a subclass of `Other` could still be an `ST[Int]`.
                // It *is* an error when one side is final and unrelated
                // (`String`, a `final class`, a primitive), which is what
                // scalac 2.13.16 reports for exactly those cases.
                self.type_expr(pat, &Type::NoType);
                if !self.stable_pattern_compatible(&pat.ty, &pt) {
                    self.adapt(pat, &pt);
                }
            }
            TreeKind::Bind { name, body } => {
                self.type_pattern(body, sel_ty);
                // A constructor pattern's `body.ty` is already the narrowed
                // class type (`type_pattern`'s ctor-pattern arm sets it).
                // A custom `unapply`'s `body.ty` deliberately stays the
                // scrutinee's own type -- the backend's `gen_unapply_pattern`
                // reads it back to know whether the pattern already proved
                // the runtime type test redundant, and narrowing it there
                // would make that check trivially (and wrongly) true. The
                // extractor's receiver type is what `c` should be bound at,
                // though (`case c @ LiteralNode(_) if c.volatileHint`), so
                // narrow it here instead, same as nsc's own type test order:
                // run the inner pattern's check, *then* narrow what it
                // proved.
                let bind_ty = if matches!(body.kind, TreeKind::UnApply { .. }) {
                    let receiver = self
                        .unapply_receiver_type(body.sym, sel_ty)
                        .unwrap_or_else(|| body.ty.clone());
                    if self.st.is_sub_type(&body.ty, &receiver) {
                        body.ty.clone()
                    } else {
                        receiver
                    }
                } else {
                    body.ty.clone()
                };
                let n = name.clone();
                let id = self
                    .st
                    .alloc(n.clone(), self.st.owner, SymKind::Term, Flags::PARAM, "");
                self.st.get_mut(id).ty = bind_ty.clone();
                self.st.enter_in_current(&n, id);
                pat.sym = id;
                pat.ty = bind_ty;
            }
            TreeKind::Apply { fun, args } => {
                // nsc types a constructor pattern's function in
                // `typingConstructorPattern` mode, where a non-stable method
                // of that name does not qualify. Only a bare `Ident` needs the
                // rule; a `Select`'s qualifier must keep ordinary resolution.
                let ctor_pat = matches!(fun.kind, TreeKind::Ident { .. });
                let saved_ctor_pat = std::mem::replace(&mut self.ctor_pattern_fun, ctor_pat);
                self.type_expr(fun, &Type::NoType);
                self.ctor_pattern_fun = saved_ctor_pat;
                // `case (a, b) =>` is `scala.Tuple2(a, b)`: a synthesized name
                // is resolved in package `scala`, never lexically. Note the
                // ordinary path uses `lookup`, which stops at the first scope
                // binding the name *at all*, so a `def Tuple2` in scope hid
                // the class here even though it is not a class.
                let scala_ref = fun.scala_ref;
                // A *qualified* pattern names its class outright, and `fun`
                // has just been typed, so take the class from the symbol it
                // resolved to. Looking the last segment up lexically instead
                // finds whatever else is in scope under that simple name:
                // `case Ior.Left(a)`, which cats writes 76 times in
                // `data/Ior.scala`, found the prelude's `scala.util.Left` and
                // bound `a` to *its* type parameter. Harmless while
                // `scala.util.Left` carried no `CASE` flag, because the wrong
                // class then lost to the extractor arm below; giving the
                // prelude the flag the library pickles made it win.
                let class_id = self.qualified_pattern_class(fun).or_else(|| {
                    fun.name().and_then(|n| {
                        let mut cands = if scala_ref {
                            self.st.lookup_scala(n)
                        } else {
                            self.st.lookup(n)
                        };
                        // `lookup` stops at the innermost scope binding the name
                        // at all, and a *method* of that name is not a
                        // constructor pattern in nsc's
                        // `typingConstructorPattern` mode. cats' `NonEmptyList`
                        // declares `def ::[AA >: A](a: AA)`, which hid the case
                        // class `scala.::` from every `case h :: t` in the class
                        // body -- five "not found: extractor ::" in one file.
                        // Same rule `type_ident` already applies to the
                        // pattern's function; see
                        // `SymbolTable::lookup_extractor`.
                        if ctor_pat
                            && !cands.is_empty()
                            && cands
                                .iter()
                                .all(|&s| self.st.get(s).kind == SymKind::Method)
                        {
                            let alt = self.st.lookup_extractor(n);
                            if !alt.is_empty() {
                                cands = alt;
                            }
                        }
                        cands
                            .into_iter()
                            .find(|s| self.st.get(*s).kind == SymKind::Class)
                            .map(|s| self.follow_class_alias(s))
                    })
                });
                if class_id.is_some() {
                    self.reorder_named_pattern_args(args, class_id.unwrap());
                }
                let has_star = args.iter().any(pattern_has_star);
                if has_star {
                    if let Some(last) = args.last() {
                        if !pattern_has_star(last) {
                            self.error(pat.span, "`_*` must be the last pattern argument");
                        }
                    }
                    for a in args.iter().take(args.len().saturating_sub(1)) {
                        if pattern_has_star(a) {
                            self.error(a.span, "`_*` must be the last pattern argument");
                        }
                    }
                }
                // `scala.#::` is overloaded, one `unapply` per lazy sequence
                // type, and the `Stream` one can only be declared once
                // `Stream` itself is in the table.
                if fun.name() == Some("#::") {
                    if let Some(c) = self.st.class_sym_of(sel_ty) {
                        crate::prelude_consextract::ensure_stream_support(&mut self.st, c);
                    }
                }
                let unapply = self.find_unapply(fun, sel_ty);
                let unapply_seq = self.find_unapply_seq(fun);
                // `def unapply(n: Nd) = Some((n.v, n.tag))` has no result type
                // of its own; without completing it the pattern would see
                // `<notype>` and count one sub-pattern instead of two.
                for u in unapply.iter().chain(unapply_seq.iter()) {
                    self.complete_lazy_sig(*u, pat.span);
                }
                // Without the jar there is no `Array$` / `Vector$` companion
                // at all, so a sequence pattern on one finds no `unapplySeq`
                // and every sub-pattern would silently come out `Any`. Say
                // what is actually missing.
                if unapply.is_none() && unapply_seq.is_none() {
                    if let Some(c) = class_id {
                        self.check_missing_seq_factory(c, pat.span);
                    }
                }
                // A constructor pattern binds one sub-pattern per field. A class
                // that also has an `unapply` of its own may take a different
                // number (slick's `LiteralNode(v)` on a three-field class), so
                // the constructor only wins when its arity fits; otherwise the
                // extractor branches below get their turn. A repeated last
                // parameter takes any number.
                let repeated_elem = class_id.and_then(|c| self.st.repeated_case_element(c));
                let ctor_fits = class_id.is_some_and(|c| {
                    let fields = &self.st.get(c).ctor_fields;
                    if repeated_elem.is_some() {
                        args.len() >= fields.len() - 1
                    } else {
                        args.len() == fields.len()
                    }
                });
                let has_extractor = unapply.is_some() || unapply_seq.is_some();
                // SLS 8.1.6/8.1.7: only a *case* class has a constructor
                // pattern. A plain class whose companion defines an extractor
                // is matched through that extractor even when its constructor
                // happens to take as many arguments -- slick's
                // `final class ConstArray[+T](a: Array[Any], val length: Int)`
                // has an `unapplySeq`, and `case ConstArray(disc, map)` bound
                // `Array[Any]` and `Int` instead of two `Node`s, so
                // `ConstArray(disc, map)` on the right-hand side came out
                // `ConstArray[Any]` (`compiler/ExpandSums.scala:245`).
                // The `ctor_fields`-only arm stays for a class with no
                // extractor at all, which is where it was needed.
                let is_case = class_id.is_some_and(|c| self.st.get(c).flags.contains(Flags::CASE));
                let repeated_case = is_case && repeated_elem.is_some();
                let use_ctor = (!has_star || repeated_case)
                    && class_id.is_some_and(|c| {
                        let s = self.st.get(c);
                        s.flags.contains(Flags::CASE) || !s.ctor_fields.is_empty()
                    })
                    && ((ctor_fits && is_case) || !has_extractor);
                if use_ctor {
                    let class_id = class_id.unwrap();
                    let fields = self.st.get(class_id).ctor_fields.clone();
                    // Nothing said so before: `case P(a, b)` on a one-field `P`
                    // with no `unapply` to fall back on typed `b` as `Any` and
                    // reached the backend, which threw a
                    // `RuntimeException("pattern arity")` at run time.
                    if !ctor_fits {
                        self.error(
                            pat.span,
                            format!(
                                "wrong number of arguments for pattern {}: expected {}, found {}",
                                self.st.get(class_id).name,
                                fields.len(),
                                args.len()
                            ),
                        );
                    }
                    // `case Some(x)` on an `Option[Int]` binds `x: Int`: recover
                    // the pattern class's arguments from the scrutinee.
                    let cargs = self.pattern_class_args(class_id, sel_ty);
                    let class_ty = Type::Class {
                        sym: class_id,
                        args: cargs.clone(),
                    };
                    for (i, a) in args.iter_mut().enumerate() {
                        let ft = if i + 1 >= fields.len() && repeated_elem.is_some() {
                            Type::Repeated(Box::new(repeated_elem.clone().unwrap()))
                        } else {
                            fields
                                .get(i)
                                .map(|f| self.st.get(*f).ty.clone())
                                .unwrap_or(Type::Any)
                        };
                        let ft = if cargs.is_empty() {
                            ft
                        } else {
                            self.st.subst_tparams(class_id, &cargs, &ft)
                        };
                        let ft = match ft {
                            Type::Repeated(elem) if pattern_has_star(a) => {
                                self.seq_of(&elem).unwrap_or(Type::Class {
                                    sym: self.st.list_sym,
                                    args: vec![*elem],
                                })
                            }
                            Type::Repeated(elem) => *elem,
                            ft => ft,
                        };
                        self.type_pattern(a, &ft);
                    }
                    pat.ty = class_ty;
                    pat.sym = class_id;
                } else if let Some(u) = unapply.filter(|_| !has_star) {
                    let extracted = self.unapply_extracted_types(u);
                    let extracted = self.subst_unapply_tparams(u, sel_ty, extracted);
                    if args.len() != extracted.len() && !extracted.is_empty() {
                        self.error(
                            pat.span,
                            format!(
                                // The symbol is always spelled `unapply`; name
                                // the extractor the source names.
                                "extractor {} expects {} argument(s), found {}",
                                fun.name().unwrap_or("<pattern>"),
                                extracted.len(),
                                args.len()
                            ),
                        );
                    }
                    for (i, a) in args.iter_mut().enumerate() {
                        let ft = extracted.get(i).cloned().unwrap_or(Type::Any);
                        self.type_pattern(a, &ft);
                    }
                    // `pat.ty` here stays the *scrutinee's* type, not the
                    // extractor's receiver type: the backend's
                    // `gen_unapply_pattern` reads it back to decide whether
                    // the runtime `instanceof` test is redundant (comparing
                    // it against the extractor's own parameter type), and
                    // that check is only sound against what was true walking
                    // *into* this pattern. The type a `c @ Extractor(...)`
                    // binds `c` at is narrowed separately in `TreeKind::Bind`
                    // below, which is the only place that needs it.
                    let fun = std::mem::replace(fun, Box::new(Tree::dummy(TreeKind::Empty)));
                    let args = std::mem::take(args);
                    pat.kind = TreeKind::UnApply { fun, args };
                    pat.sym = u;
                    pat.ty = sel_ty.clone();
                } else if let Some(u) = unapply_seq {
                    // `case Seq((s, _))` on a `Seq[(TermSymbol, Node)]` binds
                    // `s: TermSymbol`. Only `List` was read off the scrutinee;
                    // every other sequence kept `unapplySeq`'s own `A`, so
                    // `Some(s)` came out a `Some[A]`. Unify the extractor's
                    // parameter with the scrutinee, exactly as the `unapply`
                    // branch above does -- a custom `unapplySeq` whose element
                    // type is not one of its type parameters is left alone.
                    let elem = match sel_ty {
                        Type::Class { sym, args }
                            if *sym == self.st.list_sym && !args.is_empty() =>
                        {
                            args[0].clone()
                        }
                        _ => {
                            let own = self.unapply_seq_elem_type(u);
                            self.subst_unapply_tparams(u, sel_ty, vec![own.clone()])
                                .into_iter()
                                .next()
                                .unwrap_or(own)
                        }
                    };
                    self.check_seq_pattern_backing(u, pat.span);
                    self.note_seq_extractor_payload(u);
                    // `rest @ _*` gets the container the extractor's own
                    // result type names: `List` for `List.unapplySeq` (and for
                    // a user extractor returning `Option[List[T]]`), `Seq` for
                    // the `Seq` / `Vector` / `IndexedSeq` / `Array` factories,
                    // which is what scalac's `drop$extension` returns.
                    let star_ty = self.unapply_seq_star_type(u, &elem);
                    let n = args.len();
                    for (i, a) in args.iter_mut().enumerate() {
                        if pattern_has_star(a) {
                            self.type_pattern(a, &star_ty);
                        } else if has_star && i + 1 == n {
                            self.type_pattern(a, sel_ty);
                        } else {
                            self.type_pattern(a, &elem);
                        }
                    }
                    let fun = std::mem::replace(fun, Box::new(Tree::dummy(TreeKind::Empty)));
                    let args = std::mem::take(args);
                    pat.kind = TreeKind::UnApply { fun, args };
                    pat.sym = u;
                    pat.ty = sel_ty.clone();
                } else if let Some(c) = class_id {
                    let fields = self.st.get(c).ctor_fields.clone();
                    let targs = self.pattern_class_targs(c, sel_ty);
                    for (i, a) in args.iter_mut().enumerate() {
                        let ft = fields
                            .get(i)
                            .map(|f| self.st.get(*f).ty.clone())
                            .unwrap_or(Type::Any);
                        let ft = if targs.is_empty() {
                            ft
                        } else {
                            self.st.subst_tparams(c, &targs, &ft)
                        };
                        self.type_pattern(a, &ft);
                    }
                    pat.ty = Type::Class {
                        sym: c,
                        args: targs,
                    };
                    pat.sym = c;
                } else {
                    self.error(
                        pat.span,
                        format!("not found: extractor {}", fun.name().unwrap_or("<pattern>")),
                    );
                    pat.ty = sel_ty.clone();
                }
            }
            TreeKind::Star { elem } => {
                self.type_pattern(elem, sel_ty);
                pat.ty = sel_ty.clone();
            }
            TreeKind::UnApply { fun, args } => {
                self.type_expr(fun, &Type::NoType);
                let u = if pat.sym.is_none() {
                    self.find_unapply(fun, sel_ty).unwrap_or(SymbolId::NONE)
                } else {
                    pat.sym
                };
                let extracted = if u.is_none() {
                    vec![Type::Any; args.len()]
                } else {
                    self.unapply_extracted_types(u)
                };
                for (i, a) in args.iter_mut().enumerate() {
                    let ft = extracted.get(i).cloned().unwrap_or(Type::Any);
                    self.type_pattern(a, &ft);
                }
                pat.sym = u;
                pat.ty = sel_ty.clone();
            }
            TreeKind::Typed { expr, tpt } => {
                let saved = std::mem::replace(&mut self.pattern_tpt, true);
                let ty = self.tree_to_type(tpt);
                self.pattern_tpt = saved;
                let ty = self.pattern_targs_from_scrutinee(&ty, sel_ty);
                if !self.typed_pattern_compatible(&ty, sel_ty) {
                    self.error(
                        tpt.span,
                        format!(
                            "pattern type {} is incompatible with scrutinee type {}",
                            self.st.display_type(&ty),
                            self.st.display_type(sel_ty)
                        ),
                    );
                }
                self.type_pattern(expr, &ty);
                pat.ty = ty;
            }
            TreeKind::Alternative { trees } => {
                for t in trees {
                    self.type_pattern(t, sel_ty);
                }
                pat.ty = sel_ty.clone();
            }
            _ => {
                pat.ty = sel_ty.clone();
            }
        }
    }

    /// Decide whether the two types can have a common instance. Unlike an
    /// assignment, a type test permits narrowing and unrelated open traits.
    fn typed_pattern_compatible(&mut self, pattern: &Type, scrutinee: &Type) -> bool {
        fn upper(typer: &Typer, ty: &Type) -> Type {
            let mut ty = typer.st.dealias(ty).widen_constant();
            let mut seen = std::collections::HashSet::new();
            loop {
                ty = match ty {
                    Type::TypeParam(id) | Type::TypeMember(id) if seen.insert(id) => {
                        typer.st.get(id).bound_hi.clone().unwrap_or(Type::Any)
                    }
                    Type::Annotated { tpe, .. } => *tpe,
                    _ => return ty,
                };
                ty = typer.st.dealias(&ty);
            }
        }
        fn uncertain(ty: &Type) -> bool {
            sig_has_abstract_type(ty)
                || type_mentions_wildcard(ty)
                || type_mentions_unresolved(ty)
                || matches!(ty, Type::NoType)
        }
        fn erased_args(ty: &Type) -> Type {
            match ty {
                Type::Class { sym, args } => Type::Class {
                    sym: *sym,
                    args: vec![Type::Wildcard; args.len()],
                },
                _ => ty.clone(),
            }
        }
        let p = upper(self, pattern);
        let s = upper(self, scrutinee);
        if p.is_error() || p.is_no_type() || s.is_error() || s.is_no_type() {
            return true;
        }
        // No instance can inherit the same invariant base twice with
        // different concrete arguments, even when neither class is final.
        let mut pb = self.st.base_type_seq(&p);
        let mut sb = self.st.base_type_seq(&s);
        pb.push(p.clone());
        sb.push(s.clone());
        // JVM signatures cannot encode variance. Parent classes may only
        // have that provisional metadata until a member is requested.
        let classes: std::collections::HashSet<_> = pb
            .iter()
            .chain(&sb)
            .filter_map(|t| {
                if let Type::Class { sym, .. } = t {
                    Some(*sym)
                } else {
                    None
                }
            })
            .collect();
        for class in classes {
            self.pickle
                .complete_class_variance(&mut self.st, &mut self.binary, class);
        }
        for a in &pb {
            let Type::Class { sym: ac, args: aa } = a else {
                continue;
            };
            for b in &sb {
                let Type::Class { sym: bc, args: ba } = b else {
                    continue;
                };
                if ac != bc {
                    continue;
                }
                for ((x, y), tp) in aa.iter().zip(ba).zip(&self.st.get(*ac).tparams) {
                    let f = self.st.get(*tp).flags;
                    if !f.contains(Flags::COVARIANT)
                        && !f.contains(Flags::CONTRAVARIANT)
                        && !uncertain(x)
                        && !uncertain(y)
                        && !(self.st.is_sub_type(x, y) && self.st.is_sub_type(y, x))
                    {
                        return false;
                    }
                }
            }
        }
        if let (Type::Array(a), Type::Array(b)) = (&p, &s) {
            if !uncertain(a) && !uncertain(b) {
                return self.st.is_sub_type(a, b) && self.st.is_sub_type(b, a);
            }
            return self.typed_pattern_compatible(a, b);
        }
        // Abstract arguments can be instantiated by the pattern. Concrete
        // arguments must still conform when either class is final; erasure
        // does not make a fruitless final-class test legal.
        let p = if uncertain(&p) { erased_args(&p) } else { p };
        let s = if uncertain(&s) { erased_args(&s) } else { s };
        self.stable_pattern_compatible(&p, &s)
    }

    /// nsc's `inferTypedPattern`: `case a: T[?, …]` keeps whatever the
    /// scrutinee already said about `T`'s parameters -- the pattern only has
    /// to *narrow* the type, and a wildcard written in it stands for "not
    /// stated here", not "forgotten".
    ///
    /// slick's `SynchronousDatabaseAction`:
    ///
    /// ```scala
    /// override def zip[R2, E2 <: Effect](a: DBIOAction[R2, NoStream, E2]) = a match {
    ///   case a: SynchronousDatabaseAction[?, ?, ?, ?] => … superZip(a) …
    /// ```
    ///
    /// `superZip` takes a `DBIOAction[R2, NoStream, E2]`, and a bare
    /// `SynchronousDatabaseAction[_, _, _, _]` is not one -- the scrutinee's
    /// `R2` / `NoStream` / `E2` were dropped by the pattern. Solving the
    /// pattern class's parameters from its own base type at the scrutinee's
    /// class puts them back, and leaves the result a plain class type, so
    /// erasure and codegen see exactly what they saw before.
    ///
    /// Only wildcards are filled in: an argument the source wrote stays as
    /// written, and a parameter the scrutinee does not pin (slick's `C`, which
    /// `DBIOAction` does not take) stays a wildcard.
    fn pattern_targs_from_scrutinee(&self, pat_ty: &Type, sel_ty: &Type) -> Type {
        let Type::Class { sym, args } = pat_ty else {
            return pat_ty.clone();
        };
        if args.is_empty() || !args.iter().any(|a| matches!(a, Type::Wildcard)) {
            return pat_ty.clone();
        }
        let tps = self.st.get(*sym).tparams.clone();
        if tps.len() != args.len() {
            return pat_ty.clone();
        }
        let Some(sel_sym) = self.st.class_sym_of(sel_ty) else {
            return pat_ty.clone();
        };
        if sel_sym == *sym {
            return pat_ty.clone();
        }
        let Some(Type::Class { args: sel_args, .. }) = self.base_type_instance(sel_ty, sel_sym, 0)
        else {
            return pat_ty.clone();
        };
        // The pattern class's own base type at the scrutinee's class, written
        // in the pattern class's parameters: `SynchronousDatabaseAction[R, S,
        // C, E]` seen as `DBIOAction` is `DBIOAction[R, S, E]`.
        let probe = Type::Class {
            sym: *sym,
            args: tps.iter().map(|&t| Type::TypeParam(t)).collect(),
        };
        let Some(Type::Class {
            args: base_args, ..
        }) = self.base_type_instance(&probe, sel_sym, 0)
        else {
            return pat_ty.clone();
        };
        if base_args.len() != sel_args.len() {
            return pat_ty.clone();
        }
        let mut out = args.clone();
        for (i, tp) in tps.iter().enumerate() {
            if !matches!(out[i], Type::Wildcard) {
                continue;
            }
            let Some(solved) = self.unify_tparam_all(*tp, &base_args, &sel_args) else {
                continue;
            };
            if solved.is_no_type()
                || solved.is_error()
                || matches!(solved, Type::Wildcard | Type::Nothing)
                || mentions_tparam(&solved, &tps)
            {
                continue;
            }
            out[i] = solved;
        }
        Type::Class {
            sym: *sym,
            args: out,
        }
    }

    pub(crate) fn register_sealed_from_namer(&mut self, tree: &Tree) {
        match &tree.kind {
            TreeKind::PackageDef { stats, .. } => {
                for s in stats {
                    self.register_sealed_from_namer(s);
                }
            }
            TreeKind::ClassDef { .. } => {
                if !tree.sym.is_none() {
                    self.register_sealed_child(tree.sym);
                }
                if let TreeKind::ClassDef { impl_, .. } = &tree.kind {
                    for s in &impl_.body {
                        self.register_sealed_from_namer(s);
                    }
                }
            }
            TreeKind::ModuleDef { .. } => {
                if !tree.sym.is_none() {
                    let cls = self.st.module_class_of(tree.sym);
                    self.register_sealed_child(cls);
                }
                if let TreeKind::ModuleDef { impl_, .. } = &tree.kind {
                    for s in &impl_.body {
                        self.register_sealed_from_namer(s);
                    }
                }
            }
            _ => {}
        }
    }

    fn find_class_named(&self, child: SymbolId, name: &str) -> Option<SymbolId> {
        let owner = self.st.get(child).owner;
        if !owner.is_none() {
            if let Some(id) = self.st.get(owner).members.iter().copied().find(|&m| {
                let s = self.st.get(m);
                s.name == name && s.is_class_like()
            }) {
                return Some(id);
            }
        }
        self.st
            .lookup(name)
            .into_iter()
            .find(|s| self.st.get(*s).is_class_like())
            .or_else(|| {
                self.st
                    .symbols
                    .iter()
                    .find(|s| s.name == name && s.is_class_like())
                    .map(|s| s.id)
            })
    }

    pub(crate) fn register_sealed_child(&mut self, child: SymbolId) {
        let parents = self.st.get(child).parents.clone();
        for p in parents {
            let pid = match &p {
                Type::Named { name, .. } => self.find_class_named(child, name),
                other => self.st.class_sym_of(other),
            };
            if let Some(pid) = pid {
                if self.st.is_sealed(pid) && !self.st.get(pid).children.contains(&child) {
                    self.st.get_mut(pid).children.push(child);
                }
            }
        }
    }

    /// The class a bare `this` denotes — the same rule as [`Self::super_owner`].
    ///
    /// A template's own constructor invocation is evaluated *outside* the
    /// template: in `new C(this.x) { … }` the argument is part of the
    /// enclosing expression, and nsc types it with the enclosing class as
    /// `enclClass`. slick's
    /// `new ClassLoader(this.getClass.getClassLoader) { … }` in
    /// `object ClassLoaderUtil` came out reading `this` from the anonymous
    /// class's own — still uninitialised — slot 0, which the verifier
    /// rejects: "Type uninitializedThis is not assignable to
    /// java/lang/Object". `class D extends Base(this.toString)` is the same
    /// rule and is legal Scala too.
    pub(crate) fn this_owner(&self, qual: Option<&str>) -> SymbolId {
        self.super_owner(qual)
    }

    /// The class whose parents a bare `super` names. Normally the enclosing
    /// class; inside a template's own parent list it is the class *around*
    /// that template, since a class cannot name its own `super` there.
    pub(crate) fn super_owner(&self, qual: Option<&str>) -> SymbolId {
        let base = match self.parent_ctx {
            Some((inner, outer)) if inner == self.st.this_class => outer,
            _ => self.st.this_class,
        };
        match qual {
            Some(name) => self.st.enclosing_class_named(base, name).unwrap_or(base),
            None => base,
        }
    }

    /// The type `super` stands for: the ancestor *as seen from* the current
    /// class, not the ancestor named on its own.
    ///
    /// `class Sub[A] extends Act[A]` inherits `Act`'s members at `A`; naming
    /// the bare class instead leaves `Act`'s own `R` in the member types, and
    /// `super.id` then reads as `Act[R]` where `Act[A]` is wanted — a mismatch
    /// whose two sides print the same when `Sub` also happens to call its
    /// parameter `R`.
    pub(crate) fn super_prefix_type(&self, this_id: SymbolId, parent: SymbolId) -> Type {
        let self_ty = self.st.self_type_of_class(this_id);
        self.st
            .base_type_seq(&self_ty)
            .into_iter()
            .find(|t| matches!(t, Type::Class { sym, args } if *sym == parent && !args.is_empty()))
            .unwrap_or_else(|| self.st.type_of_class(parent))
    }

    pub(crate) fn super_target(&self, this_id: SymbolId, mix: Option<&str>) -> SymbolId {
        if this_id.is_none() {
            return SymbolId::NONE;
        }
        let parents: Vec<SymbolId> = self
            .st
            .get(this_id)
            .parents
            .iter()
            .filter_map(|p| self.st.class_sym_of(p))
            .filter(|p| {
                let n = self.st.get(*p).name.as_str();
                n != "AnyRef" && n != "Any" && n != "AnyVal" && n != "Object"
            })
            // `Product` and `Serializable` are never what `super` means. nsc
            // writes every case class as `C extends Base with Product with
            // Serializable`, so they *are* the last parents, but `super.m`
            // walks the linearization for something that defines `m` and
            // neither of these defines anything a subclass overrides. Taking
            // the list's last entry literally made `override def getDumpInfo =
            // super.getDumpInfo…` in slick's case classes report
            // `value getDumpInfo is not a member of Serializable` -- 30 times.
            // The same two names are filtered whether the compiler put them
            // there (`Typer::link_case_product`) or the user wrote them: a
            // `super` call to either was already a dead end.
            .filter(|p| {
                let jvm = self.st.get(*p).jvm_name.as_str();
                jvm != "scala/Product" && jvm != "java/io/Serializable"
            })
            .collect();
        if let Some(name) = mix {
            parents
                .iter()
                .copied()
                .find(|p| {
                    let n = self.st.get(*p).name.as_str();
                    n == name || n.trim_end_matches('$') == name
                })
                .unwrap_or(SymbolId::NONE)
        } else {
            parents.last().copied().unwrap_or(SymbolId::NONE)
        }
    }

    /// `super.m`'s real target member, found by walking `this_id`'s actual
    /// mixin parents (never a `self:` annotation -- see
    /// `SymbolTable::lookup_member_real`) in linearization order.
    ///
    /// `super_target` above picks one parent (the syntactically last one) up
    /// front, independent of which member will be selected, and
    /// `lookup_member` (used for an ordinary selection on that parent's type)
    /// also walks the parent's own self-type -- which found
    /// `RelationalActionComponent { self: RelationalProfile => }`'s self-type
    /// member for `super.computeCapabilities` inside `RelationalProfile`
    /// itself, i.e. the very override being completed. This walks every real
    /// parent, last-declared first (later mixins are more specific in Scala's
    /// linearization, so `super` prefers them), and returns the first parent
    /// whose *real* inheritance chain actually defines `name` -- `Relational
    /// ActionComponent` has no `computeCapabilities` of its own, so the
    /// search continues past it to `BasicProfile`, which does.
    pub(crate) fn super_select_member(
        &self,
        this_id: SymbolId,
        mix: Option<&str>,
        name: &str,
    ) -> Option<(SymbolId, Vec<SymbolId>)> {
        if this_id.is_none() {
            return None;
        }
        let mut parents: Vec<SymbolId> = self
            .st
            .get(this_id)
            .parents
            .iter()
            .filter_map(|p| self.st.class_sym_of(p))
            .filter(|p| {
                let n = self.st.get(*p).name.as_str();
                n != "AnyRef" && n != "Any" && n != "AnyVal" && n != "Object"
            })
            .filter(|p| {
                let jvm = self.st.get(*p).jvm_name.as_str();
                jvm != "scala/Product" && jvm != "java/io/Serializable"
            })
            .collect();
        if let Some(mix_name) = mix {
            parents.retain(|p| {
                let n = self.st.get(*p).name.as_str();
                n == mix_name || n.trim_end_matches('$') == mix_name
            });
        } else {
            parents.reverse();
        }
        // nsc resolves `super.m` to the first *concrete* `m` along the
        // linearization: a mixin that only re-declares `m` (slick's
        // `BasicStreamingQueryActionExtensionMethodsImpl` narrows `result`
        // covariantly and leaves it abstract) is not what `super.result`
        // means, and calling it emitted an `invokestatic` on a `$class`
        // holder that has no such method -- indeed no such class file, since
        // the trait has no concrete member at all (`NoClassDefFoundError`).
        let mut deferred: Option<(SymbolId, Vec<SymbolId>)> = None;
        for p in parents {
            let members = self.st.lookup_member_real(p, name);
            if members.is_empty() {
                continue;
            }
            if members.iter().any(|m| !self.is_deferred_member(*m)) {
                return Some((p, members));
            }
            if deferred.is_none() {
                deferred = Some((p, members));
            }
        }
        deferred
    }

    /// A member with no implementation: a body-less `def` (the namer sets
    /// `ABSTRACT` on those) or a body-less `val` / `var`.
    pub(crate) fn is_deferred_member(&self, m: SymbolId) -> bool {
        let s = self.st.get(m);
        match s.kind {
            SymKind::Method => s.flags.contains(Flags::ABSTRACT),
            SymKind::Term => s.deferred_val,
            _ => false,
        }
    }

    /// The instance of `target` among `ty`'s base classes: `Some[Int]` seen as
    /// `Option` is `Option[Int]`. `None` when `target` is not a base class.
    pub(crate) fn base_type_instance(
        &self,
        ty: &Type,
        target: SymbolId,
        depth: u32,
    ) -> Option<Type> {
        if depth > 16 {
            return None;
        }
        // nsc's `Types.baseType` reads a singleton type through the type it
        // widens to: `OD.type` *is* a `D[Int]` when the module class extends
        // `D[Int]`, so an argument written as a bare `object` name still
        // pins the callee's type parameters.
        let (sym, args): (SymbolId, &[Type]) = match ty {
            Type::Class { sym, args } => (*sym, args),
            Type::ModuleRef(s) | Type::ThisType(s) => (*s, &[]),
            Type::SingleType { prefix, sym } => {
                let under = &self.st.get(*sym).ty;
                let under = if under.is_no_type() { prefix } else { under };
                return self.base_type_instance(under, target, depth + 1);
            }
            Type::Annotated { tpe, .. } => {
                return self.base_type_instance(tpe, target, depth + 1);
            }
            _ => return None,
        };
        if sym == target {
            return Some(ty.clone());
        }
        // The walk below has no visited set (a class legitimately appears
        // twice at different arguments), so a diamond is re-entered once per
        // path and a miss costs the whole DAG. Asking first whether the target
        // symbol is up there at all is linear and needs no arguments. It
        // answers `Some(false)` only when every parent in the closure is an
        // ordinary class or `AnyRef`/`Any`/`AnyVal`, which is exactly the case
        // this walk would grind through and return `None` for.
        if self.st.class_reaches(sym, target) == Some(false) {
            return None;
        }
        // Everything here is borrowed. This walks the whole parent DAG on every
        // call, so the two `Vec<Type>` clones it used to make (the arguments and
        // the parent list) were among the typer's largest sources of allocation.
        for p in &self.st.get(sym).parents {
            let p = self.st.subst_tparams_cow(sym, args, p);
            if let Some(found) = self.base_type_instance(&p, target, depth + 1) {
                return Some(found);
            }
        }
        None
    }

    /// Type arguments for a constructor pattern's class, read off the scrutinee:
    /// matching `Option[Int]` with `case Some(v)` binds `v: Int`, because
    /// `Some[A] <: Option[A]` forces `A = Int`. Empty when nothing constrains
    /// them, which leaves the declared (unsubstituted) field types in place.
    pub(crate) fn pattern_class_targs(&self, cls: SymbolId, sel_ty: &Type) -> Vec<Type> {
        let tps = self.st.get(cls).tparams.clone();
        if tps.is_empty() {
            return Vec::new();
        }
        let (sel_sym, sel_args) = match sel_ty {
            Type::Class { sym, args } => (*sym, args.clone()),
            _ => return Vec::new(),
        };
        if sel_args.is_empty() {
            return Vec::new();
        }
        if sel_sym == cls {
            return if sel_args.len() == tps.len() {
                sel_args
            } else {
                Vec::new()
            };
        }
        let open = Type::Class {
            sym: cls,
            args: tps.iter().map(|t| Type::TypeParam(*t)).collect(),
        };
        let Some(base) = self.base_type_instance(&open, sel_sym, 0) else {
            return Vec::new();
        };
        let Type::Class {
            args: base_args, ..
        } = &base
        else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(tps.len());
        for tp in &tps {
            match unify_tparam(*tp, base_args, &sel_args) {
                Some(t) if !t.is_no_type() => out.push(t),
                _ => return Vec::new(),
            }
        }
        out
    }

    /// The class a collection factory's `apply` really builds.
    ///
    /// The shortcuts below recompute a factory result's type arguments (the
    /// lub of the elements, or the pair a `Tuple2` argument carries). They
    /// used to recover the *class* by looking the companion's simple name up
    /// in the call site's scope, which is only right for the factories
    /// `Predef` exports: `scala.collection.mutable.Set(1, 2)` selects
    /// `mutable.Set$.apply`, and `lookup("Set")` then answered
    /// `scala.collection.immutable.Set`. The call was inferred to build an
    /// *immutable* set, so `+=`, `-=` and `add` were "not a member" of it and
    /// codegen was free to store one into the other. The result type the
    /// signature already carries names the right class; the shortcut only
    /// ever means to replace its arguments, so keep its symbol whenever it is
    /// the companion's own class, and fall back to the scope only when the
    /// declaration gives nothing usable.
    pub(crate) fn factory_result_class(
        &self,
        ret: &Type,
        simple: &str,
        arity: usize,
    ) -> Option<SymbolId> {
        if let Type::Class { sym, args } = ret {
            if args.len() == arity && self.st.get(*sym).name == simple {
                return Some(*sym);
            }
        }
        self.st
            .lookup(simple)
            .into_iter()
            .find(|id| self.st.get(*id).kind == crate::symbol::SymKind::Class)
    }

    /// A collection factory's element types, widened by the expected type.
    /// `Set(s)` on an `AnonSym` is a `Set[AnonSym]` from its arguments alone,
    /// but `def f(s: AnonSym): Set[Sym] = Set(s)` is a `Set[Sym]` -- and `Set`
    /// is invariant, so the difference is an error rather than a subtype.
    /// nsc reaches this through ordinary inference; the factory shortcuts here
    /// bypass it, so they have to ask. `None` when the expected type is not
    /// this very class, or does not admit what the arguments gave.
    pub(crate) fn factory_targs_from_pt(
        &self,
        cls: SymbolId,
        from_args: &[Type],
        pt: &Type,
    ) -> Option<Vec<Type>> {
        let Type::Class { sym, args } = pt else {
            return None;
        };
        if *sym != cls || args.len() != from_args.len() || args.is_empty() {
            return None;
        }
        let tps = self.st.get(cls).tparams.clone();
        if !type_args_are_instantiated(args, &tps) {
            return None;
        }
        from_args
            .iter()
            .zip(args)
            .all(|(a, p)| self.st.is_sub_type(a, p))
            .then(|| args.clone())
    }

    /// `pattern_class_targs` one parameter at a time: what the expected type
    /// says about each of `cls`'s type parameters, `None` where it says
    /// nothing. `new TmRC(c, f)` checked against `RC[R, String]` learns `R`
    /// and `V` this way and leaves `U` to the constructor arguments.
    pub(crate) fn base_targs_from_pt(&self, cls: SymbolId, pt: &Type) -> Vec<Option<Type>> {
        let tps = self.st.get(cls).tparams.clone();
        let (pt_sym, pt_args) = match pt {
            Type::Class { sym, args } if !args.is_empty() => (*sym, args.clone()),
            _ => return vec![None; tps.len()],
        };
        if tps.is_empty() || pt_sym == cls {
            return vec![None; tps.len()];
        }
        let open = Type::Class {
            sym: cls,
            args: tps.iter().map(|t| Type::TypeParam(*t)).collect(),
        };
        let Some(Type::Class {
            args: base_args, ..
        }) = self.base_type_instance(&open, pt_sym, 0)
        else {
            return vec![None; tps.len()];
        };
        tps.iter()
            .map(|tp| {
                unify_tparam(*tp, &base_args, &pt_args)
                    .filter(|t| !t.is_no_type() && !t.is_error() && !mentions_no_type(t))
            })
            .collect()
    }

    fn find_unapply(&self, fun: &Tree, sel_ty: &Type) -> Option<SymbolId> {
        let owner = if !fun.sym.is_none() {
            let s = self.st.get(fun.sym);
            match s.kind {
                SymKind::Module | SymKind::ModuleClass => self.st.module_class_of(fun.sym),
                SymKind::Class => self
                    .st
                    .companion_module(fun.sym)
                    .map(|m| self.st.module_class_of(m))
                    .unwrap_or(SymbolId::NONE),
                // `val SilentCast = new FunctionSymbol(…)` is an extractor
                // too: the `unapply` lives on the value's own type.
                _ => self.st.class_sym_of(&fun.ty).unwrap_or(SymbolId::NONE),
            }
        } else if let Some(n) = fun.name() {
            let found = self.st.lookup(n);
            found
                .into_iter()
                .find(|s| matches!(self.st.get(*s).kind, SymKind::Module | SymKind::ModuleClass))
                .map(|m| self.st.module_class_of(m))
                .unwrap_or(SymbolId::NONE)
        } else {
            self.st.class_sym_of(&fun.ty).unwrap_or(SymbolId::NONE)
        };
        if owner.is_none() {
            return None;
        }
        let alts: Vec<SymbolId> = self
            .st
            .lookup_member(owner, "unapply")
            .into_iter()
            .filter(|m| self.st.get(*m).kind == SymKind::Method)
            .collect();
        // An extractor object may be overloaded: `scala.#::` declares one
        // `unapply` for `LazyList` and one for `Stream`, and they compile to
        // different descriptors, so taking whichever came first bound
        // `case h #:: t` on a `Stream` at `LazyList`. The scrutinee's own class
        // decides; conformance is only the tie-break, because a class the typer
        // knows only from a classfile has no parents to rule the other
        // alternative out with.
        let param = |m: SymbolId| match &self.st.get(m).ty {
            Type::Method { paramss, .. } => paramss.first().and_then(|ps| ps.first()).cloned(),
            Type::Function { params, .. } => params.first().cloned(),
            _ => None,
        };
        let sel_cls = self.st.class_sym_of(sel_ty);
        let fits = |m: SymbolId| {
            (sel_cls.is_some() && param(m).and_then(|p| self.st.class_sym_of(&p)) == sel_cls)
                || param(m).is_some_and(|p| self.st.is_sub_type(sel_ty, &p))
        };
        if alts.len() > 1 {
            if let Some(m) = alts
                .iter()
                .copied()
                .find(|&m| {
                    sel_cls.is_some() && param(m).and_then(|p| self.st.class_sym_of(&p)) == sel_cls
                })
                .or_else(|| {
                    alts.iter()
                        .copied()
                        .find(|&m| param(m).is_some_and(|p| self.st.is_sub_type(sel_ty, &p)))
                })
            {
                return Some(m);
            }
        }
        // `scala.#::` matches a `LazyList` or a `Stream` and nothing else -- nsc
        // says "cannot resolve overloaded unapply" for `case h #:: t` on a
        // `List`. Falling through to the first alternative would have bound the
        // tail at the wrong class and emitted a call the JVM rejects, so the
        // pattern is reported as not found instead. Only this object: for
        // everything else the scrutinee is checked where it always was.
        if !alts.is_empty()
            && self.st.get(owner).jvm_name == crate::prelude_consextract::HASH_COLON_COLON_MODULE
            && !alts.iter().copied().any(fits)
        {
            return None;
        }
        alts.into_iter().next()
    }

    fn find_unapply_seq(&self, fun: &Tree) -> Option<SymbolId> {
        let owner = if !fun.sym.is_none() {
            let s = self.st.get(fun.sym);
            match s.kind {
                SymKind::Module | SymKind::ModuleClass => self.st.module_class_of(fun.sym),
                SymKind::Class => self
                    .st
                    .companion_module(fun.sym)
                    .map(|m| self.st.module_class_of(m))
                    .unwrap_or(SymbolId::NONE),
                // `val SilentCast = new FunctionSymbol(…)` is an extractor
                // too: the `unapply` lives on the value's own type.
                _ => self.st.class_sym_of(&fun.ty).unwrap_or(SymbolId::NONE),
            }
        } else if let Some(n) = fun.name() {
            let found = self.st.lookup(n);
            found
                .into_iter()
                .find(|s| matches!(self.st.get(*s).kind, SymKind::Module | SymKind::ModuleClass))
                .map(|m| self.st.module_class_of(m))
                .unwrap_or(SymbolId::NONE)
        } else {
            self.st.class_sym_of(&fun.ty).unwrap_or(SymbolId::NONE)
        };
        if owner.is_none() {
            return None;
        }
        self.st
            .lookup_member(owner, "unapplySeq")
            .into_iter()
            .find(|m| self.st.get(*m).kind == SymKind::Method)
    }

    /// The type `rest @ _*` binds: the extractor's own result container
    /// re-applied to the element type the scrutinee gave us.
    ///
    /// `List.unapplySeq: Option[List[A]]` keeps `rest: List[A]`, which is what
    /// every existing fixture and the private runtime's `List` codegen expect.
    /// The `Seq` / `Vector` / `IndexedSeq` / `Array` factories declare
    /// `Option[Seq[A]]`, matching scalac, whose
    /// `UnapplySeqWrapper.drop$extension` returns `immutable.Seq`.
    fn unapply_seq_star_type(&self, unapply: SymbolId, elem: &Type) -> Type {
        let fallback = Type::Class {
            sym: self.st.list_sym,
            args: vec![elem.clone()],
        };
        match self.unapply_extracted_types(unapply).into_iter().next() {
            Some(Type::Class { sym, args }) if args.len() == 1 => Type::Class {
                sym,
                args: vec![elem.clone()],
            },
            _ => fallback,
        }
    }

    /// A sequence pattern on `Seq` / `Vector` / `IndexedSeq` / `Array` compiles
    /// to `scala.collection.SeqFactory$UnapplySeqWrapper$` (respectively
    /// `scala.Array$UnapplySeqWrapper$`) extension calls. The private runtime
    /// (`--no-scala-library`) emits neither, and `scala/collection/SeqOps`
    /// does not exist there at all, so say so instead of emitting code that
    /// cannot link.
    fn check_seq_pattern_backing(&mut self, unapply: SymbolId, span: Span) {
        if self.library_abi {
            return;
        }
        let owner = self.st.get(unapply).owner;
        let jvm = self.st.get(owner).jvm_name.clone();
        let known = jvm == crate::prelude_seqpat::ARRAY_FACTORY_MODULE
            || crate::prelude_seqpat::SEQ_FACTORY_MODULES.contains(&jvm.as_str());
        if !known {
            return;
        }
        let name = jvm.rsplit('/').next().unwrap_or(&jvm).trim_end_matches('$');
        self.error(
            span,
            format!(
                "sequence pattern on `{name}` needs the real scala-library \
                 (`--scala-library`); the private runtime has no \
                 `scala.collection.SeqOps`"
            ),
        );
    }

    /// `case Array(a, b)` with `--no-scala-library`: the class is in scope but
    /// its companion (and therefore `unapplySeq`) is not.
    fn check_missing_seq_factory(&mut self, cls: SymbolId, span: Span) {
        if self.library_abi {
            return;
        }
        let jvm = self.st.get(cls).jvm_name.clone();
        let is_seq_class = cls == self.st.array_sym
            || matches!(
                jvm.as_str(),
                "scala/collection/immutable/Seq"
                    | "scala/collection/immutable/Vector"
                    | "scala/collection/immutable/IndexedSeq"
            );
        if !is_seq_class {
            return;
        }
        let name = self.st.get(cls).name.clone();
        self.error(
            span,
            format!(
                "sequence pattern on `{name}` needs the real scala-library \
                 (`--scala-library`); the private runtime has no \
                 `{name}` companion"
            ),
        );
    }

    /// Record which container this extractor's `Option` holds, for the
    /// backend. See `SymbolTable::seq_extractor_payload`: erasure drops the
    /// type argument, and a non-`List` payload has to be read through
    /// scalac's `UnapplySeqWrapper` instead of a head/tail walk.
    fn note_seq_extractor_payload(&mut self, unapply: SymbolId) {
        let owner = self.st.get(unapply).owner;
        let jvm = self.st.get(owner).jvm_name.clone();
        // The built-in factories return the sequence itself, not an `Option`;
        // the backend recognises them by companion and never asks here.
        if jvm == crate::prelude_seqpat::ARRAY_FACTORY_MODULE
            || crate::prelude_seqpat::SEQ_FACTORY_MODULES.contains(&jvm.as_str())
        {
            return;
        }
        let Some(inner) = self.unapply_extracted_types(unapply).into_iter().next() else {
            return;
        };
        let payload = match &inner {
            Type::Array(_) => crate::symbol::SeqPayload::Array,
            Type::Class { sym, .. } if self.is_non_list_seq(*sym) => crate::symbol::SeqPayload::Seq,
            _ => return,
        };
        self.st.seq_extractor_payload.insert(unapply, payload);
    }

    /// A sequence class that is not `List`: the head/tail walk would
    /// `checkcast` an `ArraySeq` or a `Vector` to `List` and throw.
    fn is_non_list_seq(&self, cls: SymbolId) -> bool {
        if cls == self.st.list_sym || self.st.get(cls).name == "List" {
            return false;
        }
        crate::lin::linearize(&self.st, cls).into_iter().any(|b| {
            matches!(
                self.st.get(b).jvm_name.as_str(),
                "scala/collection/SeqOps" | "scala/collection/Seq"
            )
        })
    }

    fn unapply_seq_elem_type(&self, unapply: SymbolId) -> Type {
        let extracted = self.unapply_extracted_types(unapply);
        let inner = extracted.into_iter().next().unwrap_or(Type::Any);
        match inner {
            Type::Class { sym, args }
                if sym == self.st.list_sym || self.st.get(sym).name == "List" =>
            {
                args.first().cloned().unwrap_or(Type::Any)
            }
            Type::Class { args, .. } if !args.is_empty() => args[0].clone(),
            other => other,
        }
    }

    fn reorder_named_pattern_args(&mut self, args: &mut Vec<Tree>, class_id: SymbolId) {
        if !args
            .iter()
            .any(|a| matches!(a.kind, TreeKind::Assign { .. }))
        {
            return;
        }
        let fields = self.st.get(class_id).ctor_fields.clone();
        if fields.is_empty() {
            self.error(
                args.first().map(|a| a.span).unwrap_or(Span::DUMMY),
                "named extractor arguments require constructor parameter names",
            );
            return;
        }
        let names: Vec<String> = fields
            .iter()
            .map(|f| self.st.get(*f).name.clone())
            .collect();
        let mut slots: Vec<Option<Tree>> = names.iter().map(|_| None).collect();
        let taken = std::mem::take(args);
        for a in taken {
            if let TreeKind::Assign { lhs, rhs } = &a.kind {
                if let TreeKind::Ident { name } = &lhs.kind {
                    match names.iter().position(|p| p == name) {
                        Some(i) => {
                            if slots[i].is_some() {
                                self.error(
                                    a.span,
                                    format!("parameter '{name}' is already specified"),
                                );
                            }
                            slots[i] = Some((**rhs).clone());
                        }
                        None => self.error(a.span, format!("unknown parameter name: {name}")),
                    }
                    continue;
                }
            }
            self.error(
                a.span,
                "positional and named extractor arguments cannot be mixed",
            );
        }
        *args = slots
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                s.unwrap_or_else(|| {
                    self.error(
                        Span::DUMMY,
                        format!("missing extractor argument `{}`", names[i]),
                    );
                    Tree::dummy(TreeKind::Wildcard)
                })
            })
            .collect();
    }

    /// `Some.unapply[A](x: Option[A]): Option[A]` extracts `A`; the scrutinee
    /// says what `A` is. Without this the bound variable keeps the extractor's
    /// own type parameter and degrades to `Any`.
    fn subst_unapply_tparams(&self, unapply: SymbolId, sel_ty: &Type, out: Vec<Type>) -> Vec<Type> {
        let tps = self.st.get(unapply).tparams.clone();
        if tps.is_empty() || sel_ty.is_no_type() {
            return out;
        }
        let param = match &self.st.get(unapply).ty {
            Type::Method { paramss, .. } => paramss.first().and_then(|p| p.first()).cloned(),
            Type::Function { params, .. } => params.first().cloned(),
            _ => None,
        };
        let Some(param) = param else {
            return out;
        };
        let (param, sel) = self.align_for_unify(&param, sel_ty);
        let params = [param];
        let args = [sel];
        let mut ids = Vec::new();
        let mut tys = Vec::new();
        for tp in &tps {
            if let Some(t) = unify_tparam(*tp, &params, &args) {
                if !t.is_no_type() && !t.is_error() {
                    ids.push(*tp);
                    tys.push(t);
                }
            }
        }
        if ids.is_empty() {
            return out;
        }
        // A type parameter the *parameter type* does not mention can still be
        // determined -- through the bounds of the ones that are. `+:.unapply[A,
        // C <: Seq[A]](t: C)` names only `C` in its parameter, and `A` is what
        // the head sub-pattern binds: matching an `ArraySeq[Int]` gives `C =
        // ArraySeq[Int]`, and `C`'s bound `Seq[A]` against that same scrutinee
        // (walked to `Seq`) gives `A = Int`. nsc solves the whole constraint
        // set at once; this is the one shape a library extractor reaches us
        // with, and without it every `case h +: t` bound `h` as an unresolved
        // `A`.
        if ids.len() < tps.len() {
            for i in 0..ids.len() {
                let Some(hi) = self.st.get(ids[i]).bound_hi.clone() else {
                    continue;
                };
                let (hi, inst) = self.align_for_unify(&hi, &tys[i]);
                let hi = [hi];
                let inst = [inst];
                for tp in &tps {
                    if ids.contains(tp) {
                        continue;
                    }
                    if let Some(t) = unify_tparam(*tp, &hi, &inst) {
                        if !t.is_no_type() && !t.is_error() {
                            ids.push(*tp);
                            tys.push(t);
                        }
                    }
                }
            }
        }
        out.iter()
            .map(|t| crate::symbol::subst_tparams_slice(&ids, &tys, t))
            .collect()
    }

    /// Line an `unapply`'s parameter type up with the scrutinee before reading
    /// the extractor's type parameters off it.
    ///
    /// [`unify_one`] pairs two class applications by position, which is right
    /// only when they are applications of the *same* class. A case class whose
    /// parent reorders or drops type parameters breaks that: cats'
    ///
    /// ```scala
    /// final case class Right[+B](b: B) extends (Nothing Ior B)
    /// ```
    ///
    /// synthesizes `unapply[B](x: Right[B])`, and matching it against a
    /// scrutinee `Ior[A, B]` paired the extractor's `B` with the scrutinee's
    /// *first* argument. `case Ior.Right(b)` therefore bound `b: A`, and every
    /// `IorT` method that matches on its own value reported `type mismatch;
    /// found: A  required: B`.
    ///
    /// Walking one side to the other's class first is what nsc does -- the
    /// extractor is run against the scrutinee's base type at the extractor's
    /// class. Either direction may be the one that exists (`Some.unapply[A](x:
    /// Option[A])` against a `Some[Int]` scrutinee walks the *scrutinee*),
    /// so both are tried; when neither class is the other's base the pair is
    /// left alone.
    fn align_for_unify(&self, param: &Type, sel_ty: &Type) -> (Type, Type) {
        // A parameter that *is* a type parameter takes the scrutinee whole.
        // `class_sym_of` answers with the bound's class for one of those, and
        // walking the scrutinee up to it threw away exactly what the extractor
        // is there to keep: `+:.unapply[A, C <: Seq[A]](t: C)` on a
        // `Vector[Int]` would have bound `C = Seq[Int]`.
        if matches!(param, Type::TypeParam(_)) {
            return (param.clone(), sel_ty.clone());
        }
        let (Some(psym), Some(ssym)) = (self.st.class_sym_of(param), self.st.class_sym_of(sel_ty))
        else {
            return (param.clone(), sel_ty.clone());
        };
        if psym == ssym {
            return (param.clone(), sel_ty.clone());
        }
        if let Some(base) = self.base_type_instance(sel_ty, psym, 0) {
            return (param.clone(), base);
        }
        if let Some(base) = self.base_type_instance(param, ssym, 0) {
            return (base, sel_ty.clone());
        }
        (param.clone(), sel_ty.clone())
    }

    /// The type an `x @ Extractor(...)` pattern narrows `x` to: the
    /// `unapply`'s own declared parameter type (substituting its type
    /// parameters, unified against the scrutinee, exactly as
    /// `subst_unapply_tparams` does for the extracted sub-patterns). nsc
    /// performs the same implicit type test a `case x: T` pattern does, so
    /// `case c @ LiteralNode(_) if c.volatileHint` sees `c: LiteralNode`
    /// (which declares `volatileHint`), not the scrutinee's static `Node`.
    fn unapply_receiver_type(&self, unapply: SymbolId, sel_ty: &Type) -> Option<Type> {
        let param = match &self.st.get(unapply).ty {
            Type::Method { paramss, .. } => paramss.first().and_then(|p| p.first()).cloned(),
            Type::Function { params, .. } => params.first().cloned(),
            _ => None,
        }?;
        let tps = self.st.get(unapply).tparams.clone();
        if tps.is_empty() || sel_ty.is_no_type() {
            return Some(param);
        }
        let (aligned, sel) = self.align_for_unify(&param, sel_ty);
        let params = [aligned];
        let args = [sel];
        let mut ids = Vec::new();
        let mut tys = Vec::new();
        for tp in tps {
            if let Some(t) = unify_tparam(tp, &params, &args) {
                if !t.is_no_type() && !t.is_error() {
                    ids.push(tp);
                    tys.push(t);
                }
            }
        }
        if ids.is_empty() {
            return Some(param);
        }
        Some(crate::symbol::subst_tparams_slice(&ids, &tys, &param))
    }

    fn unapply_extracted_types(&self, unapply: SymbolId) -> Vec<Type> {
        let ret = match &self.st.get(unapply).ty {
            Type::Method { ret, .. } | Type::Function { ret, .. } => (**ret).clone(),
            t => t.clone(),
        };
        if matches!(ret, Type::Boolean) {
            return vec![];
        }
        if let Type::Class { sym, args } = &ret {
            let name = self.st.get(*sym).name.as_str();
            if *sym == self.st.option_sym || name == "Option" || name == "Some" {
                if args.is_empty() {
                    // A bare `Option` says nothing about what it yields: that
                    // is what an `unapply` read back from a *classfile* looks
                    // like when its signature was erased. For a case class's
                    // own synthetic extractor the constructor fields are the
                    // answer -- and without them `case LK(u, n)` on a
                    // separately compiled `case class LK(k: Unit, n: Int)`
                    // counted one sub-pattern and reported "extractor LK
                    // expects 1 argument(s), found 2".
                    if let Some(fields) = self.case_ctor_field_types(self.st.get(unapply).owner) {
                        return fields;
                    }
                }
                let inner = args.first().cloned().unwrap_or(Type::Any);
                return self.flatten_extract(inner);
            }
        }
        self.flatten_extract(ret)
    }

    /// The constructor field types of the class whose companion module class
    /// is `module_cls`, when that pair has a case class's shape.
    ///
    /// The `CASE` flag itself cannot be used: our pickle *writes* it, but the
    /// reader never sets it, so a case class read back through `-cp` looks
    /// like a plain one. The shape test is that the companion also carries an
    /// `apply` of exactly the constructor's arity, which is what
    /// `synthesize_case_members` gives every case class and what a hand-written
    /// companion of a non-case class rarely matches.
    fn case_ctor_field_types(&self, module_cls: SymbolId) -> Option<Vec<Type>> {
        if module_cls.is_none() {
            return None;
        }
        let s = self.st.get(module_cls);
        if !matches!(s.kind, SymKind::ModuleClass | SymKind::Module) {
            return None;
        }
        let base = s.name.strip_suffix('$').unwrap_or(&s.name).to_string();
        let owner = s.owner;
        let cls = self
            .st
            .get(owner)
            .members
            .iter()
            .copied()
            .find(|&m| self.st.get(m).kind == SymKind::Class && self.st.get(m).name == base)?;
        let fields = self.st.get(cls).ctor_fields.clone();
        if fields.is_empty() {
            return None;
        }
        let has_matching_apply = self.st.get(module_cls).members.iter().any(|&m| {
            self.st.get(m).name == "apply"
                && match &self.st.get(m).ty {
                    Type::Method { paramss, .. } => {
                        paramss.iter().map(|c| c.len()).sum::<usize>() == fields.len()
                    }
                    _ => false,
                }
        });
        if !has_matching_apply {
            return None;
        }
        Some(fields.iter().map(|f| self.st.get(*f).ty.clone()).collect())
    }

    fn flatten_extract(&self, inner: Type) -> Vec<Type> {
        match inner {
            Type::Tuple(ts) => ts,
            Type::Class { sym, args } => {
                let n = self.st.get(sym).name.as_str();
                if numbered_arity(n, "Tuple").is_some_and(|k| args.is_empty() || k == args.len()) {
                    if args.is_empty() {
                        vec![Type::Any, Type::Any]
                    } else {
                        args
                    }
                } else {
                    vec![Type::Class { sym, args }]
                }
            }
            other => vec![other],
        }
    }

    fn check_match_exhaustive(&mut self, span: Span, sel_ty: &Type, cases: &[CaseDef]) {
        let Some(cls) = self.st.class_sym_of(sel_ty) else {
            return;
        };
        if !self.st.is_sealed(cls) {
            return;
        }
        if cases
            .iter()
            .any(|c| c.guard.is_empty() && self.pattern_is_catchall(&c.pat))
        {
            return;
        }
        let leaves = self.st.sealed_leaves(cls);
        if leaves.is_empty() {
            return;
        }
        let mut missing = Vec::new();
        for leaf in &leaves {
            if !cases
                .iter()
                .any(|c| c.guard.is_empty() && self.pattern_covers(&c.pat, *leaf))
            {
                missing.push(self.st.get(*leaf).name.trim_end_matches('$').to_string());
            }
        }
        if !missing.is_empty() {
            self.warning(
                span,
                format!(
                    "match may not be exhaustive. It would fail on the following input: {}",
                    missing.join(", ")
                ),
            );
        }
    }

    fn pattern_is_catchall(&self, pat: &Tree) -> bool {
        match &pat.kind {
            TreeKind::Wildcard | TreeKind::Empty => true,
            TreeKind::Bind { body, .. } => self.pattern_is_catchall(body),
            TreeKind::Ident { name } => {
                let is_varid = name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_lowercase() || c == '_');
                is_varid && (pat.sym.is_none() || self.st.get(pat.sym).kind == SymKind::Term)
            }
            TreeKind::Typed { expr, .. } => self.pattern_is_catchall(expr),
            _ => false,
        }
    }

    fn pattern_covers(&self, pat: &Tree, leaf: SymbolId) -> bool {
        if self.pattern_is_catchall(pat) {
            return true;
        }
        match &pat.kind {
            TreeKind::Typed { .. } => {
                if let Some(ps) = self.st.class_sym_of(&pat.ty) {
                    ps == leaf || self.st.is_sub_type(&self.st.type_of_class(leaf), &pat.ty)
                } else {
                    false
                }
            }
            TreeKind::Ident { .. } => {
                if pat.sym.is_none() {
                    return false;
                }
                let s = self.st.get(pat.sym);
                match s.kind {
                    SymKind::Module | SymKind::ModuleClass => {
                        self.st.module_class_of(pat.sym) == leaf
                    }
                    SymKind::Class => pat.sym == leaf,
                    _ => false,
                }
            }
            TreeKind::Apply { .. } | TreeKind::UnApply { .. } => {
                if pat.sym.is_none() {
                    return false;
                }
                let s = self.st.get(pat.sym);
                if s.kind == SymKind::Class {
                    pat.sym == leaf
                } else if s.name == "unapply" {
                    let owner = s.owner;
                    let name = self.st.get(owner).name.trim_end_matches('$').to_string();
                    self.st.get(leaf).name.trim_end_matches('$') == name
                } else {
                    false
                }
            }
            TreeKind::Bind { body, .. } => self.pattern_covers(body, leaf),
            TreeKind::Alternative { trees } => trees.iter().any(|t| self.pattern_covers(t, leaf)),
            _ => false,
        }
    }
}
