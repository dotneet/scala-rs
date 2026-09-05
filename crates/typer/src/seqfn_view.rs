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

    /// Every wrapping `Predef` offers for an `Array[elem]`, in nsc's own
    /// implicit priority order (`Predef` before `LowPriorityImplicits` before
    /// `LowPriorityImplicits2`), each with the type the wrapped array has.
    ///
    /// Which one nsc picks is decided by the expected type, and that is not a
    /// detail: `scala.Seq` and `scala.IndexedSeq` are the *immutable* ones, so
    /// `genericWrapArray`'s `mutable.ArraySeq` does not reach them and the
    /// lowest-priority `copyArrayToImmutableIndexedSeq` is what
    /// `-Xprint:typer` shows for `def v(a: Array[Any]): Seq[Any] = a`.
    /// `scala.Iterable` is `collection.Iterable`, which the mutable one does
    /// reach, and there nsc takes `genericWrapArray`. Callers walk this list
    /// in order and keep the first whose type conforms, which reproduces both.
    ///
    /// `library_abi`-only, like [`Self::array_seq_wrap`]: the private runtime
    /// declares none of these, so the ordinary diagnostic stands there.
    pub(crate) fn array_wrap_candidates(&self, elem: &Type) -> Vec<(&'static str, Type)> {
        if !self.library_abi {
            return Vec::new();
        }
        let mut out: Vec<(&'static str, Type)> = Vec::new();
        // The `Boolean` special case this file was written for keeps its own
        // (exact `ArraySeq$ofBoolean`) declaration and stays first.
        if let Some(hit) = self.array_seq_wrap(elem) {
            out.push(hit);
        }
        if let Some(sym) =
            crate::classpath::find_by_jvm(&self.st, "scala/collection/mutable/ArraySeq")
        {
            out.push((
                "genericWrapArray",
                Type::Class {
                    sym,
                    args: vec![elem.clone()],
                },
            ));
        }
        if let Some(sym) =
            crate::classpath::find_by_jvm(&self.st, "scala/collection/immutable/IndexedSeq")
        {
            out.push((
                "copyArrayToImmutableIndexedSeq",
                Type::Class {
                    sym,
                    args: vec![elem.clone()],
                },
            ));
        }
        out.retain(|(name, _)| !self.st.lookup(name).is_empty());
        out
    }

    /// The wrapped type an `Array[elem]` can offer a `pt`-shaped position,
    /// or `None` when none of the wrappings reaches it. A pure type
    /// computation: overload resolution scores with it before any tree exists.
    pub(crate) fn array_wrap_for(&self, elem: &Type, pt: &Type) -> Option<(&'static str, Type)> {
        self.array_wrap_candidates(elem)
            .into_iter()
            .find(|(_, view)| self.st.is_sub_type(view, pt))
    }

    /// Would one of the array wrappings make an `Array` argument applicable to
    /// `param`? Overload resolution's half of [`Self::array_wrap_for`], for
    /// the case where the alternative's own type parameters are still open.
    ///
    /// `Map() ++ arr` asks `concat[B >: A](IterableOnce[B])` whether an
    /// `Array[(Int, String)]` fits, and nothing conforms to `IterableOnce[B]`
    /// while `B` is unknown. What the argument decides is the *shape*: the
    /// wrapped array is an `IterableOnce` of something, which is all an
    /// applicability test can ask before `B` is solved. `adapt` inserts the
    /// real call afterwards, against the instantiated parameter.
    pub(crate) fn array_wrap_conforms(
        &self,
        arg: &Type,
        param: &Type,
        open: &[scala_rs_parser::SymbolId],
    ) -> bool {
        let Type::Array(elem) = arg else {
            return false;
        };
        if self.array_wrap_for(elem, param).is_some() {
            return true;
        }
        if !crate::check::mentions_tparam(param, open) {
            return false;
        }
        let Some(want) = self.st.class_sym_of(param) else {
            return false;
        };
        self.array_wrap_candidates(elem)
            .into_iter()
            .any(|(_, view)| {
                std::iter::once(view.clone())
                    .chain(self.st.base_type_seq(&view))
                    .any(|b| self.st.class_sym_of(&b) == Some(want))
            })
    }

    /// Rewrites `tree` into `genericWrapArray(tree)` (or whichever wrapping
    /// [`Self::array_wrap_for`] picked) when that closes the gap to `pt`.
    /// Same contract as [`Self::coerce_array_to_function`], which it
    /// generalises; `false` leaves `tree` untouched.
    pub(crate) fn coerce_array_to_collection(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let Type::Array(elem) = tree.ty.clone() else {
            return false;
        };
        let Some((name, view)) = self.array_wrap_for(&elem, pt) else {
            return false;
        };
        self.wrap_array_call(tree, name, view, pt);
        true
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
        self.wrap_array_call(tree, name, view, pt);
        true
    }

    /// `tree` (an `Array`) becomes `Predef.<name>(tree): view`, then is
    /// re-`adapt`ed to `pt` so a further subtype check or eta-expansion still
    /// runs. The result type is written from `view` rather than read off the
    /// callee: `genericWrapArray[T]` is polymorphic, and this is the one place
    /// that knows what `T` is.
    fn wrap_array_call(&mut self, tree: &mut Tree, name: &str, view: Type, pt: &Type) {
        let Some(sym) = self.st.lookup(name).into_iter().next() else {
            return;
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
            scala_ref: false,
            stable_pat: false,
        };
        *tree = Tree {
            id: NodeId(0),
            span,
            kind: TreeKind::Apply {
                fun: Box::new(fun),
                args: vec![arg],
            },
            ty: view,
            sym,
            postfix: false,
            scala_ref: false,
            stable_pat: false,
        };
        self.adapt(tree, pt);
    }
}
