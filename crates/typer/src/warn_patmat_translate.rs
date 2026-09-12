//! nsc's `MatchTranslation`: a case's pattern, guard and body as a list of
//! `TreeMaker`s over binders (see `warn_patmat`).

use crate::symbol::SymKind;
use crate::warn_patmat::*;
use crate::warn_patmat_types::{CVal, NTy, Types};
use crate::warn_util::point_of;
use scala_rs_parser::ast::*;
use std::collections::HashMap;

/// A pattern shape the port does not model: the match is not analysed.
pub(crate) struct Unsupported;

/// `Unsupported`, noting where it came from when `SCALA_RS_PATMAT_DEBUG`
/// is set (which of the modelling gaps a match fell into).
macro_rules! bail {
    () => {{
        if std::env::var_os("SCALA_RS_PATMAT_DEBUG").is_some() {
            eprintln!("patmat: not analysed ({}:{})", file!(), line!());
        }
        $crate::warn_patmat_translate::Unsupported
    }};
}
pub(crate) use bail;

pub(crate) struct Translator<'t> {
    pub(crate) tys: Types<'t>,
    pub(crate) binders: Vec<Binder>,
    /// Pattern-variable symbols to their binders.
    sym_binders: HashMap<SymbolId, B>,
    fresh: &'t mut u32,
    src: &'t str,
}

impl<'t> Translator<'t> {
    pub(crate) fn new(tys: Types<'t>, fresh: &'t mut u32, src: &'t str) -> Self {
        Translator {
            tys,
            binders: Vec::new(),
            sym_binders: HashMap::new(),
            fresh,
            src,
        }
    }

    pub(crate) fn binder(&mut self, tp: NTy) -> B {
        self.binders.push(Binder { tp });
        self.binders.len() - 1
    }

    /// The binder of a pattern variable, given the type the extractor
    /// assigns it (`setVarInfo`).
    fn sym_binder(&mut self, sym: SymbolId, tp: NTy) -> B {
        if let Some(b) = self.sym_binders.get(&sym) {
            self.binders[*b].tp = tp;
            return *b;
        }
        let b = self.binder(tp);
        self.sym_binders.insert(sym, b);
        b
    }

    /// The binder of a pattern variable, keeping the type an enclosing
    /// extractor gave it (`case Some(x: Int)` binds `x` at `Option`'s `A`
    /// and then tests it).
    fn sym_binder_keep(&mut self, sym: SymbolId, tp: NTy) -> B {
        if let Some(b) = self.sym_binders.get(&sym) {
            return *b;
        }
        self.sym_binder(sym, tp)
    }

    fn ty(&self, t: &Type) -> NTy {
        self.tys.of(t)
    }

    /// A pattern variable (nsc's `Bind(x, _)`) or `_`.
    pub(crate) fn is_var_pattern(p: &Tree) -> bool {
        match &p.kind {
            TreeKind::Ident { name } => {
                name == "_" || (!p.stable_pat && scala_rs_parser::ast::is_variable_name(name))
            }
            _ => false,
        }
    }

    /// nsc's `WildcardPattern`.
    pub(crate) fn is_wildcard(p: &Tree) -> bool {
        match &p.kind {
            TreeKind::Wildcard | TreeKind::Empty => true,
            TreeKind::Ident { name } => name == "_",
            TreeKind::Star { elem } => Self::is_wildcard(elem) || Self::is_var_pattern(elem),
            TreeKind::Alternative { trees } => trees.iter().all(Self::is_wildcard),
            TreeKind::Bind { name, body } => name == "_" && Self::is_wildcard(body),
            _ => false,
        }
    }

