//! nsc's `RefChecks.checkNoDoubleDefs` — "double definition: … have same type
//! after erasure".
//!
//! Two methods a template declares under the same name are legal overloads
//! only while erasure keeps them apart. `def f(x: Int => Unit)` and
//! `def f(x: Int => String)` both become `f(Function1)` in the class file, so
//! one of them would be lost; nsc rejects the pair, and until this module
//! existed scala-rs accepted it and emitted a class file with two identical
//! descriptors (`neg/t588`, `neg/valueclasses-doubledefs`, `neg/t0259`, …).
//!
//! ## Deliberate conservatism
//!
//! This is a rejection rule, so a false positive breaks working code while a
//! missed one is only a gap. The comparison is therefore made only between
//! members the *source* of one template wrote, and only when both signatures
//! erase to something this compiler is sure of:
//!
//! * synthetic members are skipped — a case class's `copy`/`apply`, default
//!   getters and the accessors a `val` parameter becomes are the compiler's,
//!   and two of them colliding is this compiler's bug to fix elsewhere, not
//!   the programmer's error;
//! * a signature that is still `NoType`, `Error` or an unresolved
//!   `Type::Named` says nothing, and neither does an `Overload`;
//! * varargs are compared as what they erase to (`Seq`), which is what makes
//!   `neg/t0259`'s two `(groups: T*)` constructors a collision.
//!
//! Everything here is a pure function of the symbol table plus the template's
//! own body, the same shape as `override_check`.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// One `double definition`, attached to the *second* of the two members —
/// which is where scalac points.
pub struct DoubleDef {
    pub sym: SymbolId,
    pub message: String,
}

/// A type this compiler resolved. Mirrors `override_check::uncertain`.
fn uncertain(ty: &Type) -> bool {
    let mut bad = false;
    walk(ty, &mut |t| {
        if matches!(
            t,
            Type::NoType | Type::Error | Type::Named { .. } | Type::Overload(_)
        ) {
            bad = true;
        }
    });
    bad
}

fn walk(ty: &Type, f: &mut impl FnMut(&Type)) {
    f(ty);
    match ty {
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => walk(t, f),
        Type::Tuple(ts) | Type::Overload(ts) => ts.iter().for_each(|t| walk(t, f)),
        Type::Function { params, ret } => {
            params.iter().for_each(|t| walk(t, f));
            walk(ret, f);
        }
        Type::Named { args, .. } | Type::Class { args, .. } => args.iter().for_each(|t| walk(t, f)),
        Type::Method { paramss, ret } => {
            paramss.iter().flatten().for_each(|t| walk(t, f));
            walk(ret, f);
        }
        Type::Applied { ctor, args } => {
            walk(ctor, f);
            args.iter().for_each(|t| walk(t, f));
        }
        Type::Annotated { tpe, .. } => walk(tpe, f),
        _ => {}
    }
}

/// The class file's own notion of "the same parameter type", as a string.
///
/// `erase_member_ty` stops one step short of the descriptor: it leaves a
/// `Function`, a `Tuple`, a by-name and a repeated parameter structured, and
/// the backend collapses each to `scala.FunctionN` / `scala.TupleN` /
/// `scala.Function0` / `scala.collection.immutable.Seq` when it writes the
/// descriptor. Two overloads collide exactly when *those* agree — which is why
/// `def visit(f: Int => Unit)` and `def visit(f: Int => String)` are a double
/// definition (`neg/t588`) — so the comparison is made on this key and mirrors
/// `backend::gen::jvm_desc`.
fn key(ty: &Type) -> String {
    match ty {
        Type::Unit | Type::NoType => "V".into(),
        Type::Boolean => "Z".into(),
        Type::Byte => "B".into(),
        Type::Short => "S".into(),
        Type::Int => "I".into(),
        Type::Long => "J".into(),
        Type::Float => "F".into(),
        Type::Double => "D".into(),
        Type::Char => "C".into(),
        Type::String => "Ljava/lang/String;".into(),
        Type::Nothing => "Lscala/runtime/Nothing$;".into(),
        // `Null` has a class of its own too, so `def f(x: Null)` and
        // `def f(x: AnyRef)` are two methods for nsc, not a double definition.
        Type::Null => "Lscala/runtime/Null$;".into(),
        Type::Array(t) => {
            // Both bottom arrays erase to Object[], including nested arrays.
            let elem = match t.widen_constant() {
                Type::Null | Type::Nothing => "Ljava/lang/Object;".into(),
                _ => key(t),
            };
            format!("[{elem}")
        }
        Type::Class { sym, .. } | Type::ModuleRef(sym) | Type::ThisType(sym) => {
            format!("L#{};", sym.0)
        }
        Type::Function { params, .. } => format!("Lscala/Function{};", params.len()),
        Type::Tuple(ts) => format!("Lscala/Tuple{};", ts.len()),
        Type::ByName(_) => "Lscala/Function0;".into(),
        Type::Repeated(_) => "Lscala/collection/immutable/Seq;".into(),
        Type::Constant(lit) => key(&Type::lit_underlying(lit)),
        Type::Annotated { tpe, .. } => key(tpe),
        Type::Method { ret, .. } => key(ret),
        // Everything left over erases to `Object` in the descriptor.
        _ => "Ljava/lang/Object;".into(),
    }
}

