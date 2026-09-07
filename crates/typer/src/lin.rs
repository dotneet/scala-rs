//! Class linearization (SLS 5.1.2), shared by the typer and the backend.
//!
//! The C3 merge used to live only in `crates/backend/src/gen.rs`, where it
//! drives super accessors and mixin forwarders. The typer needs the *same*
//! order to decide whether an `abstract override` member ever reaches a
//! concrete implementation, so it lives here and `gen.rs` calls in.
//!
//! ## Why the walk carries a path and a memo
//!
//! The recursion used to be bounded by a depth counter alone (`depth > 64`).
//! A depth bound bounds the *depth* of the recursion tree, not its size: with
//! two parents on a cycle the tree is `2^64` nodes, so
//!
//! ```text
//! trait X extends Y with Z
//! trait Y extends Z
//! trait Z extends X
//! ```
//!
//! — three lines that real scalac rejects in under two seconds — spun at 100%
//! CPU with flat memory for as long as it was left running. Three compiler
//! processes were found in that state after four to five and a half hours, and
//! a 41-file subset of cats reproduced it in under a second of startup.
//!
//! [`Lin::lin`] therefore carries the recursion *path* and stops the moment it
//! re-enters a class it is already linearizing. That is a termination
//! guarantee for any symbol graph whatsoever, including one this compiler
//! built wrongly; it does not depend on the graph being well formed.
//!
//! The memo is what makes the walk cheap rather than merely finite: `lin` is a
//! pure function of its class, so a diamond that used to be re-expanded once
//! per path is now computed once. A subtree that *did* truncate at the path is
//! deliberately **not** memoised — that result depends on where the walk came
//! from, and caching it would leak one caller's truncation into another's
//! answer.
//!
//! For a well-formed (acyclic) hierarchy the result is unchanged: no
//! truncation can happen, and memoising a pure function is not observable.

use crate::symbol::SymbolTable;
use rustc_hash::{FxHashMap, FxHashSet};
use scala_rs_parser::{Flags, SymbolId};

fn skip_parent(st: &SymbolTable, p: SymbolId) -> bool {
    matches!(
        st.get(p).name.as_str(),
        "Any" | "AnyRef" | "AnyVal" | "Object"
    )
}

fn parents_of(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    st.get(cls)
        .parents
        .iter()
        // A parent written as a function type is `scala.FunctionN`; the
        // linearization has to contain it, or an implementation of a
        // narrowed `apply` gets no bridge for `apply(Object)Object`.
        .filter_map(|p| st.class_sym_of(&st.function_class_form(p).unwrap_or_else(|| p.clone())))
        .filter(|p| !skip_parent(st, *p))
        .collect()
}

fn c3_merge(mut lists: Vec<Vec<SymbolId>>) -> Vec<SymbolId> {
    let mut out = Vec::new();
    loop {
        lists.retain(|l| !l.is_empty());
        if lists.is_empty() {
            break;
        }
        let mut chosen = None;
        for l in &lists {
            let h = l[0];
            let in_tail = lists.iter().any(|o| o.iter().skip(1).any(|&x| x == h));
            if !in_tail {
                chosen = Some(h);
                break;
            }
        }
        let h = match chosen {
            Some(h) => h,
            None => lists[0][0],
        };
        out.push(h);
        for l in &mut lists {
            if l.first() == Some(&h) {
                l.remove(0);
            }
        }
    }
    out
}

/// The linearization walk: SLS 5.1.2's recursion, plus the two pieces of
/// bookkeeping that make it total and linear (see the module comment).
struct Lin<'a> {
    st: &'a SymbolTable,
    /// A class's linearization, for the subtrees that never truncated.
    memo: FxHashMap<u32, Vec<SymbolId>>,
    /// The classes currently being linearized, outermost first.
    path: Vec<SymbolId>,
    /// Bumped whenever the walk truncates at a class already on `path`. A
    /// subtree is only memoisable while this stands still across it.
    truncations: u32,
}

impl Lin<'_> {
    fn lin(&mut self, cls: SymbolId) -> Vec<SymbolId> {
        if let Some(v) = self.memo.get(&cls.0) {
            return v.clone();
        }
        if self.path.contains(&cls) {
            // A cyclic `extends` graph, or a half-resolved parent list that
            // looks like one. `cls` is already being linearized further up, so
            // treat it here as having no parents; the cycle itself gets its
            // own diagnostic (`inheritance_cycle`).
            self.truncations += 1;
            return vec![cls];
        }
        let before = self.truncations;
        let parents = parents_of(self.st, cls);
        self.path.push(cls);
        let mut lists: Vec<Vec<SymbolId>> = parents.iter().rev().map(|&p| self.lin(p)).collect();
        self.path.pop();
        lists.push(parents.iter().rev().copied().collect());
        let mut out = vec![cls];
        // `cls` heads its own linearization; a cyclic `extends` graph that
        // reaches it again must not list it twice.
        out.extend(
            dedup_keep_last(c3_merge(lists))
                .into_iter()
                .filter(|&b| b != cls),
        );
        if self.truncations == before {
            self.memo.insert(cls.0, out.clone());
        }
        out
    }
}

