//! `case class` / `case object` as a real `scala.Product`, and the synthetic
//! companion as a real `scala.runtime.AbstractFunctionN`.
//!
//! What nsc emits (checked with `javap -v -p` against scalac 2.13.16):
//!
//! ```text
//! case class P(x: Int, y: String)
//!   public class Main$P  implements scala.Product, java.io.Serializable
//!   public class Main$P$ extends scala.runtime.AbstractFunction2<Object, String, Main$P>
//!                        implements java.io.Serializable
//! case object Q
//!   public class Main$Q$ implements scala.Product, java.io.Serializable
//! ```
//!
//! The `Product` edge is unconditional — a `case class` that already extends
//! something keeps it and gains `Product` after it
//! (`class E$L implements E$T, scala.Product, java.io.Serializable`). Without
//! the edge `val p: Product = P(1, 2)` and `List[Product](P(1, 2))` are type
//! errors, and `productIterator` / `productElement` / `productElementName` /
//! `productElementNames` have nowhere to come from: nsc declares all four on
//! `scala.Product`, and only the first three are overridden in the case class.
//!
//! The `AbstractFunctionN` edge is where `P.tupled` and `P.curried` come from
//! (`FunctionN` declares them; `crates/typer/src/prelude_fntuple.rs` gives the
//! prelude's `FunctionN` the same two members). nsc adds it only to the
//! companion it synthesized itself, and only when it fits — every one of these
//! was read off a classfile rather than guessed:
//!
//! * a user-written `object P` never gets it, whatever it extends:
//!   `object P extends Base` → `class F$Plain$ extends E$Base`, and even
//!   `object P extends SomeTrait` → `class F$WithTrait$ implements E$Mix`
//!   with no `AbstractFunction1` in sight;
//! * a case class with type parameters does not: `case class Gen[A](a: A, b: Int)`
//!   → `class E$Gen$ implements java.io.Serializable`;
//! * more than one parameter section does not, implicit sections included:
//!   `case class Impl(a: Int)(implicit o: Ordering[Int])` and
//!   `case class Curr(a: Int)(b: String)` both → plain `implements Serializable`;
//! * arity 23 and up does not — `AbstractFunctionN` stops at 22:
//!   `case class Big(a1: Int, …, a23: Int)` → plain `implements Serializable`,
//!   while the 22-field sibling gets `AbstractFunction22`.
//!
//! Everything here is `library_abi`-only. `scala.Product`,
//! `java.io.Serializable` and `scala.runtime.AbstractFunctionN` all come from
//! the classpath; the private runtime (`crates/backend/src/runtime.rs`) has
//! none of them, so under `--no-scala-library` no parent is linked and
//! `p.productIterator` stays "value productIterator is not a member of P"
//! rather than a call the backend could not emit.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// The two interfaces every `case class` and `case object` implements.
pub(crate) const PRODUCT_PARENTS: [&str; 2] = ["scala/Product", "java/io/Serializable"];

/// `java.io.Serializable`, the one parent a companion always gets.
pub(crate) const SERIALIZABLE: &str = "java/io/Serializable";

/// Highest arity `scala.runtime.AbstractFunctionN` is defined for.
pub(crate) const MAX_ABSTRACT_FUNCTION: usize = 22;

/// Append `parents` to `class_id`'s parent list, keeping what is already there.
///
/// Idempotent on purpose: the header pass may type the same template more than
/// once, and `Product` must not be pushed twice.
pub(crate) fn add_parents(st: &mut SymbolTable, class_id: SymbolId, parents: &[SymbolId]) {
    for &p in parents {
        if p.is_none() {
            continue;
        }
        let ty = Type::Class {
            sym: p,
            args: vec![],
        };
        if !st.get(class_id).parents.contains(&ty) {
            st.get_mut(class_id).parents.push(ty);
        }
    }
}

/// Is `class_id` a `case class`, or the module class of a `case object`?
///
/// The `CASE` flag alone does not say: `ensure_companion` stamps it on the
/// *companion* module class of every case class as well, and `P$` is not a
/// `Product` (nsc: `class Main$P$ extends AbstractFunction2 … implements
/// Serializable`, no `scala.Product`). A companion is a module class that has
/// a class of the same name beside it; a `case object`'s module class does not.
pub(crate) fn wants_product(st: &SymbolTable, class_id: SymbolId) -> bool {
    if class_id.is_none() || !st.get(class_id).flags.contains(Flags::CASE) {
        return false;
    }
    let s = st.get(class_id);
    if s.kind != SymKind::ModuleClass && !s.flags.contains(Flags::MODULE) {
        return true;
    }
    let base = s.name.strip_suffix('$').unwrap_or(&s.name).to_string();
    let owner = s.owner;
    !st.get(owner)
        .members
        .iter()
        .any(|&m| st.get(m).kind == SymKind::Class && st.get(m).name == base)
}

