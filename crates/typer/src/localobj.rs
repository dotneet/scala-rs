//! Diagnose the nested-`object` shapes that cannot be compiled: an `object`
//! declared *inside a method* that reads anything outside itself, and an
//! `object` inside a value class (which nsc rejects outright — "implementation
//! restriction: nested object is not allowed in value class").
//!
//! A member `object` of a class or trait is compiled the way nsc compiles it —
//! an `$outer` field, a constructor taking the enclosing instance, and a
//! lazily initialised `<name>$module` field with a `<name>()` accessor on the
//! enclosing template. A *local* `object` is a different shape: nsc gives it
//! one instance per call, held in a `scala.runtime.LazyRef` local of the
//! enclosing method, and hands its constructor both the enclosing instance and
//! every captured local. scala-rs still emits a local `object` as a static
//! singleton, which is only right while the body reads nothing from outside;
//! reading the enclosing instance produced `NoSuchFieldError: $outer` at run
//! time, and reading a captured local produced a reference to a class named
//! after the method. Until the `LazyRef` shape is implemented, refuse those at
//! compile time instead of emitting something that cannot run.

use std::collections::HashSet;

use scala_rs_parser::ast::Flags;
use scala_rs_parser::{SymbolId, Tree, TreeKind};
use scala_rs_span::Diagnostic;

use crate::symbol::{SymKind, SymbolTable};

/// Errors for the nested-`object` shapes that cannot be compiled: a local
/// `object` whose body reaches outside itself, and one written inside a value
/// class (which nsc rejects outright).
pub fn check_local_objects(file_index: usize, tree: &Tree, st: &SymbolTable) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    walk(file_index, tree, st, &mut out);
    out
}

/// A local `case class` whose synthetic companion would need to capture an
/// enclosing-method local.
///
/// `P(args)` compiles to a call through the companion's `apply`
/// (`crates/backend/src/gen.rs`, `emit_case_apply`), and the companion is
/// emitted as a static `MODULE$` singleton
/// (`crates/backend/src/gen.rs`, `emit_case_companion`) exactly like any
/// other local `object` -- the same shape `check_local_objects` above refuses
/// once its body reads outside itself. A case class's companion has no body
/// in source for that check to walk (it is synthesized, not written), so this
/// is a second, narrower check: run *after* `mark_anon_captures` has filled
/// in `Symbol::captures`, and refuse a local case class whose free-variable
/// list is non-empty. Until the companion gets the same `LazyRef` treatment a
/// capturing local `object` would need, `P(1)` for such a class would type-check
/// and then throw `NoSuchMethodError` building the real `<init>` at run time
/// (the class itself does correctly gain a capture constructor parameter; only
/// the companion's `apply` never learns to supply it).
pub fn check_local_case_class_captures(
    file_index: usize,
    tree: &Tree,
    st: &SymbolTable,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    walk_case_captures(file_index, tree, st, &mut out);
    out
}

fn walk_case_captures(file_index: usize, tree: &Tree, st: &SymbolTable, out: &mut Vec<Diagnostic>) {
    if let TreeKind::ClassDef { name, mods, .. } = &tree.kind {
        if mods.flags.contains(Flags::CASE)
            && !tree.sym.is_none()
            && is_local_case_class(st, tree.sym)
            && !st.get(tree.sym).captures.is_empty()
        {
            out.push(Diagnostic::error(
                file_index,
                tree.span,
                format!(
                    "not implemented: a local `case class {name}` that reads a local \
                     of the enclosing method (its synthetic companion would have to \
                     capture it too, the same shape a local `object` needs and cannot \
                     get yet). Move it out of the method, or drop `case` and write an \
                     ordinary local `class`."
                ),
            ));
        }
    }
    each_child(tree, &mut |c| walk_case_captures(file_index, c, st, out));
}

/// A `class` written inside a method body: owned by the method (or by a
/// `val` inside one), not by a template.
fn is_local_case_class(st: &SymbolTable, id: SymbolId) -> bool {
    let owner = st.get(id).owner;
    if owner.is_none() {
        return false;
    }
    matches!(st.get(owner).kind, SymKind::Method | SymKind::Term)
}

fn walk(file_index: usize, tree: &Tree, st: &SymbolTable, out: &mut Vec<Diagnostic>) {
    if let TreeKind::ModuleDef { name, impl_, .. } = &tree.kind {
        // nsc's own restriction, word for word: a value class has no instance
        // to hang the object's `$outer` on.
        if !tree.sym.is_none() && st.is_value_class(st.get(module_class_of(st, tree.sym)).owner) {
            out.push(
                Diagnostic::error(
                    file_index,
                    tree.span,
                    "implementation restriction: nested object is not allowed in value class",
                )
                .note("This restriction is planned to be removed in subsequent releases."),
            );
        }
        if !tree.sym.is_none() && is_local_module(st, tree.sym) {
            let mcls = module_class_of(st, tree.sym);
            let mut bad = None;
            for p in &impl_.parents {
                find_outside_ref(p, st, mcls, &mut bad);
            }
            for s in &impl_.body {
                find_outside_ref(s, st, mcls, &mut bad);
            }
            if let Some(what) = bad {
                out.push(Diagnostic::error(
                    file_index,
                    tree.span,
                    format!(
                        "not implemented: a local `object` that reads {what} \
                         (`object {name}` is declared inside a method). \
                         Move it out of the method, or make it a `lazy val`."
                    ),
                ));
            }
        }
    }
    each_child(tree, &mut |c| walk(file_index, c, st, out));
}

