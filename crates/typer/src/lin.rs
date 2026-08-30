//! Class linearization (SLS 5.1.2), shared by the typer and the backend.
//!
//! The C3 merge used to live only in `crates/backend/src/gen.rs`, where it
//! drives super accessors and mixin forwarders. The typer needs the *same*
//! order to decide whether an `abstract override` member ever reaches a
//! concrete implementation, so it lives here and `gen.rs` calls in.

use crate::symbol::SymbolTable;
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

fn lin(st: &SymbolTable, cls: SymbolId, depth: u32) -> Vec<SymbolId> {
    // A cyclic `extends` graph gets its own diagnostic; do not hang here.
    if depth > 64 {
        return vec![cls];
    }
    let parents = parents_of(st, cls);
    let mut lists: Vec<Vec<SymbolId>> = parents
        .iter()
        .rev()
        .map(|p| lin(st, *p, depth + 1))
        .collect();
    lists.push(parents.iter().rev().copied().collect());
    let mut out = vec![cls];
    // `cls` heads its own linearization; a cyclic `extends` graph that reaches
    // it again must not list it twice.
    out.extend(
        dedup_keep_last(c3_merge(lists))
            .into_iter()
            .filter(|&b| b != cls),
    );
    out
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
pub fn linearize(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    lin(st, cls, 0)
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
