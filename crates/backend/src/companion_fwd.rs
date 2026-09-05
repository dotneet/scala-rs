//! nsc's `addForwarders`: the `public static` methods a top-level `object`
//! contributes to the classfile named after it.
//!
//! When an `object Test` has no companion, that classfile is the *mirror
//! class* `Test.class`, which nsc synthesizes for the purpose. When a `class
//! Test` (or `trait Test`) is written next to it, there is no mirror class:
//! the forwarders go onto the companion's own classfile instead. That second
//! half was missing here, which is why `java Test` could not start a program
//! whose `object Test` had a companion class (`scala/scala`'s `run/t363` is
//! exactly that test, and nothing else).
//!
//! The rules below were read off `javap -p` on real scalac 2.13.16 rather
//! than from nsc's source, one probe per question. What it forwards:
//!
//! * every **public** member the module class has, *including inherited*
//!   ones -- a superclass's `def`, a mixed-in trait's `def` and its `val`;
//! * `val` / `var` / `lazy val` getters, and a `var`'s `x_$eq` setter;
//! * every alternative of an overloaded method;
//! * `deflt$default$1` and the rest of the default-argument getters;
//! * a value class's `plus$extension` statics, when the companion declares
//!   them;
//! * a `case object`'s `productPrefix` / `productArity` / `toString` / … .
//!
//! What it does *not* forward:
//!
//! * `private` members, and `protected` ones -- note that both `protected
//!   def prot` and `private[p] def bnd` come out `public` in `Test$.class`,
//!   so the JVM access flags alone cannot decide this; it takes the Scala
//!   symbol;
//! * anything whose *name* also names a member of the companion class,
//!   inherited members included. This is by name, not by signature: with
//!   `class Test { def clash(): Int }` next to `object Test { def
//!   clash(): Int; def clash(i: Int): Int }` *neither* alternative is
//!   forwarded. `java.lang.Object`'s names count, which is why a companion
//!   class suppresses a `toString` forwarder that a mirror class would get;
//! * members merely inherited from `java.lang.Object` (nothing is emitted
//!   onto the module classfile for those, so they cannot be picked here
//!   either);
//! * bridges;
//! * anything at all when the `object` is not top level -- `object Outer {
//!   class Nested; object Nested }` gets no forwarders on `Outer$Nested`.

use crate::classfile::{
    encode_method_name, Method, ACC_ABSTRACT, ACC_BRIDGE, ACC_PUBLIC, ACC_STATIC,
};
use scala_rs_parser::{Flags, SymbolId};
use scala_rs_typer::{SymKind, SymbolTable};
use std::collections::HashSet;

/// One `public static` forwarder: the module method it stands for, by JVM
/// name and descriptor.
pub(crate) struct Forwarder {
    pub name: String,
    pub desc: String,
    /// The module method's own JVMS §4.7.9 `Signature`, carried over: the
    /// forwarder has the same descriptor, so scalac signs it the same way.
    pub signature: Option<String>,
}

/// The class (or trait) written next to a module class, if the source wrote
/// one. Looked up through the owner rather than by simple name: two packages
/// in one run can both have a `Config`.
pub(crate) fn companion_class_of(st: &SymbolTable, module_class: SymbolId) -> Option<SymbolId> {
    if module_class.is_none() {
        return None;
    }
    let s = st.get(module_class);
    let base = s.name.strip_suffix('$').unwrap_or(&s.name).to_string();
    let owner = s.owner;
    st.get(owner)
        .members
        .iter()
        .copied()
        .find(|&m| st.get(m).kind == SymKind::Class && st.get(m).name == base)
}