/// The module class of an `object` symbol.
fn module_class_of(st: &SymbolTable, id: SymbolId) -> SymbolId {
    match st.get(id).ty {
        scala_rs_parser::Type::ModuleRef(c) => c,
        _ => id,
    }
}

/// An `object` written inside a method body: its module class is owned by a
/// method (or by a `val` inside one), not by a template.
fn is_local_module(st: &SymbolTable, id: SymbolId) -> bool {
    let cls = module_class_of(st, id);
    let owner = st.get(cls).owner;
    if owner.is_none() {
        return false;
    }
    matches!(st.get(owner).kind, SymKind::Method | SymKind::Term)
}

/// What the body reaches for that the singleton shape cannot supply.
fn find_outside_ref(tree: &Tree, st: &SymbolTable, mcls: SymbolId, out: &mut Option<String>) {
    if out.is_some() {
        return;
    }
    // Only a reference with no qualifier can mean "the enclosing instance":
    // `k * 3` selects `*` on `k`, and `xs.head` on a member of some other
    // class, neither of which needs anything the singleton shape lacks.
    if let TreeKind::This { .. } = &tree.kind {
        if !tree.sym.is_none() && !conforms(st, mcls, tree.sym) {
            *out = Some("the enclosing instance".into());
            return;
        }
    }
    if matches!(tree.kind, TreeKind::Ident { .. }) && !tree.sym.is_none() {
        let s = st.get(tree.sym);
        let owner = s.owner;
        if !owner.is_none() && matches!(s.kind, SymKind::Term | SymKind::Method) {
            match st.get(owner).kind {
                // A local of the enclosing method would have to become a
                // constructor argument of the object.
                SymKind::Method | SymKind::Term if !owns(st, mcls, owner) => {
                    *out = Some("a local of the enclosing method".into());
                    return;
                }
                // A member of an enclosing class needs the `$outer` the
                // singleton shape has no way to hold.
                SymKind::Class if !conforms(st, mcls, owner) => {
                    *out = Some("the enclosing instance".into());
                    return;
                }
                _ => {}
            }
        }
    }
    each_child(tree, &mut |c| find_outside_ref(c, st, mcls, out));
}

/// Is `owner` `mcls` itself or one of its own owners? A `def` nested inside
/// the object owns its own locals, and those are fine.
fn owns(st: &SymbolTable, mcls: SymbolId, owner: SymbolId) -> bool {
    let mut cur = owner;
    while !cur.is_none() {
        if cur == mcls {
            return true;
        }
        cur = st.get(cur).owner;
    }
    false
}

/// `current` is `owner` or inherits from it.
fn conforms(st: &SymbolTable, current: SymbolId, owner: SymbolId) -> bool {
    if owner.is_none() || current == owner {
        return true;
    }
    let mut work = vec![current];
    let mut seen = HashSet::new();
    while let Some(id) = work.pop() {
        if !seen.insert(id.0) {
            continue;
        }
        if id == owner {
            return true;
        }
        for p in &st.get(id).parents {
            if let Some(ps) = st.class_sym_of(p) {
                work.push(ps);
            }
        }
    }
    false
}

/// Every sub-tree that can hold expressions or definitions.
fn each_child(tree: &Tree, f: &mut dyn FnMut(&Tree)) {
    match &tree.kind {
        TreeKind::PackageDef { stats, .. } => stats.iter().for_each(f),
        TreeKind::ClassDef {
            vparamss, impl_, ..
        } => {
            vparamss.iter().flatten().for_each(&mut *f);
            impl_.parents.iter().for_each(&mut *f);
            impl_.body.iter().for_each(f);
        }
        TreeKind::ModuleDef { impl_, .. } => {
            impl_.parents.iter().for_each(&mut *f);
            impl_.body.iter().for_each(f);
        }
        TreeKind::ValDef { tpt, rhs, .. } => {
            f(tpt);
            f(rhs);
        }
        TreeKind::DefDef {
            vparamss, tpt, rhs, ..
        } => {
            vparamss.iter().flatten().for_each(&mut *f);
            f(tpt);
            f(rhs);
        }
        TreeKind::Block { stats, expr } => {
            stats.iter().for_each(&mut *f);
            f(expr);
        }
        TreeKind::If { cond, thenp, elsep } => {
            f(cond);
            f(thenp);
            f(elsep);
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { cond, body } => {
            f(cond);
            f(body);
        }
        TreeKind::Function { vparams, body } => {
            vparams.iter().for_each(&mut *f);
            f(body);
        }
        TreeKind::Apply { fun, args }
        | TreeKind::TypeApply { fun, args }
        | TreeKind::UnApply { fun, args } => {
            f(fun);
            args.iter().for_each(f);
        }
        TreeKind::Typed { expr, tpt } => {
            f(expr);
            f(tpt);
        }
        TreeKind::Select { qual, .. } => f(qual),
        TreeKind::Assign { lhs, rhs } => {
            f(lhs);
            f(rhs);
        }
        TreeKind::Match { selector, cases } => {
            f(selector);
            for c in cases {
                f(&c.pat);
                f(&c.guard);
                f(&c.body);
            }
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            f(block);
            for c in catches {
                f(&c.pat);
                f(&c.guard);
                f(&c.body);
            }
            f(finalizer);
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } | TreeKind::New { tpt: expr } => {
            f(expr)
        }
        TreeKind::InterpolatedString { args, .. } => args.iter().for_each(f),
        TreeKind::LabelDef { params, rhs, .. } => {
            params.iter().for_each(&mut *f);
            f(rhs);
        }
        _ => {}
    }
}
