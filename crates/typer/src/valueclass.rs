//! The value-class restrictions — SLS 5.1.7 / SIP-15, nsc's
//! `Typers.validateDerivedValueClass`.
//!
//! ## Why this module exists
//!
//! Nothing here was checked. `test/files/neg/valueclasses.scala` is thirty
//! lines that are nothing but violations, and it compiled to 33 class files
//! without a word. That was invisible for as long as `@specialized` was a
//! parse error, because line 29 of that file carries one: the parse error
//! stood in for fifteen checks that do not exist.
//!
//! ## The trigger
//!
//! nsc runs this when **the first parent** is `AnyVal`
//! (`clazz.info.firstParent.typeSymbol == AnyValClass`), not when the class is
//! a *well-formed* value class. That distinction is the whole point: the
//! errors below all describe classes that name `AnyVal` and are not value
//! classes because of it. `SymbolTable::is_value_class` asks the opposite
//! question — one ctor field and an `AnyVal` parent — and every class this
//! module complains about answers it with `false`.
//!
//! ## The order of the parameter checks is nsc's, and it is not a chain
//!
//! ```text
//! if (paramAccessor.isMutable) error("value class parameter must not be a var")
//! decls.find(_.accessedOrSelf == paramAccessor) match {
//!   case None                       => error("... must be a val and not be private[this]")
//!   case Some(a) if a.isProtectedLocal => error("... must not be protected[this]")
//!   case Some(_)                    => checkEphemeral(...)   // field definitions
//! }
//! ```
//!
//! The `var` test is a separate `if`, so `class V8(var x: Int) extends AnyVal`
//! gets *only* the `var` message and still has its body checked — a `var`
//! parameter does have a getter. The three below it are one `match`, so a
//! parameter that is not a `val` never also reports `protected[this]`.
//!
//! "no getter" is spelled here as *the source did not write `val`/`var`* or
//! *it wrote `private[this]`*; nsc reaches the same set by looking for the
//! accessor symbol, which it does not create in either case.
//!
//! ## One diagnostic per position
//!
//! `trait T extends AnyVal` gets *one* message from scalac, not two, even
//! though a trait also fails the one-val-parameter rule and nothing in
//! `validateDerivedValueClass` returns early. The filter is in the reporter:
//! `FilteringReporter.duplicateOk` answers `false` for any message at a point
//! where an error has already been issued ("after an error, no further
//! messages at that position are issued"). The same rule is why a nested
//! `trait` reports only the trait message and not `value class may not be a
//! member of another class`.
//!
//! Reproducing the reporter wholesale would change every diagnostic in the
//! compiler, so it is reproduced here, over this check's own output: a
//! violation is dropped when one has already been recorded at the same span.
//! Positions that genuinely differ — the class and its parameter, on line 6 of
//! `neg/valueclasses.scala` — still get one message each, as scalac gives
//! them.
//!
//! ## What is deliberately not here
//!
//! `checkEphemeral` also rejects nested classes and objects, secondary
//! constructors, a redefined `equals`/`hashCode`, and any statement that is
//! not a definition — all under "implementation restriction: ... is not
//! allowed in value class". None of those appears in `neg/valueclasses.check`
//! and each is a rejection rule of its own, so they are left out rather than
//! guessed at. `value class may not wrap another user-defined value class` is
//! already implemented, in [`crate::cyclic::value_class_wraps_value_class`].

use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_span::Span;

use crate::symbol::{SymKind, SymbolTable};

/// One diagnostic, at the position nsc reports it (to the line — nsc points at
/// a name where we only have the whole definition's span).
pub struct Violation {
    pub span: Span,
    pub msg: &'static str,
}

/// nsc's trigger: the class's **first** parent is `AnyVal`.
///
/// First, not "any", because a universal trait (`trait T extends Any`) and a
/// value class that mixes one in (`class C(val x: Int) extends AnyVal with T`)
/// must be told apart from each other by the same rule nsc uses.
pub fn first_parent_is_anyval(st: &SymbolTable, id: SymbolId) -> bool {
    if id.is_none() {
        return false;
    }
    match st.get(id).parents.first() {
        Some(p) => {
            matches!(p, Type::AnyVal) || st.class_sym_of(p).is_some_and(|c| c == st.anyval_sym)
        }
        None => false,
    }
}

/// nsc's `Symbol.isStatic`: every owner up to the root is a package or an
/// object. A value class may be top-level or a member of an object; anything
/// else is rejected, with the message picked by whether the *immediate* owner
/// is a term (`clazz.owner.isTerm`).
fn is_static(st: &SymbolTable, id: SymbolId) -> bool {
    let mut owner = st.get(id).owner;
    // Bounded by the symbol count: `owner` strictly decreases in practice, but
    // a malformed table must not hang the compiler.
    for _ in 0..1024 {
        if owner.is_none() {
            return true;
        }
        match st.get(owner).kind {
            SymKind::Package | SymKind::NoSymbol => return true,
            SymKind::ModuleClass | SymKind::Module => owner = st.get(owner).owner,
            _ => return false,
        }
    }
    true
}

