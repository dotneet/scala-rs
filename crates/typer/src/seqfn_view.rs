//! `Array[Boolean]` seen as `Int => Boolean` (agent/seqfn).
//!
//! nsc:
//!
//! ```scala
//! val sieve = Array.fill(31)(true)
//! (2 to 30).filter(sieve)   // Array[Boolean], not a function -- but
//!                           // `Predef.wrapBooleanArray` turns it into a
//!                           // `mutable.ArraySeq[Boolean]`, and `Seq[A] <:
//!                           // PartialFunction[Int, A] <: Int => A`
//!                           // (`prelude_seqfn.rs`) does the rest.
//! ```
//!
//! `views.rs`'s `conversion_view` does not reach this: it searches for an
//! *implicit* `A => B`, and `wrapBooleanArray` is deliberately not `implicit`
//! in the prelude (`prelude_seqfn::add_wrap_boolean_array` -- marking it
//! `implicit` would make it compete with `booleanArrayOps` for ordinary
//! `Array[Boolean]` method calls, the same reason `wrapIntArray` is not
//! `implicit` either). So this is its own small, targeted view: `arg_score`
//! asks [`Typer::array_seq_wrap`] whether an `Array` argument *could* satisfy
//! a function-shaped parameter after wrapping (a pure type computation, no
//! tree involved -- overload resolution only scores alternatives), and once
//! an alternative is picked, `adapt`'s [`Typer::coerce_array_to_function`]
//! builds the real `wrapBooleanArray(arr)` call.
//!
//! `library_abi`-only: `array_seq_wrap` answers `None` under
//! `--no-scala-library` because `add_wrap_boolean_array` never declares the
//! method there, so the existing `type mismatch` diagnostic stands.

use scala_rs_parser::{NodeId, Tree, TreeKind, Type};

use crate::check::Typer;

impl Typer {
    /// The type `Array[elem]` becomes through `Predef.wrapBooleanArray`,
    /// together with that method's name -- `elem` is `Boolean`, the one
    /// element type this slice backs, and `--scala-library` is in effect.
    /// `None` otherwise, in which case the caller falls through to whatever
    /// it would have done before this file existed.
    pub(crate) fn array_seq_wrap(&self, elem: &Type) -> Option<(&'static str, Type)> {
        if !self.library_abi || !matches!(elem, Type::Boolean) {
            return None;
        }
        let sym = crate::classpath::find_by_jvm(&self.st, "scala/collection/mutable/ArraySeq")?;
        Some((
            "wrapBooleanArray",
            Type::Class {
                sym,
                args: vec![Type::Boolean],
            },
        ))
    }

    /// Rewrites `tree` (already known not to conform to `pt` directly) into
    /// `wrapBooleanArray(tree)` when that closes the gap, and re-`adapt`s the
    /// result (so a further eta-expansion or subtype check still runs).
    /// Leaves `tree` untouched and returns `false` for every other shape --
    /// same contract as `adapt_to_sam`.
    pub(crate) fn coerce_array_to_function(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let Type::Array(elem) = tree.ty.clone() else {
            return false;
        };
        let Some((name, view)) = self.array_seq_wrap(&elem) else {
            return false;
        };
        if !self.st.is_sub_type(&view, pt) {
            return false;
        }
        let Some(sym) = self.st.lookup(name).into_iter().next() else {
            return false;
        };
        let span = tree.span;
        let arg = std::mem::replace(tree, Tree::dummy(TreeKind::Empty));
        let fun_ty = self.st.get(sym).ty.clone();
        let fun = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Ident { name: name.into() },
            ty: fun_ty,
            sym,
            postfix: false,
        };
        *tree = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(fun),
                args: vec![arg],
            },
            ty: view.clone(),
            sym,
            postfix: false,
        };
        self.adapt(tree, pt);
        true
    }
}
