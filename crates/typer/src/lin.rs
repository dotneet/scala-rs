//! Class linearization (SLS 5.1.2), shared by the typer and the backend.
//!
//! The merge used to live only in `crates/backend/src/gen.rs`, where it drives
//! super accessors and mixin forwarders. The typer needs the *same* order to
//! decide whether an `abstract override` member ever reaches a concrete
//! implementation, so it lives here and `gen.rs` calls in.
//!
//! ## Why this is `+:` and not a C3 merge
//!
//! SLS 5.1.2 does not specify a C3 merge. It specifies a right-associative fold
//! of `+:` over the parents' linearizations, and `+:` is total — it has an
//! answer for every pair of lists, including two that order a shared ancestor
//! differently. A C3 merge does not: when no list head is free it has to guess.
//! This one guessed `lists[0][0]`, which put a shared ancestor reached at two
//! different depths in the wrong place. `class Wider extends Root with L6 with
//! L5` over `tests/fixtures/linterm_diamond.scala` came out `Wider L5 L6 L4 L3
//! L1 L2 L0` where scalac 2.13.16 says `Wider L5 L3 L6 L4 L1 L2 L0` — a clean
//! compile that ran the `super` chain in the wrong order, with no diagnostic.
//! [`Lin::lin`] now performs that fold literally; see [`prepend_replacing`].
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
//!
//! ## Why the memo outlives the call
//!
//! `Lin`'s memo used to be built and thrown away once per [`linearize`], and
//! `linearize` runs about 45 million times in a gitbucket build -- 812 million
//! `Lin::lin` node visits, the largest single entry in that profile. The memo
//! is therefore also kept on the symbol table ([`SymbolTable`]'s `lin_cache`),
//! keyed by its mutation generation: every answer stays usable until *any*
//! symbol is handed out for mutation, at which point the whole cache is
//! dropped rather than reasoned about.
//!
//! Two kinds of answer are never published. A truncated subtree, for the reason
//! above; and a class whose parent clause does not *name* its class outright
//! ([`parent_names_its_class`]), because resolving such a clause reads the
//! scopes, the projection tables or the thread-local chase guard, none of which
//! is the symbol graph -- so it can answer differently with no symbol having
//! changed. Both are tracked per node, so a clean subtree under a tainted one
//! is still kept.

use crate::symbol::SymbolTable;
use rustc_hash::{FxHashMap, FxHashSet};
use scala_rs_parser::{Flags, SymbolId, Type};

fn skip_parent(st: &SymbolTable, p: SymbolId) -> bool {
    matches!(
        st.get(p).name.as_str(),
        "Any" | "AnyRef" | "AnyVal" | "Object"
    )
}

/// Whether a parent clause names its class *outright*, so that
/// [`parents_of`]'s answer for it is a function of the symbol graph alone.
///
/// This is what makes a linearization safe to remember past the call that
/// computed it (`SymbolTable::lin_cache`). Everything else -- an unresolved
/// `Type::Named`, a type parameter, an applied alias, a projection -- is
/// resolved by [`SymbolTable::class_sym_of`] through the *scopes*, through
/// `abs_projections` / `path_members`, or through the thread-local chase guard,
/// and all three change while the typer walks the program without any symbol
/// being mutated. A class with such a parent is linearized as before and its
/// answer is simply not kept.
///
/// A function type is resolved through the JVM-name index
/// (`SymbolTable::function_class_form`), which is part of the symbol graph, so
/// it counts as named outright. So does an inner class behind a prefix, which
/// is a `Refined` around the class it names (`crate::prefix`), and an applied
/// type whose constructor names its class -- gitbucket's every table is
/// `extends profile.Table[…]`, so excluding those would have left the cache
/// doing nothing where it matters most.
fn parent_names_its_class(p: &Type) -> bool {
    match p {
        // Resolved outright, or to a field of the table that never changes.
        Type::Class { .. }
        | Type::ModuleRef(_)
        | Type::ThisType(_)
        | Type::Function { .. }
        | Type::Array(_)
        | Type::Any
        | Type::AnyRef
        | Type::AnyVal
        | Type::JavaObject
        | Type::Null
        | Type::Nothing
        | Type::Unit
        | Type::Boolean
        | Type::Byte
        | Type::Short
        | Type::Int
        | Type::Long
        | Type::Float
        | Type::Double
        | Type::Char
        | Type::String => true,
        Type::Annotated { tpe, .. } => parent_names_its_class(tpe),
        // `class_sym_of` recurses into the constructor.
        Type::Applied { ctor, .. } => parent_names_its_class(ctor),
        // `class_sym_of` takes the first parent that answers; requiring all of
        // them to be resolvable is the conservative form of that.
        Type::Refined { parents, .. } => parents.iter().all(parent_names_its_class),
        // `Named` and `Tuple` are looked up by name through the scopes;
        // `TypeParam`, `TypeMember` and `SingleType` chase bounds under the
        // thread-local `enter_chase` guard, so their answer depends on which
        // chase is already in progress. Neither is a function of the symbol
        // graph alone.
        _ => false,
    }
}

