//! Cycle detection in type resolution — nsc's `checkNonCyclic`, and the one
//! rule that keeps erasure's value-class unboxing well-founded.
//!
//! ## Why this module exists
//!
//! Before it, a cyclic type reference did not produce a diagnostic: it
//! recursed until the 512 MB compiler stack ran out and the process aborted.
//! scala/scala's own corpus found eight of them (`tests/scala_corpus.sh`;
//! `docs/scala-corpus.md` lists the names), and the same failure mode had
//! already cost two whole benchmark measurements — gitbucket's `JGitUtil.scala`
//! and cats-core's 244 files both reported `errors=0 classes=0`, which reads
//! like success.
//!
//! ## What nsc does, and what is reproduced here
//!
//! nsc sets a `LOCKED` flag on a symbol while it completes its info and throws
//! `CyclicReference` when the symbol is re-entered. That gives two diagnostics
//! for `def f[A <: A](x: A) = x`, both read off real scalac 2.13.16:
//!
//! ```text
//! error: illegal cyclic reference involving type A     // at the use, `x: A`
//! error: cyclic aliasing or subtyping involving type A // at the definition
//! ```
//!
//! [`bound_cycles`] reports the second one. The first belongs to the use site
//! and is not reproduced; one diagnostic is enough to reject the program, and
//! reporting a second at every mention would be noise.
//!
//! Aliases (`type U = U`, `type X = List[X]`) were already covered, by
//! `check::expand_one_alias`, which is why this module only looks at *bounds*.
//!
//! ## What must stay legal
//!
//! A bound may name the type it bounds — F-bounded polymorphism is the whole
//! point of `trait Ord[A <: Ord[A]]`, and `type X <: List[X]` is accepted by
//! scalac too. What is rejected is a bound whose **head** is the bounded type,
//! directly (`type T <: T`, `def f[A[X] <: A[X]]`) or through other abstract
//! types (`type X <: Y; type Y <: X`). Those say nothing at all, and every
//! walk that replaces an abstract type by its bound diverges on them.
//!
//! So the walk here follows heads only: it steps through `Applied`, an
//! annotation and the parents of a compound, and it stops at a class. It never
//! descends into type *arguments*, which is exactly what keeps `Ord[A <: Ord[A]]`
//! legal. Every line above was checked against `/tmp/scala-2.13.16/bin/scalac`.

use rustc_hash::FxHashSet;
use scala_rs_parser::{SymbolId, Type};

use crate::symbol::SymbolTable;

/// The abstract types a bound *leads to*: the head of the type, of every
/// parent of a compound, and of what an application applies. Type arguments
/// are deliberately not visited.
fn heads(ty: &Type, out: &mut Vec<SymbolId>) {
    match ty {
        Type::TypeParam(id) | Type::TypeMember(id) => out.push(*id),
        Type::Applied { ctor, .. } => heads(ctor, out),
        Type::Annotated { tpe, .. } => heads(tpe, out),
        Type::Refined { parents, .. } => {
            for p in parents {
                heads(p, out);
            }
        }
        _ => {}
    }
}

/// Where an abstract type continues the walk: its right-hand side when it is
/// an alias, and its bounds when it is not. A placeholder right-hand side
/// (`TypeMember(self)`, what an abstract member's own type is) is not an
/// alias.
fn successors(st: &SymbolTable, id: SymbolId, out: &mut Vec<SymbolId>) {
    let info = st.get(id);
    let is_alias = match &info.ty {
        Type::NoType | Type::Error => false,
        Type::TypeMember(x) | Type::TypeParam(x) => *x != id,
        _ => true,
    };
    if is_alias {
        heads(&info.ty, out);
        return;
    }
    if let Some(hi) = &info.bound_hi {
        heads(hi, out);
    }
    if let Some(lo) = &info.bound_lo {
        heads(lo, out);
    }
}

/// Does `start`'s own bound lead back to `start`?
pub fn is_cyclic_bound(st: &SymbolTable, start: SymbolId) -> bool {
    if start.is_none() {
        return false;
    }
    let mut stack: Vec<SymbolId> = Vec::new();
    successors(st, start, &mut stack);
    let mut seen: FxHashSet<u32> = FxHashSet::default();
    while let Some(s) = stack.pop() {
        if s == start {
            return true;
        }
        if !seen.insert(s.0) {
            continue;
        }
        successors(st, s, &mut stack);
    }
    false
}

/// The type parameters and abstract type members among `ids` whose bounds are
/// cyclic, with the message scalac 2.13.16 prints for each.
///
/// Reported once per definition; the caller supplies the span and is expected
/// to drop the bound afterwards, so that the walks downstream have nothing
/// left to diverge on.
pub fn bound_cycles(st: &SymbolTable, ids: &[SymbolId]) -> Vec<(SymbolId, String)> {
    let mut out = Vec::new();
    for &id in ids {
        if id.is_none() {
            continue;
        }
        if st.get(id).bound_hi.is_none() && st.get(id).bound_lo.is_none() {
            continue;
        }
        if is_cyclic_bound(st, id) {
            out.push((
                id,
                format!(
                    "cyclic aliasing or subtyping involving type {}",
                    st.get(id).name
                ),
            ));
        }
    }
    out
}

/// Does this type stand for a user-defined value class?
///
/// A compound counts when *any* parent does (`class VB(val x: VA with Tr)` is
/// rejected and so is `Tr with VA`), and an abstract type counts when its
/// upper bound does (`class B[T <: A](val a: T)`). All three were read off
/// real scalac 2.13.16, not assumed.
fn is_value_class_type(st: &SymbolTable, ty: &Type, seen: &mut Vec<u32>) -> bool {
    match ty {
        Type::Class { sym, .. } => st.is_value_class(*sym),
        Type::Named { .. } => st.class_sym_of(ty).is_some_and(|c| st.is_value_class(c)),
        Type::Annotated { tpe, .. } => is_value_class_type(st, tpe, seen),
        Type::Applied { ctor, .. } => is_value_class_type(st, ctor, seen),
        Type::Refined { parents, .. } => parents.iter().any(|p| is_value_class_type(st, p, seen)),
        Type::TypeParam(id) | Type::TypeMember(id) => {
            if seen.contains(&id.0) {
                return false;
            }
            seen.push(id.0);
            match st.get(*id).bound_hi.clone() {
                Some(hi) => is_value_class_type(st, &hi, seen),
                None => false,
            }
        }
        _ => false,
    }
}

/// nsc's `validateDerivedValueClass`, the "may not wrap" half of it.
///
/// A value class erases to what it wraps, so a value class that wraps another
/// one has no erasure at all: `case class Foo(x: Bar) extends AnyVal` beside
/// `case class Bar(x: Foo) extends AnyVal` (`neg/t5878`) unfolds for ever.
/// nsc rejects the pair rather than trying; so does this.
pub fn value_class_wraps_value_class(st: &SymbolTable, class_id: SymbolId) -> Option<String> {
    let underlying = st.value_class_underlying(class_id)?;
    let mut seen = vec![];
    if is_value_class_type(st, &underlying, &mut seen) {
        return Some("value class may not wrap another user-defined value class".to_string());
    }
    None
}