    /// `translateCase`: pattern, guard and body, the substitution
    /// propagated (`SubstOnly` makers dropped).
    pub(crate) fn translate_case(
        &mut self,
        scrut: B,
        c: &CaseDef,
    ) -> Result<Vec<Maker>, Unsupported> {
        let mut makers = self.translate(scrut, &c.pat)?;
        if !c.guard.is_empty() {
            let constant = match &c.guard.kind {
                TreeKind::Literal {
                    lit: Lit::Boolean(b),
                } => Some(*b),
                _ => None,
            };
            makers.push(Maker::new(TM::Guard { constant }));
        }
        makers.push(Maker::new(TM::Body {
            pos: point_of(&c.body, self.src),
        }));
        Ok(propagate(makers, Subst::default()))
    }

    /// `BoundTree(binder, pat).translate()`.
    fn translate(&mut self, b: B, pat: &Tree) -> Result<Vec<Maker>, Unsupported> {
        if Self::is_wildcard(pat) {
            return Ok(vec![Maker::new(TM::Dummy)]);
        }
        match &pat.kind {
            TreeKind::Ident { .. } if Self::is_var_pattern(pat) => {
                // nsc's `Bind(x, _)`: a binding step, then `_`.
                if pat.sym.is_none() {
                    return Err(bail!());
                }
                let tp = self.binders[b].tp.clone();
                let x = self.sym_binder(pat.sym, tp);
                Ok(vec![
                    Maker::new(TM::SubstOnly { prev: x, next: b }),
                    Maker::new(TM::Dummy),
                ])
            }
            TreeKind::Bind { body, .. } => {
                if pat.sym.is_none() {
                    return Err(bail!());
                }
                if let TreeKind::Typed { expr, .. } = &body.kind {
                    if Self::is_wildcard(expr) {
                        // `SymbolAndTypeBound`
                        let tpe = self.ty(&body.ty);
                        let x = self.sym_binder_keep(pat.sym, tpe.clone());
                        return Ok(self.type_test_step(x, b, tpe));
                    }
                }
                let tp = self.binders[b].tp.clone();
                let x = self.sym_binder(pat.sym, tp);
                let mut out = vec![Maker::new(TM::SubstOnly { prev: x, next: b })];
                out.extend(self.translate(b, body)?);
                Ok(out)
            }
            TreeKind::Typed { expr, .. } => {
                let tpe = self.ty(&pat.ty);
                if Self::is_wildcard(expr) {
                    // `TypeBound`
                    Ok(self.type_test_step(b, b, tpe))
                } else if Self::is_var_pattern(expr) && !expr.sym.is_none() {
                    // `x: T`, our spelling of `Bind(x, Typed(_, T))`.
                    let x = self.sym_binder_keep(expr.sym, tpe.clone());
                    Ok(self.type_test_step(x, b, tpe))
                } else {
                    Err(bail!())
                }
            }
            TreeKind::Apply { .. } | TreeKind::UnApply { .. } => self.extractor_step(b, pat),
            TreeKind::Literal { .. } | TreeKind::Ident { .. } | TreeKind::Select { .. } => {
                let pv = self.pat_val(pat)?;
                let next_tp = self.tys.widen(&self.binders[b].tp);
                let next = self.binder(next_tp);
                Ok(vec![Maker::new(TM::EqTest {
                    prev: b,
                    pat: pv,
                    next,
                })])
            }
            TreeKind::Alternative { trees } => {
                let mut alts = Vec::new();
                for a in trees {
                    alts.push(self.translate(b, a)?);
                }
                let pos = trees
                    .first()
                    .map(|a| point_of(a, self.src))
                    .unwrap_or(pat.span.lo.0);
                // `SwitchableTreeMaker(pattern) :: Nil` for every alternative.
                let switch = alts
                    .iter()
                    .map(|a| match a.as_slice() {
                        [Maker {
                            tm: TM::EqTest { pat, .. },
                            ..
                        }] => pat.switch_const.clone().map(|c| (c, pat.text.clone())),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>();
                Ok(vec![Maker::new(TM::Alts { alts, pos, switch })])
            }
            _ => Err(bail!()),
        }
    }

    fn type_test_step(&mut self, prev: B, tested: B, expected: NTy) -> Vec<Maker> {
        let next = self.binder(expected.clone());
        vec![Maker::new(TM::TypeTest {
            prev,
            tested,
            expected,
            next,
            extractor_arg: false,
        })]
    }

    /// The `patTree` of an equality test: its type (`p.tpe.normalize`), how
    /// nsc hash-conses it, and what it prints as.
    fn pat_val(&mut self, pat: &Tree) -> Result<PatVal, Unsupported> {
        match &pat.kind {
            TreeKind::Literal { lit } => {
                let c = CVal::from_lit(lit).ok_or_else(|| bail!())?;
                let tp = if c == CVal::Null {
                    NTy::Null
                } else {
                    NTy::Const(c.clone())
                };
                Ok(PatVal {
                    key: PatKey::Lit(c.clone()),
                    tp,
                    text: c.escaped(),
                    switch_const: switchable_const(&c),
                })
            }
            TreeKind::Ident { name } | TreeKind::Select { name, .. } => {
                if pat.sym.is_none() {
                    return Err(bail!());
                }
                let st = self.tys.st;
                let s = st.get(pat.sym);
                let (tp, stable) = match s.kind {
                    SymKind::Module | SymKind::ModuleClass => {
                        (NTy::Module(st.module_class_of(pat.sym)), true)
                    }
                    SymKind::Term | SymKind::Method => {
                        let stable = !s.flags.contains(Flags::MUTABLE)
                            && (s.kind == SymKind::Term || s.flags.contains(Flags::ACCESSOR));
                        let vt = self.tys.value_type(pat.sym);
                        let constant = match (&vt, &pat.ty) {
                            (Type::Constant(l), _) | (_, Type::Constant(l)) => Some(l.clone()),
                            _ => None,
                        };
                        let library = crate::warn_patmat_types::library_constant(st, pat.sym);
                        let tp = if let Some(c) = library {
                            NTy::Const(c)
                        } else {
                            match constant {
                                Some(l) => match CVal::from_lit(&l) {
                                    Some(CVal::Null) => NTy::Null,
                                    Some(c) => NTy::Const(c),
                                    None => return Err(bail!()),
                                },
                                None if stable => NTy::Single(pat.sym),
                                None => {
                                    *self.fresh += 1;
                                    let w = self.ty(&vt);
                                    if w.is_unknown() {
                                        return Err(bail!());
                                    }
                                    NTy::Fresh(*self.fresh, Box::new(w))
                                }
                            }
                        };
                        (tp, stable)
                    }
                    _ => return Err(bail!()),
                };
                if matches!(&tp, NTy::Single(_)) && self.tys.widen(&tp).is_unknown() {
                    return Err(bail!());
                }
                let prefix = match &pat.kind {
                    TreeKind::Select { qual, .. } if !qual.sym.is_none() => {
                        Some(Box::new(PatKey::Sym(qual.sym, None)))
                    }
                    _ => None,
                };
                let key = if stable {
                    PatKey::Sym(pat.sym, prefix)
                } else {
                    *self.fresh += 1;
                    PatKey::Unstable(*self.fresh)
                };
                let switch_const = match &tp {
                    NTy::Const(c) => switchable_const(c),
                    _ => None,
                };
                Ok(PatVal {
                    key,
                    tp,
                    text: name.trim_end_matches('$').to_string(),
                    switch_const,
                })
            }
            _ => Err(bail!()),
        }
    }

    /// `BoundTree.extractorStep`: a case class's constructor pattern or an
    /// `unapply` / `unapplySeq` extractor, with `ExtractorAlignment`'s
    /// arities.
    fn extractor_step(&mut self, b: B, pat: &Tree) -> Result<Vec<Maker>, Unsupported> {
        let st = self.tys.st;
        let (args, is_ctor) = match &pat.kind {
            TreeKind::Apply { args, .. } => (args, true),
            TreeKind::UnApply { args, .. } => (args, false),
            _ => return Err(bail!()),
        };
        if pat.sym.is_none() {
            return Err(bail!());
        }
        let is_star = args.last().is_some_and(is_star_pattern);
        let non_star_arity = args.len() - usize::from(is_star);
        let star_arity = usize::from(is_star);
        let total_arity = args.len();

        let param_type: NTy;
        let product_types: Vec<NTy>;
        let elem_type: Option<NTy>;
        let mut result_in_monad = NTy::Unknown;
        let mut ext: Option<ExtInfo> = None;
        let mut fields: Vec<SymbolId> = Vec::new();
        if is_ctor {
            let cls = pat.sym;
            let s = st.get(cls);
            if s.kind != SymKind::Class || !s.flags.contains(Flags::CASE) {
                return Err(bail!());
            }
            param_type = self.ty(&pat.ty);
            let cargs = match &pat.ty {
                Type::Class { args, .. } => args.clone(),
                _ => Vec::new(),
            };
            fields = s.ctor_fields.clone();
            let repeated = st.repeated_case_element(cls);
            let mut field_tys = Vec::new();
            for f in &fields {
                let ft = st.get(*f).ty.clone();
                let ft = if cargs.is_empty() {
                    ft
                } else {
                    st.subst_tparams(cls, &cargs, &ft)
                };
                let ft = match ft {
                    Type::Repeated(e) => *e,
                    Type::Method { paramss, ret } if paramss.is_empty() => *ret,
                    t => t,
                };
                field_tys.push(self.ty(&ft));
            }
            match &repeated {
                Some(e) => {
                    let e = if cargs.is_empty() {
                        e.clone()
                    } else {
                        st.subst_tparams(cls, &cargs, e)
                    };
                    let mut p = field_tys.clone();
                    p.pop();
                    product_types = p;
                    elem_type = Some(self.ty(&e));
                }
                None => {
                    if total_arity == 1 && field_tys.len() > 1 {
                        return Err(bail!());
                    }
                    product_types = field_tys;
                    elem_type = None;
                }
            }
        } else {
            let u = pat.sym;
            let us = st.get(u);
            let is_seq_ex = us.name == "unapplySeq";
            if !is_seq_ex && us.name != "unapply" {
                return Err(bail!());
            }
            let Type::Method { paramss, ret } = &us.ty else {
                return Err(bail!());
            };
            let param = paramss
                .first()
                .and_then(|c| c.first())
                .ok_or_else(|| bail!())?;
            let (param, ret) = if us.tparams.is_empty() {
                (param.clone(), (**ret).clone())
            } else {
                // A polymorphic extractor (`List.unapplySeq[A](x: List[A])`):
                // instantiate it from the scrutinee, as the typer did (the
                // pattern's own type is the scrutinee's).
                let tps = us.tparams.clone();
                let mut found: Vec<Option<Type>> = vec![None; tps.len()];
                // Against the scrutinee's base type for the parameter's class
                // (`Seq[A]` against a `List[String]` is `Seq[String]`).
                let actual = match (param, &pat.ty) {
                    (Type::Class { sym: p, .. }, Type::Class { sym: s, .. }) if p != s => st
                        .base_type_seq(&pat.ty)
                        .into_iter()
                        .find(|b| matches!(b, Type::Class { sym, .. } if sym == p))
                        .unwrap_or_else(|| pat.ty.clone()),
                    _ => pat.ty.clone(),
                };
                unify_tparams(param, &actual, &tps, &mut found);
                let Some(args) = found.into_iter().collect::<Option<Vec<Type>>>() else {
                    if std::env::var_os("SCALA_RS_PATMAT_DEBUG").is_some() {
                        eprintln!("patmat: unify {param:?} with {:?}", pat.ty);
                    }
                    return Err(bail!());
                };
                (
                    st.subst_tparams(u, &args, param),
                    st.subst_tparams(u, &args, ret),
                )
            };
            let mut ret = ret;
            param_type = {
                let p = self.ty(&param);
                if p.is_unknown() {
                    // `SeqFactory.unapplySeq[A](x: CC[A])` seen through a
                    // companion (`List(a, b)`): `CC` is the companion's own
                    // class, so the parameter is the scrutinee's type when
                    // that is the class, and nothing we can name otherwise.
                    let owner = us.owner;
                    let sel = self.ty(&pat.ty);
                    let companion = if owner.is_none() {
                        None
                    } else {
                        let o = st.get(owner);
                        let cname = o.jvm_name.strip_suffix('$').unwrap_or("").to_string();
                        crate::classpath::find_by_jvm(st, &cname)
                    };
                    let applied_arg = match &param {
                        Type::Applied { args, .. } if args.len() == 1 => Some(self.ty(&args[0])),
                        _ => None,
                    };
                    match (&sel, companion) {
                        (NTy::Class(c, args), Some(comp)) if *c == comp => {
                            // The wrapper's element type is the scrutinee's.
                            if let (Type::Class { sym, args: rargs }, Some(first)) =
                                (&ret, args.first())
                            {
                                if rargs.len() == 1 && !first.is_unknown() {
                                    if let Type::Class { args: sargs, .. } = &pat.ty {
                                        ret = Type::Class {
                                            sym: *sym,
                                            args: vec![sargs[0].clone()],
                                        };
                                    }
                                }
                            }
                            sel
                        }
                        // `Seq(x, y)` against a `List[Int]`: the parameter is
                        // `Seq[Int]`.
                        (_, Some(comp))
                            if st.get(comp).tparams.len() == 1
                                && applied_arg.as_ref().is_some_and(|a| !a.is_unknown()) =>
                        {
                            NTy::Class(comp, vec![applied_arg.unwrap()])
                        }
                        _ => p,
                    }
                } else {
                    p
                }
            };
            let ret = &ret;
            let ret_ty = self.ty(ret);
            let boolean = matches!(*ret, Type::Boolean);
            let is_wrapper = |c: SymbolId| {
                st.get(c).name == "UnapplySeqWrapper" && st.get(c).jvm_name.starts_with("scala/")
            };
            let (get_ty, irrefutable) = match &ret_ty {
                NTy::Class(c, a) if *c == st.option_sym && a.len() == 1 => (a[0].clone(), false),
                NTy::Class(c, a) if *c == st.some_sym && a.len() == 1 => (a[0].clone(), true),
                NTy::Class(c, a) if a.len() == 1 && is_wrapper(*c) && is_seq_ex => {
                    (ret_ty.clone(), false)
                }
                _ if boolean && !is_seq_ex => (NTy::Unknown, false),
                _ => return Err(bail!()),
            };
            let seq_wrapper = matches!(&get_ty, NTy::Class(c, _) if is_wrapper(*c));
            let mut seq_wrapper_factory = false;
            let equiv: Vec<NTy> = if boolean && !is_seq_ex {
                Vec::new()
            } else {
                match &get_ty {
                    NTy::Class(c, comps)
                        if comps.len() > 1
                            && st.get(*c).name == format!("Tuple{}", comps.len())
                            && st.get(*c).jvm_name.starts_with("scala/Tuple") =>
                    {
                        comps.clone()
                    }
                    other => vec![other.clone()],
                }
            };
            if is_seq_ex {
                let mut p = equiv.clone();
                let last = p.pop().ok_or_else(|| bail!())?;
                // nsc: the last extracted type is a `Seq[E]` (or the
                // `UnapplySeqWrapper[E]` a collection factory returns). The
                // prelude writes a factory's `unapplySeq` with the element
                // type itself (`Option[A]`).
                let factory = st.get(us.owner).jvm_name.starts_with("scala/collection/");
                let elem = match &last {
                    NTy::Class(c, a) if a.len() == 1 && !(factory && !is_seq_like(st, *c)) => {
                        a[0].clone()
                    }
                    other if factory && !other.is_unknown() => other.clone(),
                    _ => return Err(bail!()),
                };
                if factory {
                    seq_wrapper_factory = true;
                }
                product_types = p;
                elem_type = Some(elem);
            } else if total_arity == 1 && equiv.len() > 1 {
                product_types = vec![get_ty.clone()];
                elem_type = None;
            } else {
                product_types = equiv;
                elem_type = None;
            }
            let is_bool = !is_seq_ex && product_types.is_empty();
            result_in_monad = if is_bool {
                NTy::Class(st.unit_sym, vec![])
            } else {
                get_ty
            };
            ext = Some(ExtInfo {
                unapply: u,
                irrefutable,
                seq_wrapper: seq_wrapper || seq_wrapper_factory,
            });
        }
        if param_type.is_unknown() {
            return Err(bail!());
        }
        let product_arity = product_types.len();
        let is_seq = elem_type.is_some();
        if non_star_arity < product_arity || (non_star_arity > product_arity && !is_seq) {
            return Err(bail!());
        }
        let element_arity = non_star_arity - product_arity;
        let checked_length = if !is_seq || element_arity < star_arity {
            None
        } else {
            Some(element_arity)
        };
        let is_single = !is_seq && total_arity == 1;

        // `tpe <:< paramType`: a non-null test, or a type test that casts.
        let tpe = self.binders[b].tp.clone();
        let mut makers = Vec::new();
        let unapp_binder = match self.tys.sub(&tpe, &param_type) {
            Some(true) => {
                makers.push(Maker::new(TM::NonNull { prev: b }));
                b
            }
            Some(false) => {
                let next = self.binder(param_type.clone());
                makers.push(Maker::new(TM::TypeTest {
                    prev: b,
                    tested: b,
                    expected: param_type.clone(),
                    next,
                    extractor_arg: true,
                }));
                next
            }
            None => return Err(bail!()),
        };

        // Sub-pattern binders and their types (`subPatTypes`).
        let mut sub_types = product_types.clone();
        if let Some(e) = &elem_type {
            for _ in 0..element_arity {
                sub_types.push(e.clone());
            }
            if is_star {
                sub_types.push(NTy::Unknown);
            }
        }
        let mut subs = Vec::new();
        let mut sub_pats: Vec<(B, &Tree)> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let tp = sub_types.get(i).cloned().unwrap_or(NTy::Unknown);
            let (bind, p): (B, &Tree) = match &a.kind {
                TreeKind::Bind { body, .. } if !a.sym.is_none() => {
                    (self.sym_binder(a.sym, tp), &**body)
                }
                TreeKind::Ident { .. } if Self::is_var_pattern(a) && !a.sym.is_none() => {
                    (self.sym_binder(a.sym, tp), a)
                }
                // `x: T` is nsc's `Bind(x, Typed(_, T))`: `x` is the binder.
                TreeKind::Typed { expr, .. }
                    if Self::is_var_pattern(expr)
                        && !expr.sym.is_none()
                        && !matches!(&expr.kind, TreeKind::Ident { name } if name == "_") =>
                {
                    (self.sym_binder(expr.sym, tp), a)
                }
                _ => (self.binder(tp), a),
            };
            subs.push(bind);
            sub_pats.push((bind, p));
        }

        // The extracted values (`subPatRefs`).
        let result_binder = if is_ctor {
            unapp_binder
        } else {
            self.binder(result_in_monad.clone())
        };
        let tuple_sel = |i: usize| -> Field {
            if is_ctor {
                Field::Acc(fields[i - 1])
            } else {
                Field::Tuple(i)
            }
        };
        let elems_to_n = |n: usize| -> Vec<PTree> {
            (1..=n)
                .map(|i| PTree::Sel(Box::new(PTree::Ref(result_binder)), tuple_sel(i)))
                .collect()
        };
        let refs: Vec<PTree> = if !is_ctor && is_single {
            vec![PTree::Ref(result_binder)]
        } else if total_arity > 0 && is_seq {
            let seq_tree = if !is_ctor && product_arity == 0 {
                PTree::Ref(result_binder)
            } else {
                PTree::Sel(
                    Box::new(PTree::Ref(result_binder)),
                    tuple_sel(product_arity + 1),
                )
            };
            let mut r = elems_to_n(product_arity);
            for i in 0..element_arity {
                r.push(PTree::Sel(Box::new(seq_tree.clone()), Field::Index(i)));
            }
            if is_star {
                if element_arity == 0 {
                    r.push(seq_tree.clone());
                } else {
                    r.push(PTree::Sel(
                        Box::new(seq_tree.clone()),
                        Field::Drop(element_arity),
                    ));
                }
            }
            r
        } else {
            elems_to_n(total_arity)
        };
        if refs.len() != subs.len() {
            return Err(bail!());
        }
        if is_ctor {
            makers.push(Maker::new(TM::Product {
                prev: unapp_binder,
                has_extra: checked_length.is_some(),
                subs: subs.clone(),
                refs,
            }));
        } else {
            makers.push(Maker::new(TM::Extractor {
                ext: ext.expect("an extractor pattern"),
                has_extra: checked_length.is_some(),
                next: result_binder,
                subs: subs.clone(),
                refs,
                prev: unapp_binder,
                checked_length,
            }));
        }
        for (bind, p) in sub_pats {
            makers.extend(self.translate(bind, p)?);
        }
        Ok(makers)
    }
}

/// A sequence class (`Seq`, `List`, ...) or `UnapplySeqWrapper`: what the
/// last type an `unapplySeq` extracts is.
fn is_seq_like(st: &crate::symbol::SymbolTable, c: SymbolId) -> bool {
    let s = st.get(c);
    if s.name == "UnapplySeqWrapper" {
        return true;
    }
    let seq = crate::classpath::find_by_jvm(st, "scala/collection/Seq");
    c == st.list_sym
        || seq.is_some_and(|q| q == c || crate::pickle_supply::inherits_from(st, c, q))
        || matches!(
            s.name.as_str(),
            "Seq" | "IndexedSeq" | "LinearSeq" | "List" | "Vector"
        )
}

/// Bind `tps` by matching the declared type `p` against the actual `s`.
fn unify_tparams(p: &Type, s: &Type, tps: &[SymbolId], out: &mut [Option<Type>]) {
    match (p, s) {
        (Type::TypeParam(id), _) => {
            if let Some(i) = tps.iter().position(|t| t == id) {
                if out[i].is_none() {
                    out[i] = Some(s.clone());
                }
            }
        }
        (Type::Class { sym: a, args: pa }, Type::Class { sym: b, args: sa })
            if a == b && pa.len() == sa.len() =>
        {
            for (x, y) in pa.iter().zip(sa) {
                unify_tparams(x, y, tps, out);
            }
        }
        // `CC[A]` against `List[Int]`: the factory's collection type is the
        // scrutinee's class or one of its parents, with the same argument.
        (Type::Applied { args: pa, .. }, Type::Class { args: sa, .. }) if pa.len() == sa.len() => {
            for (x, y) in pa.iter().zip(sa) {
                unify_tparams(x, y, tps, out);
            }
        }
        (Type::Annotated { tpe, .. }, _) => unify_tparams(tpe, s, tps, out),
        (_, Type::Annotated { tpe, .. }) => unify_tparams(p, tpe, tps, out),
        _ => {}
    }
}

pub(crate) fn is_star_pattern(t: &Tree) -> bool {
    match &t.kind {
        TreeKind::Star { .. } => true,
        TreeKind::Bind { body, .. } => is_star_pattern(body),
        _ => false,
    }
}

/// `SwitchablePattern`: a constant in `Int` range, a `String`, or `null`.
pub(crate) fn switchable_const(c: &CVal) -> Option<CVal> {
    match c {
        CVal::Int(_) | CVal::Char(_) | CVal::Str(_) | CVal::Null => Some(c.clone()),
        _ => None,
    }
}
