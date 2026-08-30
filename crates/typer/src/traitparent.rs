//! `trait T extends C` — a trait whose parent is a *class* (SLS 5.3.3), and
//! the `abstract override` / stackable-trait rules that go with it.
//!
//! A trait's class parent is a **constraint**, not an initialisation: `T` never
//! runs `C`'s constructor, so the parent takes no argument list, and only a
//! class that already is a `C` may mix `T` in. `abstract override def m`
//! resolves `super.m` along the *linearization* of the concrete class, so it
//! is only legal when some class further down that linearization really does
//! implement `m`.
//!
//! Everything here is a pure function of the symbol table so `check.rs` only
//! grows call sites. The diagnostics reproduce scalac 2.13.16's wording, which
//! was read off the real compiler (see `crates/cli/tests/traitextends.rs`).

use crate::lin::{is_interface, linearize, trait_superclass};
use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// A method that is `abstract override`: it defers to `super`, which is only
/// bound once the concrete class fixes the linearization.
fn is_abstract_override(st: &SymbolTable, m: SymbolId) -> bool {
    let s = st.get(m);
    // `Symbol::abstract_override`, not the flags: the namer sets `ABSTRACT` on
    // every body-less `def`, so slick's deferred `override def close(): Unit`
    // would otherwise look exactly like a stackable `abstract override`.
    s.kind == SymKind::Method && s.name != "<init>" && s.abstract_override
}

fn is_concrete_method(st: &SymbolTable, m: SymbolId) -> bool {
    let s = st.get(m);
    s.kind == SymKind::Method && s.name != "<init>" && !s.flags.contains(Flags::ABSTRACT)
}

/// The declaration scalac echoes back: `def speak: String`, or
/// `def f(x: Int): String` when the method takes parameters.
fn show_decl(st: &SymbolTable, m: SymbolId) -> String {
    let s = st.get(m);
    let ret = match &s.ty {
        Type::Method { ret, .. } => st.display_type(ret),
        Type::NoType => "Unit".to_string(),
        other => st.display_type(other),
    };
    let paramss = match &s.ty {
        Type::Method { paramss, .. } => paramss.clone(),
        _ => Vec::new(),
    };
    let mut out = format!("def {}", s.name);
    for ps in &paramss {
        if ps.is_empty() && paramss.len() == 1 {
            out.push_str("()");
            continue;
        }
        let names: Vec<String> = ps.iter().map(|p| st.display_type(p)).collect();
        out.push('(');
        out.push_str(&names.join(", "));
        out.push(')');
    }
    format!("{out}: {ret}")
}

/// `T`'s own kind word, for `(defined in trait Loud)`.
fn owner_word(st: &SymbolTable, id: SymbolId) -> &'static str {
    if is_interface(st, id) {
        "trait"
    } else {
        "class"
    }
}

/// The chain of *classes* above `cls`: `cls`, its superclass, its
/// superclass's superclass … Traits are skipped, and a trait's own class
/// parent is not followed — that one is the constraint, not this class's
/// superclass.
pub fn superclass_chain(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    let mut out = vec![cls];
    let mut cur = cls;
    for _ in 0..64 {
        let next = st
            .get(cur)
            .parents
            .iter()
            .filter_map(|p| st.class_sym_of(p))
            .find(|&p| !is_interface(st, p));
        match next {
            Some(n) if n != cur && !out.contains(&n) => {
                out.push(n);
                cur = n;
            }
            _ => break,
        }
    }
    out
}

/// `Any` / `AnyRef` / `AnyVal` / `Object`.
fn is_top(st: &SymbolTable, id: SymbolId) -> bool {
    matches!(
        st.get(id).name.as_str(),
        "Any" | "AnyRef" | "AnyVal" | "Object"
    )
}

/// The superclass every mixin of `cls` constrains it to, when `cls` itself
/// writes no class parent: `class X extends Loud` where `trait Loud extends
/// Animal` really is an `Animal` on the JVM (SLS 5.1). Returns the most
/// derived such constraint, **applied**: slick's
/// `class QueryInvokerImpl[R] extends QueryInvoker[R]`, where
/// `trait QueryInvoker[R] extends StatementInvoker[R]`, must acquire
/// `StatementInvoker[R]` and not the bare `StatementInvoker`.
pub fn inherited_superclass(st: &SymbolTable, cls: SymbolId) -> Option<Type> {
    if is_interface(st, cls) {
        return None;
    }
    let parents = st.get(cls).parents.clone();
    // Only when the class writes no class parent of its own.
    if parents
        .iter()
        .filter_map(|p| st.class_sym_of(p))
        .any(|p| !is_interface(st, p))
    {
        return None;
    }
    let mut best: Option<(SymbolId, Type)> = None;
    for p in &parents {
        let Some(pid) = st.class_sym_of(p) else {
            continue;
        };
        if !is_interface(st, pid) {
            continue;
        }
        // `base_type_seq` substitutes the parent's type arguments through, so
        // the class it yields is already applied.
        let found = st.base_type_seq(p).into_iter().find_map(|b| {
            let bid = st.class_sym_of(&b)?;
            (!is_interface(st, bid) && !is_top(st, bid)).then_some((bid, b))
        });
        let Some((bid, bty)) = found else { continue };
        best = match &best {
            None => Some((bid, bty)),
            Some((cur, _)) if superclass_chain(st, bid).contains(cur) => Some((bid, bty)),
            keep => keep.clone(),
        };
    }
    best.map(|(_, t)| t)
}

