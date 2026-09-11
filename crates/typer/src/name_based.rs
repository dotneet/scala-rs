//! Name-based pattern matching: what an `unapply` pattern's sub-patterns are
//! matched against (SLS 8.1.8, nsc's `PatternExpansion`).
//!
//! An `unapply` returns `Boolean` (no sub-patterns), or a value `r` of any
//! type with a nullary `isEmpty: Boolean` and a nullary `get`. `Option` is
//! just the most common such type. The pattern fails when `r.isEmpty`, and
//! otherwise matches `r.get` against its sub-patterns:
//!
//! * one sub-pattern matches the whole `get` value -- even a tuple (nsc
//!   deprecates that "crushing" but accepts it) and even a `Tuple1`;
//! * N > 1 sub-patterns match its product selectors `_1` … `_N`, which a
//!   `TupleN` has and any other class may declare.
//!
//! scala-rs used to read the extracted types off `Option`'s type argument and
//! nothing else, and the backend always called `scala.Option.isEmpty/get`: an
//! extractor returning its own class (`def unapply(a: Casey) = a`) failed
//! verification, `case Foo997(a)` on an `Option[(String, String)]` was
//! reported as an arity error, and `case Extractor(x, y)` on an
//! `Option[Product2[Int, String]]` cast the product to `Tuple2`.

use crate::check::{numbered_arity, Typer};
use crate::symbol::{NameBasedUnapply, SymKind, UnapplySelectors};
use scala_rs_parser::ast::Type;
use scala_rs_parser::{Flags, SymbolId};
use scala_rs_span::Span;

impl Typer {
    /// The types an `unapply` pattern's `n` sub-patterns are matched against,
    /// or `None` when the extractor cannot be used and that was reported
    /// (with `report`).
    ///
    /// `typed_ret` is the result type of the call `X.unapply(<selector>)`
    /// when that was typed in full (an extractor with an implicit clause, see
    /// [`Self::type_unapply_call`]); otherwise the result is read off the
    /// declaration, its type parameters solved against the scrutinee.
    pub(crate) fn unapply_pattern_types(
        &mut self,
        unapply: SymbolId,
        sel_ty: &Type,
        n: usize,
        span: Span,
        report: bool,
        typed_ret: Option<Type>,
    ) -> Option<Vec<Type>> {
        // A case class's synthetic `unapply` is compiled as its constructor
        // pattern (`gen_ctor_fields_pattern`), one sub-pattern per field; nsc
        // does the same and reports a wrong count as an arity error.
        if self.st.get(unapply).flags.contains(Flags::CASE) {
            let extracted = self.unapply_extracted_types(unapply);
            return Some(self.subst_unapply_tparams(unapply, sel_ty, extracted));
        }
        let ret = match typed_ret {
            Some(t) => t,
            None => {
                let ret = self.st.get(unapply).ty.result().clone();
                self.subst_unapply_tparams(unapply, sel_ty, vec![ret.clone()])
                    .pop()
                    .unwrap_or(ret)
            }
        };
        let ret = self.st.dealias(&ret);
        if matches!(ret, Type::Boolean) {
            return Some(Vec::new());
        }
        let get_ty = match &ret {
            Type::Class { sym, args } if self.is_option_like(*sym) => {
                if args.is_empty() {
                    // An erased `Option` read back from a classfile: see
                    // `unapply_extracted_types`.
                    if let Some(fields) = self.case_ctor_field_types(self.st.get(unapply).owner) {
                        return Some(fields);
                    }
                }
                args.first().cloned().unwrap_or(Type::Any)
            }
            Type::Class { .. } | Type::String => {
                self.name_based_get(unapply, &ret, span, report)?
            }
            // Anything else (a type parameter, a structural type, `Nothing`)
            // keeps the old reading of the result as the extracted value.
            _ => return Some(self.unapply_extracted_types_of(ret)),
        };
        if n == 1 {
            // nsc accepts one pattern for a whole tuple but deprecates it
            // (`neg/t6675`). Only a tuple, and only one the extractor
            // *declares*: `unapply[A](v: Either[A, A]): Option[A]` matched
            // against pairs binds its `A` without a word (`pos/t6675`), and a
            // class with product selectors of its own is bound whole too.
            let declared = match self.st.dealias(self.st.get(unapply).ty.result()) {
                Type::Class { sym, args } if self.is_option_like(sym) => args.first().cloned(),
                _ => None,
            };
            if report {
                if let Some(k) = declared
                    .as_ref()
                    .and_then(|d| crate::check::tuple_arity(&self.st, &self.st.dealias(d)))
                    .filter(|&k| k > 1)
                {
                    let elems = match self.st.dealias(&get_ty) {
                        Type::Tuple(ts) | Type::Class { args: ts, .. } => ts,
                        _ => Vec::new(),
                    };
                    let elems = elems
                        .iter()
                        .map(|t| self.st.display_type(t))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let owner = self.st.get(unapply).owner;
                    let what = match self.st.get(owner).kind {
                        SymKind::Module | SymKind::ModuleClass => "object",
                        _ => "class",
                    };
                    let name = self.st.get(owner).name.trim_end_matches('$').to_string();
                    self.warning(
                        span,
                        format!(
                            "deprecated adaptation: {what} {name} expects {k} patterns to hold \
                             ({elems}) but crushing into {k}-tuple to fit single pattern \
                             (scala/bug#6675)"
                        ),
                    );
                }
            }
            return Some(vec![get_ty]);
        }
        Some(self.product_selector_types(unapply, &get_ty, n))
    }