/// Names a companion class already carries, so a forwarder must not: its own
/// members, everything it inherits, and `java.lang.Object`'s methods.
pub(crate) fn conflicting_names(st: &SymbolTable, companion: SymbolId) -> HashSet<String> {
    // No companion class means a mirror class, whose whole method table is
    // forwarders: nothing can clash. That is also why a mirror class does get
    // a `toString` forwarder when the `object` overrides `toString`, and a
    // companion class never does.
    if companion.is_none() {
        return HashSet::new();
    }
    let mut out: HashSet<String> = OBJECT_METHODS.iter().map(|n| (*n).to_string()).collect();
    for owner in scala_rs_typer::linearize(st, companion) {
        for m in &st.get(owner).members {
            let s = st.get(*m);
            if matches!(s.kind, SymKind::Method | SymKind::Term) {
                out.insert(encode_method_name(&s.name));
                out.insert(setter_name(&s.name));
            }
        }
    }
    out
}

/// Names on the module class that nsc refuses to forward because of Scala
/// access: `protected`, `private[p]` and `protected[p]`. Plain `private` needs
/// no entry here -- it is `ACC_PRIVATE` in the classfile and never a
/// candidate.
///
/// By name, because that is all a classfile method table offers to match on.
/// An overload set that mixes a `protected` alternative with a public one
/// would lose the public one too; real scalac decides per symbol. Nothing in
/// the corpus, slick or gitbucket writes that, and losing a forwarder is the
/// safe direction -- an extra one would shadow a companion member.
pub(crate) fn restricted_names(st: &SymbolTable, module_class: SymbolId) -> HashSet<String> {
    let mut out = HashSet::new();
    if module_class.is_none() {
        return out;
    }
    for owner in scala_rs_typer::linearize(st, module_class) {
        for m in &st.get(owner).members {
            let s = st.get(*m);
            if s.flags.contains(Flags::PROTECTED) || s.private_within.is_some() {
                out.insert(encode_method_name(&s.name));
                out.insert(setter_name(&s.name));
            }
        }
    }
    out
}

fn setter_name(name: &str) -> String {
    format!("{}_$eq", encode_method_name(name))
}

/// `java.lang.Object`'s methods. A companion class has all of them whatever
/// it declares, and a module class that merely inherits them emits nothing
/// for them either.
const OBJECT_METHODS: [&str; 11] = [
    "toString",
    "hashCode",
    "equals",
    "getClass",
    "clone",
    "notify",
    "notifyAll",
    "wait",
    "finalize",
    "registerNatives",
    "$init$",
];

/// Which of a module classfile's own methods become forwarders.
///
/// Driven by the method table that was actually emitted, not by the symbol
/// table: a forwarder is an `invokevirtual` against the module, so a name the
/// module classfile does not really carry would link and then throw
/// `NoSuchMethodError` at the first call.
pub(crate) fn pick(
    methods: &[Method],
    restricted: &HashSet<String>,
    conflicting: &HashSet<String>,
) -> Vec<Forwarder> {
    let mut out: Vec<Forwarder> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for m in methods {
        if m.access & ACC_PUBLIC == 0 {
            continue;
        }
        if m.access & (ACC_STATIC | ACC_BRIDGE | ACC_ABSTRACT) != 0 {
            continue;
        }
        if is_internal_name(&m.name)
            || restricted.contains(&m.name)
            || conflicting.contains(&m.name)
        {
            continue;
        }
        if !seen.insert((m.name.clone(), m.desc.clone())) {
            continue;
        }
        out.push(Forwarder {
            name: m.name.clone(),
            desc: m.desc.clone(),
            signature: m.signature.clone(),
        });
    }
    out
}

/// Compiler-internal method names, which nsc keeps off the mirror class:
/// constructors, expanded names (`p$C$$x`, the shape `private[this]` widening
/// and outer accessors take; `p$T$_setter_$v_$eq`, a trait's `val` setter),
/// lambda bodies and lazy-value initialisers.
fn is_internal_name(name: &str) -> bool {
    name.starts_with('<')
        || name.contains("$$")
        || name.contains("$_setter_$")
        || name.contains("$anonfun$")
        || name.ends_with("$lzycompute")
        || name.ends_with("$adapted")
}

