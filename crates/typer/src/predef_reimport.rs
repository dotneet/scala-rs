//! `Predef._` is an import, not a snapshot.
//!
//! nsc opens `java.lang._`, `scala._` and `Predef._` around every compilation
//! unit, and `Predef` there means whatever `scala.Predef` resolves to. This
//! compiler models the third one by *copying* the prelude's `Predef` members
//! into the base scope at install time (`prelude::import_members`), which is
//! taken before any source has been read. When the run's own sources define
//! `scala.Predef` — which is what compiling `src/library` does — that copy is
//! of a `Predef` the program does not have, and the one it does have
//! contributes nothing.
//!
//! `docs/scala-library.md` records the same defect for the `scala._` half
//! ("`scala._` was a snapshot, not an import"), fixed by
//! `Typer::auto_import_scala_member`. That fix enters a source *class or
//! object* that lands directly in package `scala`; it says nothing about the
//! members of a source `Predef`, which is a separate scope.
//!
//! What it costs, measured on `tests/scalalib_measure.sh`: 55 of the 1420
//! errors are `value max is not a member of Int` (34) and `value min is not a
//! member of Int` (21). `max` reaches `Int` through
//! `Predef.intWrapper(x): runtime.RichInt`, `intWrapper` is declared in
//! `LowPriorityImplicits` which the source `object Predef` extends, and the
//! measurement runs `--no-scala-library`, where the prelude builds no
//! `RichInt` and no `intWrapper` at all. So the source declaration is the only
//! one in existence, it is a perfectly ordinary implicit, nothing shadows it,
//! and no jar copy is involved — it was simply never in scope. Writing
//! `import scala.Predef._` by hand in the same file makes it resolve.
//!
//! Timing: the members of a source `object Predef` do not exist until the
//! signature pass has run, so this cannot be done in the namer the way
//! `auto_import_scala_member` is. It runs from `check::typecheck_units`
//! between the signature pass and the body pass — the point where every
//! unit's signatures are built and no body has been typed yet.

use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::SymbolId;

/// Enter the members of a **source-defined** `scala.Predef` into the prelude
/// scope, the way `import scala.Predef._` would.
///
/// Does nothing unless the run's sources actually define `scala.Predef`; for
/// every ordinary program the prelude's own snapshot is the whole story and
/// this is a no-op.
pub(crate) fn reimport_source_predef(st: &mut SymbolTable) {
    let Some(module) = source_predef(st) else {
        return;
    };
    // `import o._` imports what `o` *has*, not only what it declares (SLS 4.7):
    // the library writes `object Predef extends LowPriorityImplicits`, and
    // `intWrapper` is declared on that parent. Breadth-first from the module,
    // so a name `Predef` declares itself is entered ahead of the one it
    // inherits. The module *class* is where the namer hangs the body's
    // members and the parents; the module value is walked too because some
    // members are attached there instead (`prelude_conform` does exactly
    // that with `$conforms`).
    let cls = st.module_class_of(module);
    let mut work = std::collections::VecDeque::from([module, cls]);
    let mut walked = std::collections::HashSet::new();
    let mut entered: Vec<(String, SymbolId)> = Vec::new();
    while let Some(cur) = work.pop_front() {
        if cur.is_none() || !walked.insert(cur.0) {
            continue;
        }
        // A `private` member of a *strict* ancestor is not inherited (SLS
        // 5.2), so `o` does not have it and no import can name it. Same rule
        // the real wildcard import in `check_name` applies.
        let inherited = cur != module && cur != cls;
        for m in st.get(cur).members.clone() {
            if inherited && st.private_to_owner(m) {
                continue;
            }
            let name = st.get(m).name.clone();
            if name.ends_with('$') || name == "<init>" {
                continue;
            }
            // Every member, with no dedupe by name -- exactly what the real
            // `import o._` in `check_name` does, and for the same reason: a
            // name may be *overloaded*. `Predef` declares both
            // `require(Boolean)` and `require(Boolean, => Any)`, and keeping
            // only the first cost 17 errors reading `no matching overload for
            // (Boolean)Unit with arguments (Boolean, String)` in nine files --
            // every two-argument `require` and `assert` in the library.
            // Breadth-first ordering is what separates an override from what
            // it overrides; the scope slot keeps the rest as an overload set.
            entered.push((name, m));
        }
        for p in st.get(cur).parents.clone() {
            if let Some(ps) = st.class_sym_of(&p) {
                work.push_back(ps);
            }
        }
    }
    for (name, m) in entered {
        enter_replacing_prelude(st, &name, m);
    }
    // The prelude's `Predef` is now a stand-in whose original has arrived.
    // `enter_replacing_prelude` above has displaced every member the two
    // spell the same way; `Check::drop_superseded_prelude_conversions` uses
    // this flag for the ones they do not.
    st.predef_superseded = true;
    // Record the import itself, not only what it brought in. A member reached
    // this way is usually *inherited* by `Predef` -- `intWrapper` is declared
    // on `LowPriorityImplicits` -- and its owner is therefore a plain class.
    // Codegen asks `Typer::wildcard_module_for` which object an import
    // supplied such a member through, and without a wildcard recorded here it
    // has nothing to answer with: it emits the call on `this`, and
    // `3 bigger 7` compiled clean and then died at run time with
    // `class Main$ cannot be cast to class scala.LowPriorityProbe`. With the
    // wildcard the receiver is `scala.Predef$.MODULE$`, which is what nsc
    // emits. `tests/fixtures/libmaxmin_predef.scala` is that program, and it
    // is run, not just compiled.
    let i = st.prelude_scope;
    if let Some(sc) = st.scopes.get_mut(i) {
        // The module *class*, not the module value: `inherits_from`, which is
        // how `wildcard_module_for` decides whether an import supplied an
        // inherited member, walks `parents`, and a module value carries none
        // -- `object Predef extends LowPriorityImplicits` hangs that parent on
        // `Predef$`. Recording the value instead type-checked and then emitted
        // the call on `this`.
        sc.enter_wildcard(cls, &[]);
    }
}