    /// An extractor whose `unapply` takes a clause after the scrutinee's
    /// (necessarily implicit): `def unapply(s: String)(implicit p:
    /// Option[String] = None)` (`run/t3353`), `def unapply[S, T](s: S)
    /// (implicit w: FooHasType[S, T]): Option[T]` (`run/t6111`).
    ///
    /// nsc types the pattern's extractor as the call
    /// `X.unapply(<unapply-selector>)` and lets ordinary application fill
    /// the implicit clause -- which is also what solves a type parameter only
    /// the implicit mentions (`T` above). The same call is typed here, with
    /// the selector a transient local of the `unapply`'s parameter type.
    /// Returns the call's result type and the implicit arguments it was
    /// given; the backend passes those after the scrutinee. The call used to
    /// be emitted with the scrutinee alone, which did not verify.
    pub(crate) fn type_unapply_call(
        &mut self,
        fun: &scala_rs_parser::Tree,
        unapply: SymbolId,
        sel_ty: &Type,
        span: Span,
    ) -> Option<(Type, Vec<scala_rs_parser::Tree>)> {
        use scala_rs_parser::{Tree, TreeKind};
        let clauses = match &self.st.get(unapply).ty {
            Type::Method { paramss, .. } => paramss.len(),
            _ => 0,
        };
        if clauses < 2 {
            return None;
        }
        let param = self.unapply_receiver_type(unapply, sel_ty)?;
        let name = self.fresh("unapply$selector");
        let sel = self
            .st
            .alloc(&name, self.st.owner, SymKind::Term, Flags::SYNTHETIC, "");
        self.st.get_mut(sel).ty = param;
        let mut ident = Tree::dummy(TreeKind::Ident { name: name.clone() });
        ident.span = span;
        let mut select = Tree::dummy(TreeKind::Select {
            qual: Box::new(fun.clone()),
            name: self.st.get(unapply).name.clone(),
        });
        select.span = span;
        let mut call = Tree::dummy(TreeKind::Apply {
            fun: Box::new(select),
            args: vec![ident],
        });
        call.span = span;
        self.st.push_scope();
        self.st.enter_in_current(&name, sel);
        self.type_expr(&mut call, &Type::NoType);
        self.st.pop_scope();
        if call.ty.is_error() || call.ty.is_no_type() {
            return None;
        }
        // `fill_defaults_and_implicits` appends the implicit clause to the
        // argument list it was handed (`Apply(X.unapply, [sel, implicits..])`);
        // a nested `Apply(Apply(X.unapply, [sel]), implicits)` is taken too.
        let implicits = match &call.kind {
            TreeKind::Apply { fun: inner, args }
                if matches!(inner.kind, TreeKind::Apply { .. }) =>
            {
                args.clone()
            }
            TreeKind::Apply { args, .. } if args.len() > 1 => args[1..].to_vec(),
            _ => Vec::new(),
        };
        Some((call.ty.clone(), implicits))
    }