/// A JVM parameter's kind, as far as a pass-through forwarder cares: which
/// load opcode moves it and how many slots it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DescSort {
    Int,
    Long,
    Float,
    Double,
    Ref,
    Void,
}

impl DescSort {
    pub(crate) fn slots(self) -> u16 {
        match self {
            DescSort::Long | DescSort::Double => 2,
            DescSort::Void => 0,
            _ => 1,
        }
    }
}

/// A method descriptor read as a forwarder needs it: where each argument
/// sits, how many local slots the whole parameter list takes, and what the
/// return opcode has to be.
pub(crate) type Signature = (Vec<(u16, DescSort)>, u16, DescSort);

/// Split a method descriptor into `(parameter slot, sort)` pairs and the
/// return sort. `None` for a descriptor this cannot parse, which drops the
/// forwarder rather than emitting a broken one.
pub(crate) fn desc_slots(desc: &str) -> Option<Signature> {
    let bytes = desc.as_bytes();
    if bytes.first() != Some(&b'(') {
        return None;
    }
    let mut i = 1usize;
    let mut slot = 0u16;
    let mut params = Vec::new();
    while i < bytes.len() && bytes[i] != b')' {
        let (sort, next) = one_type(bytes, i)?;
        params.push((slot, sort));
        slot += sort.slots();
        i = next;
    }
    if i >= bytes.len() {
        return None;
    }
    let (ret, end) = one_type(bytes, i + 1)?;
    if end != bytes.len() {
        return None;
    }
    Some((params, slot.max(1), ret))
}

/// One field descriptor starting at `i`; returns its sort and the index just
/// past it.
fn one_type(bytes: &[u8], i: usize) -> Option<(DescSort, usize)> {
    match *bytes.get(i)? {
        b'B' | b'C' | b'I' | b'S' | b'Z' => Some((DescSort::Int, i + 1)),
        b'J' => Some((DescSort::Long, i + 1)),
        b'F' => Some((DescSort::Float, i + 1)),
        b'D' => Some((DescSort::Double, i + 1)),
        b'V' => Some((DescSort::Void, i + 1)),
        b'L' => {
            let end = bytes[i..].iter().position(|c| *c == b';')? + i;
            Some((DescSort::Ref, end + 1))
        }
        b'[' => {
            let mut j = i;
            while *bytes.get(j)? == b'[' {
                j += 1;
            }
            let (_, end) = one_type(bytes, j)?;
            Some((DescSort::Ref, end))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_slots() {
        let (ps, locals, ret) = desc_slots("(IJLjava/lang/String;[[DZ)V").expect("parses");
        assert_eq!(
            ps,
            vec![
                (0, DescSort::Int),
                (1, DescSort::Long),
                (3, DescSort::Ref),
                (4, DescSort::Ref),
                (5, DescSort::Int),
            ]
        );
        assert_eq!(locals, 6);
        assert_eq!(ret, DescSort::Void);
    }

    #[test]
    fn empty_descriptor_still_reserves_a_slot() {
        let (ps, locals, ret) = desc_slots("()Ljava/lang/Object;").expect("parses");
        assert!(ps.is_empty());
        assert_eq!(locals, 1);
        assert_eq!(ret, DescSort::Ref);
    }

    #[test]
    fn rejects_a_malformed_descriptor() {
        assert!(desc_slots("(Q)V").is_none());
        assert!(desc_slots("IV").is_none());
        assert!(desc_slots("()").is_none());
    }

    #[test]
    fn internal_names_are_never_forwarded() {
        assert!(is_internal_name("<init>"));
        assert!(is_internal_name("p$C$$x"));
        assert!(is_internal_name("$anonfun$f$1"));
        assert!(is_internal_name("v$lzycompute"));
        assert!(is_internal_name("p$T$_setter_$x_$eq"));
        assert!(!is_internal_name("w_$eq"));
        assert!(!is_internal_name("deflt$default$1"));
        assert!(!is_internal_name("plus$extension"));
    }
}
