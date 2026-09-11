//! The collection a transformation really returns.
//!
//! 2.13 declares every transformation on `IterableOps[+A, +CC[_], +C]` to
//! return `C` (`filter`, `take`, `tail`, …) or `CC[B]` (`map`, `collect`,
//! `flatMap`, `zip`, …). The typer's `BuildFrom` rebuild
//! (`check_apply::rebuild_from_receiver` and its callers) put the *receiver's
//! own class* back into such a result, which is right for `Vector` and
//! `List` and wrong wherever a class binds `CC` / `C` to something else:
//!
//! * `NumericRange[T] extends … IndexedSeqOps[T, IndexedSeq, IndexedSeq[T]]`
//!   -- `(1L to 5L).map(_ * 2)` is an `IndexedSeq[Long]` (a `Vector`);
//! * `IndexedSeqView[A] extends IndexedSeqOps[A, View, View[A]]` --
//!   `v.filter(p)` is a `View[A]` (a `View$Filter`);
//! * `SortedSet[A]`'s `CC` is `Set`: `map` without an `Ordering` builds a
//!   plain `Set`.
//!
//! Narrowing those results to the receiver's class compiled, and the
//! `checkcast` codegen puts on the result threw `ClassCastException` at run
//! time. The pickle of the receiver's class says what `CC` and `C` are:
//! walk its linearization to `IterableOps` (substituting as it goes) and read
//! them off. Only a one-parameter `scala.collection` class is asked; maps
//! (`MapOps` has a `CC` of its own) and everything without a pickle keep the
//! receiver's class, as before.

use crate::check::Typer;
use crate::symbol::OpsShape;
use scala_rs_parser::ast::Type;
use scala_rs_parser::SymbolId;

/// Which of `IterableOps`' type constructors a member returns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OpsSlot {
    /// `CC[B]`: `map`, `flatMap`, `collect`, `zip`, `zipWithIndex`, …
    Cc,
    /// `C`: `filter`, `take`, `tail`, `reverse`, `partition`'s halves, …
    C,
}

impl Typer {
    /// Read (once) the `IterableOps` shape of the collection class `recv_ty`
    /// is, so the `&self` rebuild helpers can consult it.
    pub(crate) fn prime_ops_shape(&mut self, recv_ty: &Type) {
        if !self.library_abi {
            return;
        }
        let Some(c) = self.st.class_sym_of(recv_ty) else {
            return;
        };
        let r = self.collection_root(c);
        if self.st.ops_shapes.contains_key(&r) {
            return;
        }
        let jvm = self.st.get(r).jvm_name.clone();
        let shape = if jvm.starts_with("scala/collection/") && self.st.get(r).tparams.len() == 1 {
            self.pickle
                .iterable_ops_classes(&mut self.binary, &jvm)
                .and_then(|(cc, c)| {
                    let find =
                        |n: &str| crate::classpath::find_by_jvm(&self.st, &n.replace('.', "/"));
                    Some(OpsShape {
                        cc: find(&cc)?,
                        c: find(&c)?,
                    })
                })
        } else {
            None
        };
        self.st.ops_shapes.insert(r, shape);
    }

    /// The class a member returning `slot` yields on a collection whose own
    /// class is `r`: what the pickle binds, or `r` itself when that is not
    /// known.
    pub(crate) fn ops_target(&self, r: SymbolId, slot: OpsSlot) -> SymbolId {
        match self.st.ops_shapes.get(&r).copied().flatten() {
            Some(s) => match slot {
                OpsSlot::Cc => s.cc,
                OpsSlot::C => s.c,
            },
            None => r,
        }
    }
}