    /// `StringContext(parts: _*).s(args: _*)` written out as a call.
    ///
    /// `s` is a macro in 2.13 (`def s(args: Any*): String = macro ???`): the
    /// jar's `scala.StringContext` has no `s(Seq)` method at all, only
    /// `s()`, the object `case s"…"` patterns use. The prelude declares an
    /// `s(Any*)` so interpolation literals type, and a call written out
    /// against it compiled to `invokevirtual StringContext.s(Seq)` --
    /// `NoSuchMethodError` at run time (`run/interpolationArgs`). nsc's
    /// expansion for a receiver that is not a literal is
    /// `sc.standardInterpolator(StringContext.processEscapes, args)`, which
    /// also does the part/argument count check the test prints. That is what
    /// the call is rewritten to here. (Interpolation *literals* are an
    /// `InterpolatedString` tree of their own and never reach this.)
    pub(crate) fn rewrite_explicit_s_interpolator(
        &mut self,
        tree: &mut scala_rs_parser::Tree,
        pt: &Type,
    ) -> bool {
        use scala_rs_parser::{Tree, TreeKind};
        let TreeKind::Apply { fun, args } = &tree.kind else {
            return false;
        };
        let TreeKind::Select { qual, name } = &fun.kind else {
            return false;
        };
        if name != "s" || fun.sym.is_none() || !self.library_abi {
            return false;
        }
        let owner = self.st.get(fun.sym).owner;
        if self.st.get(owner).jvm_name != "scala/StringContext"
            || !self.st.get(fun.sym).pickled_origin.is_empty()
        {
            return false;
        }
        let span = tree.span;
        let mk = |kind: TreeKind| {
            let mut t = Tree::dummy(kind);
            t.span = span;
            t
        };
        let mut sc = mk(TreeKind::Ident {
            name: "StringContext".into(),
        });
        sc.scala_ref = true;
        let process = mk(TreeKind::Select {
            qual: Box::new(sc),
            name: "processEscapes".into(),
        });
        // `sc.s(xs: _*)` hands its sequence over as it is; anything else is
        // collected into a `Seq`.
        let seq_arg = match args.as_slice() {
            [one]
                if matches!(&one.kind, TreeKind::Typed { tpt, .. }
                if matches!(tpt.kind, TreeKind::Star { .. } | TreeKind::Wildcard)
                    || matches!(&tpt.kind, TreeKind::Ident { name } if name == "_*")) =>
            {
                match &one.kind {
                    TreeKind::Typed { expr, .. } => (**expr).clone(),
                    _ => unreachable!(),
                }
            }
            _ => {
                let mut seq = mk(TreeKind::Ident { name: "Seq".into() });
                seq.scala_ref = true;
                mk(TreeKind::Apply {
                    fun: Box::new(seq),
                    args: args.clone(),
                })
            }
        };
        let call = mk(TreeKind::Apply {
            fun: Box::new(mk(TreeKind::Select {
                qual: qual.clone(),
                name: "standardInterpolator".into(),
            })),
            args: vec![process, seq_arg],
        });
        *tree = call;
        self.type_expr(tree, pt);
        true
    }