fn parents_of(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    st.get(cls)
        .parents
        .iter()
        // A parent written as a function type is `scala.FunctionN`; the
        // linearization has to contain it, or an implementation of a
        // narrowed `apply` gets no bridge for `apply(Object)Object`.
        .filter_map(|p| {
            let as_class = st.function_class_form(p);
            st.class_sym_of(as_class.as_ref().unwrap_or(p))
        })
        .filter(|p| !skip_parent(st, *p))
        .collect()
}

/// SLS 5.1.2's `+:`: concatenation in which the elements of the right operand
/// **replace** the identical elements of the left one. So `a +: b` is every
/// element of `a` that `b` does not already list, followed by all of `b`.
///
/// Two properties the linearization leans on:
///
///  * A class reachable through two parents ends up where the *right* operand
///    puts it — i.e. the earlier-written parent wins the position, because the
///    fold in [`Lin::lin`] is right-associative with the first parent innermost.
///  * The result has no duplicates as long as `a` and `b` each have none, so
///    the whole fold stays duplicate-free by induction and no repair pass is
///    needed afterwards.
///
/// That second property is load-bearing for Java interop, not just tidiness.
/// A Java class routinely re-`implements` an interface its own superclass
/// already implements — `class LinkedHashMap<K,V> extends HashMap<K,V>
/// implements Map<K,V>`. The C3 merge this replaced had no rule for that shape:
/// with no head free it fell back to `lists[0][0]`, emitting `java.util.Map` at
/// index 2 *and* again later, so `java.util.Map` preceded `java.util.HashMap`.
/// Since only a more derived base can implement a deferred member,
/// `HashMap.put` stopped counting as an implementation of `Map.put` and
/// `class Cache extends java.util.LinkedHashMap[String, Int]` was told it
/// "needs to be abstract" over eight members `HashMap` and `AbstractMap`
/// define. Under `+:` the shape needs no special case at all: `L(Map) +:
/// L(HashMap)` deletes `Map` from the left operand outright, because
/// `L(HashMap)` already lists it, and `HashMap` keeps its place. That path is
/// pinned by `crates/cli/tests/javanest.rs`.
fn prepend_replacing(a: &[SymbolId], b: Vec<SymbolId>) -> Vec<SymbolId> {
    let mut out: Vec<SymbolId> = Vec::with_capacity(a.len() + b.len());
    out.extend(a.iter().copied().filter(|x| !b.contains(x)));
    out.extend(b);
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
    /// Bumped whenever a class's parent clause does not name its class outright
    /// ([`parent_names_its_class`]). A subtree is only *rememberable past this
    /// call* while this stands still across it, for the reason given there.
    unresolved: u32,
}

impl Lin<'_> {
    fn lin(&mut self, cls: SymbolId) -> Vec<SymbolId> {
        if let Some(v) = self.memo.get(&cls.0) {
            return v.clone();
        }
        // An answer an earlier walk published. It is path-independent by
        // construction (see the module comment), so it is just as good here --
        // including for a class that is on this walk's own `path`, where it is
        // the true linearization rather than the truncation the cycle rule
        // would produce.
        if let Some(v) = self.st.cached_linearization(cls) {
            self.memo.insert(cls.0, (*v).clone());
            return (*v).clone();
        }
        if self.path.contains(&cls) {
            // A cyclic `extends` graph, or a half-resolved parent list that
            // looks like one. `cls` is already being linearized further up, so
            // treat it here as having no parents; the cycle itself gets its
            // own diagnostic (`inheritance_cycle`).
            self.truncations += 1;
            return vec![cls];
        }
        let before = (self.truncations, self.unresolved);
        if self
            .st
            .get(cls)
            .parents
            .iter()
            .any(|p| !parent_names_its_class(p))
        {
            self.unresolved += 1;
        }
        let parents = parents_of(self.st, cls);
        self.path.push(cls);
        let lins: Vec<Vec<SymbolId>> = parents.iter().map(|&p| self.lin(p)).collect();
        self.path.pop();
        // SLS 5.1.2 verbatim: `L(C) = C, L(Cn) +: … +: L(C1)`, where `+:` is
        // right-associative. So `L(C1)` — the *first* parent — is the innermost
        // operand, and each later parent's linearization is prepended to what
        // the earlier ones already built. Folding left over the parents in
        // source order with `acc = L(Ci) +: acc` is exactly that fold.
        let mut acc: Vec<SymbolId> = Vec::new();
        for l in &lins {
            acc = prepend_replacing(l, acc);
        }
        let mut out = vec![cls];
        // `cls` heads its own linearization; a cyclic `extends` graph that
        // reaches it again must not list it twice.
        out.extend(acc.into_iter().filter(|&b| b != cls));
        // Both counters have to stand still across the subtree: one answer
        // that truncated at a cycle, or one parent clause resolved through
        // something other than the symbol graph, taints everything built on it.
        if (self.truncations, self.unresolved) == before {
            self.memo.insert(cls.0, out.clone());
        }
        out
    }
}