/// The erased parameter types of `id`, flattened across parameter clauses —
/// which is exactly what the class file records, and why
/// `def foo(d: D)(a: Any, d2: d.type)` and `def foo(d: D)(a: Any)(d2: d.type)`
/// collide (`neg/t6443c`).
///
/// `None` when the signature is not a method type, or when any part of it is
/// one this compiler did not resolve.
fn erased_params(st: &SymbolTable, id: SymbolId) -> Option<(Vec<String>, Vec<Type>, Type)> {
    let ty = st.get(id).ty.clone();
    let Type::Method { paramss, ret } = &ty else {
        return None;
    };
    if uncertain(&ty) {
        return None;
    }
    let mut keys = Vec::new();
    let mut tys = Vec::new();
    for p in paramss.iter().flatten() {
        // `x: T*` is a `Seq` in the descriptor and `x: => T` a `Function0`;
        // both are compared as the class file will see them so that
        // `(groups: (String, Int)*)` and `(groups: String*)` meet
        // (`neg/t0259`).
        let e = crate::erasure::erase_member_ty(p, st);
        if uncertain(&e) {
            return None;
        }
        keys.push(key(&e));
        tys.push(e);
    }
    // The **result** is part of the descriptor, and the JVM lets two methods
    // differ in it alone -- which Scala uses. `scala.Function.uncurried` is
    // five overloads that all take one `Function1` and return `Function2` ...
    // `Function6`, and real scalac 2.13.16 accepts them (probed) while
    // rejecting `def g(x: List[Int]): Int` beside `def g(x: List[String]):
    // Int`. Leaving the result out cost twelve false diagnostics on
    // `src/library/scala/Function.scala` alone.
    let r = crate::erasure::erase_member_ty(ret, st);
    if uncertain(&r) {
        return None;
    }
    keys.push(format!("){}", key(&r)));
    Some((keys, tys, r))
}

/// The erased parameter type as the class file records it, for the message.
/// `erase_member_ty` leaves the structured shapes alone, so print what
/// [`key`] compares.
fn show_erased(ty: &Type, st: &SymbolTable) -> String {
    match ty {
        Type::Function { params, .. } => format!("Function{}", params.len()),
        Type::Tuple(ts) => format!("Tuple{}", ts.len()),
        Type::ByName(_) => "Function0".into(),
        Type::Repeated(_) => "Seq".into(),
        _ => st.display_type(ty),
    }
}

/// A member this rule has anything to say about: a `def` (or a secondary
/// constructor) the source of this very template wrote.
fn is_candidate(st: &SymbolTable, cls: SymbolId, id: SymbolId) -> bool {
    if id.is_none() {
        return false;
    }
    let s = st.get(id);
    if s.owner != cls || s.kind != SymKind::Method {
        return false;
    }
    if s.name.contains('$') {
        return false;
    }
    // A macro def has no bytecode -- every call site is replaced by the
    // expansion -- so two of them cannot collide in a class file. Real scalac
    // 2.13.16 accepts `pos/t7776`'s two `app` macros and rejects the same pair
    // written as ordinary methods (both probed).
    if s.macro_impl.is_some() {
        return false;
    }
    let f = s.flags;
    !(f.contains(Flags::SYNTHETIC) || f.contains(Flags::BRIDGE) || f.contains(Flags::ACCESSOR))
}

/// Report every pair of same-named methods of `cls`, among `members`, whose
/// erased parameter lists coincide.
///
/// `members` are the symbols of the template's own body, in source order, so
/// the second of a pair is the one scalac blames.
pub fn check_double_defs(st: &SymbolTable, cls: SymbolId, members: &[SymbolId]) -> Vec<DoubleDef> {
    let mut out = Vec::new();
    if cls.is_none() {
        return out;
    }
    let mut seen: Vec<(SymbolId, Vec<String>)> = Vec::new();
    for &id in members {
        if !is_candidate(st, cls, id) {
            continue;
        }
        let Some((ps, tys, ret)) = erased_params(st, id) else {
            continue;
        };
        let name = st.get(id).name.clone();
        if let Some((prev, _)) = seen
            .iter()
            .find(|(p, pp)| st.get(*p).name == name && *pp == ps)
        {
            let desc = |m: SymbolId| {
                let s = st.get(m);
                let what = if s.flags.contains(Flags::CONSTRUCTOR) || s.name == "<init>" {
                    format!("constructor {}", st.get(cls).name.trim_end_matches('$'))
                } else {
                    format!("def {}", s.name)
                };
                match &s.ty {
                    Type::Method { paramss, ret } => {
                        let clauses = paramss
                            .iter()
                            .map(|c| {
                                let ps = c
                                    .iter()
                                    .map(|t| st.display_type(t))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                format!("({ps})")
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        format!("{what}{clauses}: {}", st.display_type(ret))
                    }
                    t => format!("{what}{}", st.display_type(t)),
                }
            };
            let erased = tys
                .iter()
                .map(|t| show_erased(t, st))
                .collect::<Vec<_>>()
                .join(", ");
            let ret = show_erased(&ret, st);
            out.push(DoubleDef {
                sym: id,
                // nsc puts the detail on continuation lines and so do we: the
                // head of the message is what says which check fired.
                message: format!(
                    "double definition:\n{} and {} have same type after erasure: ({erased}): {ret}",
                    desc(*prev),
                    desc(id)
                ),
            });
            continue;
        }
        seen.push((id, ps));
    }
    out
}