/// Drop every repeat of a class, keeping its **last** position.
///
/// SLS 5.1.2 builds `L(C) = C, L(Cn) +: … +: L(C1)`, and `a +: b` deletes from
/// `a` whatever `b` already lists — so when a class is reachable through two
/// parents, the *later* list decides where it sits. `c3_merge` above cannot
/// always honour that: when the two parents impose contradictory orders it
/// falls back to `lists[0][0]` and emits the class again later from the list
/// that really owns it.
///
/// Java's collections hit this constantly, because a Java class re-`implements`
/// an interface its own superclass already implements:
/// `class LinkedHashMap<K,V> extends HashMap<K,V> implements Map<K,V>`. The
/// fallback put `java.util.Map` at index 2 and `java.util.HashMap` at index 3,
/// and since only a *more derived* base can implement a deferred member,
/// `HashMap.put` no longer counted as implementing `Map.put` —
/// `class Cache extends java.util.LinkedHashMap[String, Int]` was told it
/// "needs to be abstract" over eight members `HashMap` and `AbstractMap`
/// define. Keeping the last occurrence is precisely `+:`, and it also removes
/// the duplicates, which nothing downstream wants.
fn dedup_keep_last(v: Vec<SymbolId>) -> Vec<SymbolId> {
    let mut out: Vec<SymbolId> = Vec::with_capacity(v.len());
    for (i, &x) in v.iter().enumerate() {
        if !v[i + 1..].contains(&x) {
            out.push(x);
        }
    }
    out
}

/// `cls` itself first, then its ancestors most-derived first (SLS 5.1.2).
/// `Any` / `AnyRef` / `Object` are not included.
///
/// Total on every symbol graph: a cyclic `extends` chain truncates instead of
/// diverging.
pub fn linearize(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    Lin {
        st,
        memo: FxHashMap::default(),
        path: Vec::new(),
        truncations: 0,
    }
    .lin(cls)
}

/// True for a `trait` (source) or a class-file / pickle `interface`.
pub fn is_interface(st: &SymbolTable, id: SymbolId) -> bool {
    let f = st.get(id).flags;
    f.contains(Flags::TRAIT) || f.contains(Flags::INTERFACE)
}

/// SLS 5.3.3: the superclass a `trait` constrains its mixers to. `trait T
/// extends C` names it directly; `trait U extends T` inherits `C` through `T`.
/// The result is the *most derived* class in `id`'s linearization, ignoring
/// `AnyRef`, or `None` when the trait constrains nothing.
pub fn trait_superclass(st: &SymbolTable, id: SymbolId) -> Option<SymbolId> {
    if !is_interface(st, id) {
        return None;
    }
    linearize(st, id)
        .into_iter()
        .find(|&s| !is_interface(st, s) && !skip_parent(st, s))
}

/// A cycle in the `extends` graph, described the way scalac 2.13.16 reports it.
///
/// ```text
/// CycW.scala:4: error: illegal cyclic reference involving trait X
/// trait Z extends X
///         ^
/// ```
///
/// nsc completes a class by completing the parents it names, in the order they
/// are written, and raises `CyclicReference` when it re-enters a class whose
/// completion is still in progress. It therefore reports at the template that
/// *closes* the cycle ([`InheritanceCycle::closing`], `Z` above) and names the
/// class whose completion was re-entered ([`InheritanceCycle::involving`], `X`).
pub struct InheritanceCycle {
    /// The class whose parent list closes the cycle. scalac reports there.
    pub closing: SymbolId,
    /// The class the cycle re-enters. scalac names this one.
    pub involving: SymbolId,
}

/// The first cycle nsc's completion order reaches from `root`, as the list of
/// classes on it: the re-entered class first, the class that closes it last.
///
/// Terminates on every graph — `path` stops a cycle and `done` stops a
/// diamond, so each class is entered at most once.
fn first_cycle_from(st: &SymbolTable, root: SymbolId) -> Option<Vec<SymbolId>> {
    fn go(
        st: &SymbolTable,
        cls: SymbolId,
        path: &mut Vec<SymbolId>,
        done: &mut FxHashSet<u32>,
    ) -> Option<Vec<SymbolId>> {
        path.push(cls);
        for p in parents_of(st, cls) {
            if let Some(at) = path.iter().position(|&x| x == p) {
                let cycle = path[at..].to_vec();
                path.pop();
                return Some(cycle);
            }
            if done.contains(&p.0) {
                continue;
            }
            if let Some(hit) = go(st, p, path, done) {
                path.pop();
                return Some(hit);
            }
        }
        path.pop();
        done.insert(cls.0);
        None
    }
    if root.is_none() {
        return None;
    }
    go(st, root, &mut Vec::new(), &mut FxHashSet::default())
}

/// The `extends` cycle reachable from `cls`, named and placed the way scalac
/// would.
///
/// The answer must not depend on which member of the cycle asked, or the same
/// cycle would be reported once per class on it, each time naming a different
/// class. So the walk is re-run from a canonical entry: the least symbol on the
/// cycle, which — symbols being numbered as the namer meets them — is the class
/// nsc's own source-order completion would have locked first.
pub fn inheritance_cycle(st: &SymbolTable, cls: SymbolId) -> Option<InheritanceCycle> {
    let found = first_cycle_from(st, cls)?;
    let entry = *found.iter().min_by_key(|s| s.0)?;
    let canonical = first_cycle_from(st, entry)?;
    Some(InheritanceCycle {
        closing: *canonical.last()?,
        involving: *canonical.first()?,
    })
}