/// Bind `id` under `name` in the prelude scope, **replacing** the prelude's
/// own snapshot of that name rather than joining it.
///
/// Entering it alongside is not enough, and says so out loud: the prelude's
/// `int2Integer` and the source `Predef`'s are two distinct symbols with the
/// same signature, so every `x: java.lang.Integer = someInt` became
/// `ambiguous implicit: int2Integer, int2Integer` -- 16 of them across
/// `src/library`, where there had been 2 ambiguities in the whole run. They
/// are not two candidates. The source definition is the one the program has;
/// the prelude's is this compiler's stand-in for it, and a stand-in whose
/// original has arrived is not a second overload.
///
/// This is `SymbolTable::shadow_supplied_by_source` one level down: that
/// function replaces a prelude *class or object* a source definition
/// supersedes, and this replaces a prelude *member of `Predef`* the same way.
/// Only prelude symbols are displaced (`id < prelude_end`); a binding that
/// came from source or from a classfile is left exactly where it is.
fn enter_replacing_prelude(st: &mut SymbolTable, name: &str, id: SymbolId) {
    let i = st.prelude_scope;
    let prelude_end = st.prelude_end;
    let Some(sc) = st.scopes.get_mut(i) else {
        return;
    };
    // Every binding of the name, not `Scope::lookup`'s best-precedence subset:
    // a prelude entry sitting at a worse rank still competes in implicit
    // search, which searches the scope rather than resolving a name.
    let victims: Vec<SymbolId> = sc
        .lookup_ranked(name)
        .iter()
        .map(|b| b.sym)
        .filter(|v| v.0 < prelude_end && *v != id)
        .collect();
    if victims.is_empty() {
        sc.enter(name, id);
    } else {
        sc.replace(name, &victims, id);
    }
}

/// The run's own `scala.Predef`, when its sources define one.
///
/// `SymbolTable::predef` is the prelude's, and stays so for the whole run: its
/// id is written into prelude signatures that are still in use, which is why
/// `shadow_supplied_by_source` makes a replaced symbol unreachable by name
/// rather than rewriting it away. So the source one has to be found by
/// looking, and it is identified the same way that function identifies a
/// source definition — an id at or past `prelude_end`.
fn source_predef(st: &SymbolTable) -> Option<SymbolId> {
    if st.scala_pkg.is_none() {
        return None;
    }
    let prelude_end = st.prelude_end;
    st.get(st.scala_pkg).members.iter().copied().find(|&m| {
        m.0 >= prelude_end
            && m != st.predef
            && st.get(m).kind == SymKind::Module
            && st.get(m).name == "Predef"
    })
}