/// A diagnostic this module wants `check.rs` to report: which parent (by
/// index into the parent list, so the caller can pick the right span) and
/// what to say.
pub struct MixinError {
    pub parent_index: usize,
    pub message: String,
}

/// SLS 5.3.3: `class C extends S with T` is only legal when `S` conforms to
/// `T`'s own superclass. scalac 2.13.16:
///
/// ```text
/// illegal inheritance; superclass Plain
///  is not a subclass of the superclass Animal
///  of the mixin trait Loud
/// ```
pub fn check_mixin_superclasses(st: &SymbolTable, cls: SymbolId) -> Vec<MixinError> {
    let mut out = Vec::new();
    if cls.is_none() {
        return out;
    }
    let chain = superclass_chain(st, cls);
    let own_super = chain.get(1).copied();
    let own_super_name = own_super
        .map(|s| st.get(s).name.clone())
        .unwrap_or_else(|| "AnyRef".to_string());
    for (idx, p) in st.get(cls).parents.clone().iter().enumerate() {
        let Some(pid) = st.class_sym_of(p) else {
            continue;
        };
        if !is_interface(st, pid) {
            continue;
        }
        let Some(k) = trait_superclass(st, pid) else {
            continue;
        };
        if chain.contains(&k) {
            continue;
        }
        // No class parent of its own: the template *acquires* the mixin's
        // superclass (SLS 5.1) instead of clashing with it.
        if own_super.is_none() {
            continue;
        }
        out.push(MixinError {
            parent_index: idx,
            message: format!(
                "illegal inheritance; superclass {own_super_name}\n is not a subclass of the superclass {}\n of the mixin trait {}",
                st.get(k).name,
                st.get(pid).name
            ),
        });
    }
    out
}

/// Every `abstract override` member reachable from a **concrete** `cls` has to
/// bottom out in a real implementation further down the linearization, or
/// `super.m` inside the trait has nothing to call.
///
/// scalac 2.13.16 says, for `class NoImpl extends Animal with Loud`:
///
/// ```text
/// class NoImpl needs to be a mixin.
/// abstract override def speak: String (defined in trait Loud) is marked `abstract` and `override`, but no concrete implementation could be found in a base class
/// ```
///
/// and, when the class itself supplies the only implementation:
///
/// ```text
/// `abstract override` modifiers required to override:
/// abstract override def speak: String (defined in trait Loud)
/// ```
///
/// `headline` is the first line of the first form — `object creation
/// impossible.` at a `new C with T`, `class C needs to be a mixin.` at a
/// class definition.
pub fn check_abstract_override_grounded(
    st: &SymbolTable,
    cls: SymbolId,
    headline: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    if cls.is_none() {
        return out;
    }
    let lin = linearize(st, cls);
    let mut reported: Vec<String> = Vec::new();
    for (i, &owner) in lin.iter().enumerate() {
        if i == 0 || !is_interface(st, owner) {
            continue;
        }
        for &m in &st.get(owner).members {
            if !is_abstract_override(st, m) {
                continue;
            }
            let name = st.get(m).name.clone();
            let arity = st.get(m).params.len();
            if reported.contains(&name) {
                continue;
            }
            let grounded = lin.iter().skip(i + 1).any(|&below| {
                st.get(below).members.iter().any(|&b| {
                    st.get(b).name == name
                        && st.get(b).params.len() == arity
                        && is_concrete_method(st, b)
                        && !is_abstract_override(st, b)
                })
            });
            if grounded {
                continue;
            }
            reported.push(name.clone());
            let decl = format!(
                "abstract override {} (defined in {} {})",
                show_decl(st, m),
                owner_word(st, owner),
                st.get(owner).name
            );
            // The class's *own* definition sits above the trait in the
            // linearization, so it can never serve as the super target;
            // scalac asks for `abstract override` on it instead.
            let own = st.get(cls).members.iter().any(|&b| {
                st.get(b).name == name
                    && st.get(b).params.len() == arity
                    && is_concrete_method(st, b)
                    && !is_abstract_override(st, b)
            });
            if own {
                out.push(format!(
                    "`abstract override` modifiers required to override:\n{decl}"
                ));
            } else {
                out.push(format!(
                    "{headline}\n{decl} is marked `abstract` and `override`, but no concrete implementation could be found in a base class"
                ));
            }
        }
    }
    out
}