/// The `scala.runtime.AbstractFunctionN` a case class's companion extends, as
/// an internal name — `None` when nsc would not give it one.
///
/// `paramss` is the case class's primary constructor, already typed.
pub(crate) fn companion_function_class(
    st: &SymbolTable,
    class_id: SymbolId,
    paramss: &[Vec<Type>],
) -> Option<String> {
    if class_id.is_none() || !st.get(class_id).flags.contains(Flags::CASE) {
        return None;
    }
    // `case class Gen[A](a: A)`: nsc leaves the companion a plain
    // `Serializable`. `AbstractFunction1[A, Gen[A]]` would need the *method*'s
    // type parameter as a *class* parameter, which is not expressible.
    if !st.get(class_id).tparams.is_empty() {
        return None;
    }
    // Curried and implicit sections alike: only a single-section constructor
    // is a `FunctionN`.
    let [params] = paramss else {
        return None;
    };
    if params.len() > MAX_ABSTRACT_FUNCTION {
        return None;
    }
    synthetic_companion(st, class_id)?;
    Some(format!("scala/runtime/AbstractFunction{}", params.len()))
}

/// The module class of `class_id`'s companion, but only when *this compiler*
/// synthesized it -- `namer_member` clears `SYNTHETIC` from the module class
/// the moment it sees a written `object P`, and nsc gives a written companion
/// neither `AbstractFunctionN` nor anything else.
pub(crate) fn synthetic_companion(st: &SymbolTable, class_id: SymbolId) -> Option<SymbolId> {
    let module_cls = companion_module_class(st, class_id)?;
    st.get(module_cls)
        .flags
        .contains(Flags::SYNTHETIC)
        .then_some(module_cls)
}

/// Give the synthetic companion of `class_id` the `AbstractFunctionN` parent,
/// with `T1 … Tn` from the constructor and `R` the case class itself.
///
/// `abs_fn` is the symbol `companion_function_class` named, and `module_cls`
/// the one `synthetic_companion` found.
pub(crate) fn link_companion_function(
    st: &mut SymbolTable,
    module_cls: SymbolId,
    class_id: SymbolId,
    params: &[Type],
    abs_fn: SymbolId,
) {
    if abs_fn.is_none() || module_cls.is_none() {
        return;
    }
    let seq = crate::classpath::find_by_jvm(st, "scala/collection/immutable/Seq");
    let mut args: Vec<Type> = params.to_vec();
    // A repeated parameter is a `Seq` to everyone but the caller; nsc writes
    // `AbstractFunction2<Object, scala.collection.immutable.Seq<String>, …>`.
    for a in args.iter_mut() {
        if let (Type::Repeated(inner), Some(seq)) = (a.clone(), seq) {
            *a = Type::Class {
                sym: seq,
                args: vec![*inner],
            };
        }
    }
    args.push(Type::Class {
        sym: class_id,
        args: vec![],
    });
    let ty = Type::Class { sym: abs_fn, args };
    let parents = &mut st.get_mut(module_cls).parents;
    // Drop the placeholder `AnyRef` the namer gave it; an `AbstractFunctionN`
    // *is* the superclass, and leaving both would make `AnyRef` win in the
    // backend's "first non-interface parent" pick.
    parents.retain(|p| !matches!(p, Type::AnyRef));
    if parents
        .iter()
        .any(|p| matches!(p, Type::Class { sym, .. } if *sym == abs_fn))
    {
        return;
    }
    parents.insert(0, ty);
}

/// The module class (`P$`) of the companion of `class_id`, if there is one.
fn companion_module_class(st: &SymbolTable, class_id: SymbolId) -> Option<SymbolId> {
    let name = st.get(class_id).name.clone();
    let owner = st.get(class_id).owner;
    let m = st
        .get(owner)
        .members
        .iter()
        .copied()
        .find(|&s| st.get(s).kind == SymKind::Module && st.get(s).name == name)
        .or_else(|| {
            st.lookup(&name)
                .into_iter()
                .find(|&s| st.get(s).kind == SymKind::Module && st.get(s).owner == owner)
        })?;
    match st.get(m).ty {
        Type::ModuleRef(c) => Some(c),
        _ => Some(m),
    }
}