    /// Report an extractor method that cannot take the scrutinee: SLS 8.1.8
    /// wants exactly one parameter in its first clause (an implicit clause
    /// may follow). `def unapply: Option[Int]`, `def unapply()` and
    /// `def unapply(a: Int, b: Int)` were accepted and then called with the
    /// scrutinee anyway -- a `VerifyError` (`neg/t5078`). Messages as
    /// scalac 2.13.16 words them. Only a method this run defines: the
    /// prelude's and the pickle's own extractors are not re-judged here.
    pub(crate) fn reject_extractor_shape(
        &mut self,
        unapply: SymbolId,
        fun: &scala_rs_parser::Tree,
        span: Span,
    ) -> bool {
        let s = self.st.get(unapply);
        if unapply.0 < self.st.prelude_end || !s.pickled_origin.is_empty() {
            return false;
        }
        let owner = s.owner;
        if self.st.get(owner).jvm_name.starts_with("scala/") {
            return false;
        }
        let (paramss, ret) = match &s.ty {
            Type::Method { paramss, ret } => (paramss.clone(), (**ret).clone()),
            other => (Vec::new(), other.clone()),
        };
        let method = s.name.clone();
        let reason = match paramss.first() {
            None => format!("an {method} method must accept a single argument"),
            Some(c) if c.is_empty() => format!("an {method} method must accept a single argument"),
            Some(c) if c.len() > 1 => {
                "as it has more than one (non-implicit) parameter".to_string()
            }
            Some(_) => return false,
        };
        // `def unapply(a: Int, b: Int): Option[Int]`, with the parameter
        // names the symbol carries.
        let names: Vec<Vec<String>> = self
            .st
            .get(unapply)
            .paramss
            .iter()
            .map(|c| c.iter().map(|p| self.st.get(*p).name.clone()).collect())
            .collect();
        let clauses: String = paramss
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let ps: Vec<String> = c
                    .iter()
                    .enumerate()
                    .map(|(j, t)| {
                        let n = names
                            .get(i)
                            .and_then(|c| c.get(j))
                            .cloned()
                            .unwrap_or_else(|| format!("x${}", j + 1));
                        format!("{n}: {}", self.st.display_type(t))
                    })
                    .collect();
                format!("({})", ps.join(", "))
            })
            .collect();
        let what = match self.st.get(owner).kind {
            SymKind::Module | SymKind::ModuleClass => "object",
            _ => "value",
        };
        let name = fun
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| self.st.get(owner).name.trim_end_matches('$').to_string());
        let sep = if reason.starts_with("as ") { " " } else { ": " };
        self.error(
            span,
            format!(
                "{what} {name} is not a case class, nor does it have a valid unapply/unapplySeq \
                 member\nNote: def {method}{clauses}: {} exists in {what} {name}, but it cannot \
                 be used as an extractor{sep}{reason}",
                self.st.display_type(&ret)
            ),
        );
        true
    }

    /// `Option` / `Some` by name as well as by symbol: the prelude and the
    /// pickle may each supply one.
    fn is_option_like(&self, sym: SymbolId) -> bool {
        let name = self.st.get(sym).name.as_str();
        sym == self.st.option_sym || name == "Option" || name == "Some"
    }

    /// Resolve `isEmpty` and `get` on a non-`Option` result type, record them
    /// for the backend, and answer `get`'s type.
    fn name_based_get(
        &mut self,
        unapply: SymbolId,
        ret: &Type,
        span: Span,
        report: bool,
    ) -> Option<Type> {
        let get = self.nullary_member(ret, "get", span);
        let is_empty = self.nullary_member(ret, "isEmpty", span);
        // nsc checks `get` first (`t8989`: a class with `isEmpty` only), then
        // that `isEmpty` exists and is a `Boolean` (`t7850`).
        let Some((get, get_ty)) = get else {
            if report {
                self.error(
                    span,
                    format!(
                        "The result type of an unapply method must contain a member `get` to be \
                         used as an extractor pattern, no such member exists in {}",
                        self.st.display_type(ret)
                    ),
                );
            }
            return None;
        };
        match is_empty {
            Some((is_empty, ty)) if matches!(self.st.dealias(&ty), Type::Boolean) => {
                if !self.st.name_based_unapply.contains_key(&unapply) {
                    let result_class = self.st.class_sym_of(ret).unwrap_or(SymbolId::NONE);
                    let result_tmp = self.alloc_extractor_tmp("unapply$result", ret);
                    self.st.name_based_unapply.insert(
                        unapply,
                        NameBasedUnapply {
                            is_empty,
                            get,
                            result_tmp,
                            result_class,
                        },
                    );
                }
                Some(get_ty)
            }
            found => {
                if report {
                    let found = found
                        .map(|(_, ty)| {
                            format!(" (found: `def isEmpty: {}`)", self.st.display_type(&ty))
                        })
                        .unwrap_or_default();
                    self.error(
                        span,
                        format!(
                            "an unapply result must have a member `def isEmpty: Boolean`{found}"
                        ),
                    );
                }
                None
            }
        }
    }

    /// The types of `_1` … `_n` on `get_ty`: a tuple's elements, or the
    /// product selectors a class declares (recorded for the backend). A type
    /// with no selectors at all is one value, and the arity check that
    /// follows reports the count.
    fn product_selector_types(&mut self, unapply: SymbolId, get_ty: &Type, n: usize) -> Vec<Type> {
        match self.st.dealias(get_ty) {
            Type::Tuple(ts) => return ts,
            Type::Class { sym, args }
                if numbered_arity(&self.st.get(sym).name, "Tuple")
                    .is_some_and(|k| k == args.len()) =>
            {
                return args;
            }
            _ => {}
        }
        let mut selectors = Vec::new();
        let mut tys = Vec::new();
        for i in 1..=crate::check::MAX_TUPLE_ARITY {
            let Some((sel, ty)) = self.nullary_member(get_ty, &format!("_{i}"), Span::DUMMY) else {
                break;
            };
            selectors.push(sel);
            tys.push(ty);
        }
        if tys.is_empty() {
            return vec![get_ty.clone()];
        }
        if tys.len() == n && !self.st.unapply_selectors.contains_key(&(unapply, n)) {
            let class = self.st.class_sym_of(get_ty).unwrap_or(SymbolId::NONE);
            let tmp = self.alloc_extractor_tmp("unapply$get", get_ty);
            self.st.unapply_selectors.insert(
                (unapply, n),
                UnapplySelectors {
                    selectors,
                    tmp,
                    class,
                },
            );
        }
        tys
    }

    /// A member `name` of `recv` callable without arguments, and its type as
    /// seen from `recv`. Alternatives that take parameters are discarded, as
    /// nsc's ad-hoc resolution of `isEmpty` / `get` does (`run/t7850d`).
    pub(crate) fn nullary_member(
        &mut self,
        recv: &Type,
        name: &str,
        span: Span,
    ) -> Option<(SymbolId, Type)> {
        let recv = self.st.dealias(recv);
        let cls = self.st.class_sym_of(&recv)?;
        let cands: Vec<SymbolId> = self
            .st
            .lookup_member(cls, name)
            .into_iter()
            .filter(|&m| matches!(self.st.get(m).kind, SymKind::Method | SymKind::Term))
            .collect();
        for &c in &cands {
            self.complete_lazy_sig(c, span);
        }
        let nullary: Vec<SymbolId> = cands
            .into_iter()
            .filter(|&m| match &self.st.get(m).ty {
                Type::Method { paramss, .. } => paramss.iter().all(|p| p.is_empty()),
                _ => true,
            })
            .collect();
        let m = *self.drop_overridden_at(cls, nullary).first()?;
        let ty = match &self.st.get(m).ty {
            Type::Method { ret, .. } => (**ret).clone(),
            t => t.clone(),
        };
        Some((m, self.st.subst_as_seen_from(&recv, &ty)))
    }

    fn alloc_extractor_tmp(&mut self, name: &str, ty: &Type) -> SymbolId {
        let id = self
            .st
            .alloc(name, self.st.owner, SymKind::Term, Flags::SYNTHETIC, "");
        self.st.get_mut(id).ty = ty.clone();
        id
    }
}