/// `cls`'s linearization, and whether it may be remembered past this call:
/// it may when nothing in the walk truncated at a cycle and every parent clause
/// it read named its class outright ([`parent_names_its_class`]).
///
/// Callers that cache something of their own derived from the same walk --
/// [`SymbolTable::base_type_args`] -- have to honour the same verdict.
pub fn linearize_settled(st: &SymbolTable, cls: SymbolId) -> (Vec<SymbolId>, bool) {
    if let Some(v) = st.cached_linearization(cls) {
        return ((*v).clone(), true);
    }
    let mut walk = Lin {
        st,
        memo: FxHashMap::default(),
        path: Vec::new(),
        truncations: 0,
        unresolved: 0,
    };
    let out = walk.lin(cls);
    // `memo` holds exactly the nodes whose whole subtree stood still, which is
    // also exactly what may be published; `cls` is among them when its own
    // answer is settled.
    let settled = walk.memo.contains_key(&cls.0);
    st.publish_linearizations(walk.memo);
    (out, settled)
}

/// `cls` itself first, then its ancestors most-derived first (SLS 5.1.2).
/// `Any` / `AnyRef` / `Object` are not included.
///
/// Total on every symbol graph: a cyclic `extends` chain truncates instead of
/// diverging.
pub fn linearize(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    linearize_settled(st, cls).0
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

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::symbol::SymKind;

    fn class(st: &mut SymbolTable, name: &str) -> SymbolId {
        st.alloc(name, st.root, SymKind::Class, Flags::EMPTY, name)
    }

    /// The cache must follow a parent list that changes. It is keyed by the
    /// symbol table's mutation generation, which `get_mut` bumps, so the second
    /// answer is recomputed rather than served.
    #[test]
    fn a_changed_parent_list_is_not_served_from_the_cache() {
        let mut st = SymbolTable::new();
        let base = class(&mut st, "Base");
        let mid = class(&mut st, "Mid");
        let leaf = class(&mut st, "Leaf");
        st.get_mut(leaf).parents.push(Type::Class {
            sym: mid,
            args: vec![],
        });
        assert_eq!(linearize(&st, leaf), vec![leaf, mid]);
        // Cached now; adding a parent to `Mid` must still be seen.
        st.get_mut(mid).parents.push(Type::Class {
            sym: base,
            args: vec![],
        });
        assert_eq!(linearize(&st, leaf), vec![leaf, mid, base]);
    }

    /// A class whose parent is still an unresolved name is linearized as before
    /// but never remembered: the name is resolved through the scopes, and
    /// entering it there mutates no symbol.
    #[test]
    fn an_unresolved_parent_is_not_remembered() {
        let mut st = SymbolTable::new();
        let base = class(&mut st, "Base");
        let leaf = class(&mut st, "Leaf");
        st.get_mut(leaf).parents.push(Type::Named {
            name: "Base".into(),
            args: vec![],
        });
        // Nothing binds `Base` as a type yet, so the parent resolves to nothing.
        assert_eq!(linearize(&st, leaf), vec![leaf]);
        assert!(!linearize_settled(&st, leaf).1);
        assert!(st.cached_linearization(leaf).is_none());
        // Entering the name in scope is not a symbol mutation, and the answer
        // has to change anyway.
        st.enter_in_current("Base", base);
        assert_eq!(linearize(&st, leaf), vec![leaf, base]);
    }

    /// A cyclic `extends` graph still truncates, and still publishes nothing.
    #[test]
    fn a_cycle_is_not_remembered() {
        let mut st = SymbolTable::new();
        let x = class(&mut st, "X");
        let y = class(&mut st, "Y");
        st.get_mut(x).parents.push(Type::Class {
            sym: y,
            args: vec![],
        });
        st.get_mut(y).parents.push(Type::Class {
            sym: x,
            args: vec![],
        });
        assert_eq!(linearize(&st, x), vec![x, y]);
        assert!(!linearize_settled(&st, x).1);
        assert!(st.cached_linearization(x).is_none());
    }
}
