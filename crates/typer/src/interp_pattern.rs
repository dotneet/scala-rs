//! Interpolated string patterns: `case s"$a-$b" =>`.
//!
//! The parser gives nsc's tree for them (`interpolatedString(inPattern =
//! true)`): the extractor pattern `StringContext("", "-", "").s(a, b)`, whose
//! function is a *selection* on a value rather than a stable path.

use crate::check::*;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;

impl Typer {
    /// nsc types a constructor pattern's function in pattern-constructor mode,
    /// where only a value -- an object, or what a parameterless member
    /// returns -- can be the extractor. `StringContext` declares both
    /// `def s(args: Any*): String` and `object s { def unapplySeq(..) }`, and
    /// `case s"..."` means the object; a user interpolator may be any
    /// parameterless member (`implicit class X(sc: StringContext) { def x =
    /// Some }`). An `Ident` function gets the same rule from
    /// `ctor_pattern_fun`; this is it for a selection, applied to the selected
    /// name only (its qualifier is an ordinary expression, evaluated at run
    /// time exactly once, as nsc's is).
    ///
    /// The object inside a library class is supplied from its pickle as an
    /// accessor method (see `PickleSupply::install_nested_module`), and so is
    /// its `unapply` / `unapplySeq`: the hand-written prelude declares the
    /// `StringContext` class with its `s` method only.
    pub(crate) fn prefer_extractor_object(&mut self, fun: &mut Tree) {
        let TreeKind::Select { qual, name } = &fun.kind else {
            return;
        };
        if !fun.sym.is_none() && self.st.get(fun.sym).kind != SymKind::Method {
            return;
        }
        let Some(cls) = self.st.class_sym_of(&qual.ty) else {
            return;
        };
        let name = name.clone();
        let parameterless = |st: &crate::symbol::SymbolTable, m: SymbolId| match st.get(m).kind {
            SymKind::Module => true,
            SymKind::Method => match &st.get(m).ty {
                Type::Method { paramss, .. } => paramss.iter().all(|c| c.is_empty()),
                _ => true,
            },
            _ => false,
        };
        if !fun.sym.is_none() && parameterless(&self.st, fun.sym) {
            self.settle_extractor_value(fun);
            return;
        }
        let mut alts = self.st.lookup_member(cls, &name);
        if !alts.iter().any(|&m| parameterless(&self.st, m)) && self.library_abi {
            alts.extend(
                self.pickle
                    .complete(&mut self.st, &mut self.binary, cls, &name),
            );
        }
        if let Some(m) = alts.into_iter().find(|&m| parameterless(&self.st, m)) {
            fun.sym = m;
            fun.ty = self.st.get(m).ty.clone();
            self.settle_extractor_value(fun);
        }
    }

    /// `fun` now names a value; its type is what the pattern extracts with.
    /// Make sure that class's `unapply` / `unapplySeq` is in the table.
    fn settle_extractor_value(&mut self, fun: &mut Tree) {
        // `def x = { println("x"); Some }` has no written result type.
        if !fun.sym.is_none() && self.st.get(fun.sym).kind == SymKind::Method {
            self.complete_lazy_sig(fun.sym, fun.span);
            if fun.ty.is_no_type() || matches!(fun.ty, Type::Method { .. }) {
                fun.ty = self.st.get(fun.sym).ty.clone();
            }
        }
        if let Type::Method { paramss, ret } = &fun.ty {
            if paramss.iter().all(|c| c.is_empty()) {
                fun.ty = (**ret).clone();
            }
        }
        let Some(ext) = self.st.class_sym_of(&fun.ty) else {
            return;
        };
        if !self.library_abi {
            return;
        }
        for m in ["unapply", "unapplySeq"] {
            if self.st.lookup_member(ext, m).is_empty() {
                self.pickle.complete(&mut self.st, &mut self.binary, ext, m);
            }
        }
    }
}