/// The nine classes nsc's `isPrimitiveValueClass` covers. They all read
/// `final abstract class Int private extends AnyVal` and have no parameter at
/// all, so the parameter rules must not apply to them — which matters the
/// moment `src/library` is the thing being compiled
/// (`tests/scalalib_measure.sh`).
fn is_primitive(st: &SymbolTable, id: SymbolId) -> bool {
    st.is_primitive_value_class(id)
        || matches!(
            st.get(id).jvm_name.as_str(),
            "scala/Int"
                | "scala/Long"
                | "scala/Float"
                | "scala/Double"
                | "scala/Char"
                | "scala/Boolean"
                | "scala/Byte"
                | "scala/Short"
                | "scala/Unit"
        )
}

/// A field definition in the body, in nsc's sense: a `val`/`var` the source
/// wrote. Constructor parameters are not in the body here (they live in
/// `vparamss`), which is what nsc's `body.filterNot(referencesUnderlying)`
/// achieves for the accessor it has already turned them into.
fn is_source_field(stat: &Tree) -> bool {
    match &stat.kind {
        TreeKind::ValDef { mods, .. } => {
            !mods.flags.contains(Flags::SYNTHETIC) && !mods.flags.contains(Flags::PARAM)
        }
        _ => false,
    }
}

/// Every violation of the value-class rules in one class definition.
///
/// Pure in the symbol table so it can be reasoned about (and tested) without
/// a `Typer`; the caller turns each [`Violation`] into a diagnostic.
pub fn violations(
    st: &SymbolTable,
    id: SymbolId,
    class_span: Span,
    is_trait: bool,
    vparamss: &[Vec<Tree>],
    tparam_trees: &[Tree],
    body: &[Tree],
) -> Vec<Violation> {
    let mut out = Vec::new();
    if !first_parent_is_anyval(st, id) {
        return out;
    }
    if is_trait {
        out.push(Violation {
            span: class_span,
            msg: "only classes (not traits) are allowed to extend AnyVal",
        });
    }
    if !is_static(st, id) {
        let owner_is_term = matches!(
            st.get(st.get(id).owner).kind,
            SymKind::Method | SymKind::Term
        );
        out.push(Violation {
            span: class_span,
            msg: if owner_is_term {
                "value class may not be a local class"
            } else {
                "value class may not be a member of another class"
            },
        });
    }
    if !is_primitive(st, id) {
        // nsc matches `clazz.primaryConstructor.paramss` against
        // `List(List(_))`: exactly one clause holding exactly one parameter.
        // A context bound counts, because the evidence clause it desugars to
        // is a second clause — scalac 2.13.16 rejects
        // `class C[T: Ordering](val x: T) extends AnyVal`, and so does this.
        let single = match vparamss {
            [only] => match only.as_slice() {
                [p] => Some(p),
                _ => None,
            },
            _ => None,
        };
        match single {
            None => out.push(Violation {
                span: class_span,
                msg: "value class needs to have exactly one val parameter",
            }),
            Some(p) => {
                // The *symbol*'s flags, not the tree's: the namer has already
                // applied nsc's two rules about what a constructor parameter
                // becomes — a bare one is `private[this]`, and a `case class`
                // makes every one of them a public `val`. Reading the written
                // modifiers instead cost cats one false diagnostic on
                // `final case class ShowInterpolator(_sc: StringContext)
                // extends AnyVal`, which scalac accepts.
                let flags = if p.sym.is_none() {
                    match &p.kind {
                        TreeKind::ValDef { mods, .. } => mods.flags,
                        _ => Flags::EMPTY,
                    }
                } else {
                    st.get(p.sym).flags
                };
                if flags.contains(Flags::MUTABLE) {
                    out.push(Violation {
                        span: p.span,
                        msg: "value class parameter must not be a var",
                    });
                }
                // No getter, in nsc's terms: `private[this]`, which is both
                // what the source can write and what a bare parameter became
                // above.
                let local = flags.contains(Flags::LOCAL);
                if local && flags.contains(Flags::PRIVATE) {
                    out.push(Violation {
                        span: p.span,
                        msg: "value class parameter must be a val and not be private[this]",
                    });
                } else if local && flags.contains(Flags::PROTECTED) {
                    out.push(Violation {
                        span: p.span,
                        msg: "value class parameter must not be protected[this]",
                    });
                } else {
                    for stat in body {
                        if is_source_field(stat) {
                            out.push(Violation {
                                span: stat.span,
                                msg: "field definition is not allowed in value class",
                            });
                        }
                    }
                }
            }
        }
    }
    // nsc checks this one whatever the parameters look like, and outside the
    // `isPrimitiveValueClass` guard.
    let tparam_syms = &st.get(id).tparams;
    for (i, tp) in tparam_trees.iter().enumerate() {
        let Some(&sym) = tparam_syms.get(i) else {
            break;
        };
        if !sym.is_none() && st.get(sym).specialized.is_some() {
            out.push(Violation {
                span: tp.span,
                msg: "type parameter of value class may not be specialized",
            });
        }
    }
    // nsc's reporter, applied to this check's own output: the first error at a
    // position hides every later one there. See the module header.
    let mut seen: Vec<Span> = Vec::new();
    out.retain(|v| {
        let fresh = !seen.contains(&v.span);
        if fresh {
            seen.push(v.span);
        }
        fresh
    });
    out
}
