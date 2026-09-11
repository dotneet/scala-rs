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
    pub(crate) fn unapply_pattern_types(
        &mut self,
        unapply: SymbolId,
        sel_ty: &Type,
        n: usize,
        span: Span,
        report: bool,
    ) -> Option<Vec<Type>> {
        // A case class's synthetic `unapply` is compiled as its constructor
        // pattern (`gen_ctor_fields_pattern`), one sub-pattern per field; nsc
        // does the same and reports a wrong count as an arity error.
        if self.st.get(unapply).flags.contains(Flags::CASE) {
            let extracted = self.unapply_extracted_types(unapply);
            return Some(self.subst_unapply_tparams(unapply, sel_ty, extracted));
        }
        let ret = self.st.get(unapply).ty.result().clone();
        let ret = self
            .subst_unapply_tparams(unapply, sel_ty, vec![ret.clone()])
            .pop()
            .unwrap_or(ret);
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
            // (`neg/t6675`). Only a tuple: a class with product selectors of
            // its own is bound whole without a word.
            if report {
                if let Some(k) = crate::check::tuple_arity(&self.st, &self.st.dealias(&get_ty))
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
