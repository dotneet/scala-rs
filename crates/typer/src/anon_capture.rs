//! Free-variable analysis for classes nested inside a method body.
//!
//! `new T { … }` and local `class C { … }` may read `val`s / `var`s /
//! parameters of the enclosing method. nsc turns each such free variable into
//! a field of the nested class and passes it as an extra constructor argument.
//! This pass only records *what* has to be captured, on
//! [`Symbol::captures`](crate::Symbol::captures); the backend turns the list
//! into fields, constructor parameters and `new` arguments.
//!
//! The order of the recorded vector is the constructor parameter order, so it
//! has to stay stable between the class emitter and the `new` call site.

use std::collections::HashSet;

use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind};

use crate::symbol::{SymKind, SymbolTable};

/// Record captured enclosing-method locals on every class symbol in `tree`.
pub fn mark_anon_captures(tree: &Tree, st: &mut SymbolTable) {
    let mut found: Vec<(SymbolId, Vec<SymbolId>)> = Vec::new();
    let mut classes: Vec<SymbolId> = Vec::new();
    walk(tree, st, &mut found, &mut classes);
    for (cls, caps) in found {
        st.get_mut(cls).captures = caps;
    }
    inherit_trait_captures(st, &classes);
    mark_outer_captures(tree, st);
}

/// Mark the local/anonymous classes whose bodies really need the enclosing
/// instance.  The hidden constructor argument is an ABI detail and is kept
/// for every non-static nested class, but scalac does not materialise an
/// `$outer` field for an anonymous class that never reads the enclosing
/// `this` or one of its members.  Keeping that distinction is important for
/// serializable typeclass singletons: an otherwise unused outer object would
/// make Java serialization walk into a non-serializable package module.
fn mark_outer_captures(tree: &Tree, st: &mut SymbolTable) {
    match &tree.kind {
        TreeKind::ClassDef { impl_, .. } => {
            if !tree.sym.is_none() && is_local_or_anonymous(st, tree.sym) {
                let needed = class_uses_outer(tree, st, tree.sym);
                st.get_mut(tree.sym).captures_outer = needed;
            }
            // A nested class is a separate lexical body.  Its use of an
            // enclosing member must not accidentally mark this class too.
            for p in &impl_.parents {
                mark_outer_captures(p, st);
            }
            for s in &impl_.body {
                mark_outer_captures(s, st);
            }
        }
        TreeKind::ModuleDef { impl_, .. } => {
            for p in &impl_.parents {
                mark_outer_captures(p, st);
            }
            for s in &impl_.body {
                mark_outer_captures(s, st);
            }
        }
        _ => each_child(tree, &mut |c| mark_outer_captures(c, st)),
    }
}

fn is_local_or_anonymous(st: &SymbolTable, id: SymbolId) -> bool {
    let s = st.get(id);
    if s.name.starts_with("$anon$") {
        return true;
    }
    matches!(st.get(s.owner).kind, SymKind::Method | SymKind::Term)
}

/// The lexical class around a source class.  This intentionally mirrors the
/// backend's `enclosing_instance`: method/block owners are implementation
/// scopes, while a class owner is the instance that can supply members.
fn enclosing_instance(st: &SymbolTable, class_id: SymbolId) -> Option<SymbolId> {
    if class_id.is_none() {
        return None;
    }
    let s = st.get(class_id);
    if s.flags.contains(Flags::JAVA) || s.flags.contains(Flags::STATIC) {
        return None;
    }
    let mut owner = s.owner;
    while !owner.is_none() && matches!(st.get(owner).kind, SymKind::Method | SymKind::Term) {
        owner = st.get(owner).owner;
    }
    let o = st.get(owner);
    if o.kind == SymKind::Class && !o.flags.contains(Flags::MODULE) {
        return Some(owner);
    }
    // A class declared in a member `object` is still per enclosing instance;
    // top-level objects remain static singletons.  Keep this small mirror of
    // the backend's `member_module_outer` so an anonymous class in the former
    // can be marked when it reads the object's members.
    if (o.kind == SymKind::ModuleClass || o.flags.contains(Flags::MODULE))
        && module_has_outer(st, owner)
    {
        return Some(owner);
    }
    None
}

fn module_has_outer(st: &SymbolTable, module: SymbolId) -> bool {
    let owner = st.get(module).owner;
    if owner.is_none() {
        return false;
    }
    let o = st.get(owner);
    if o.kind == SymKind::Class && !o.flags.contains(Flags::MODULE) {
        return true;
    }
    (o.kind == SymKind::ModuleClass || o.flags.contains(Flags::MODULE))
        && module_has_outer(st, owner)
}

fn outer_chain(st: &SymbolTable, class_id: SymbolId) -> Vec<SymbolId> {
    let mut out = Vec::new();
    let mut cur = class_id;
    let mut seen = HashSet::new();
    while let Some(owner) = enclosing_instance(st, cur) {
        if !seen.insert(owner.0) {
            break;
        }
        out.push(owner);
        cur = owner;
    }
    out
}

fn outer_supplies_owner(st: &SymbolTable, outer: SymbolId, owner: SymbolId) -> bool {
    if owner == outer || st.is_ancestor_of(owner, outer) {
        return true;
    }
    st.get(outer).self_type.as_ref().is_some_and(|ty| {
        st.self_type_classes(ty)
            .into_iter()
            .any(|c| c == owner || st.is_ancestor_of(owner, c))
    })
}

fn member_needs_outer(st: &SymbolTable, current: SymbolId, id: SymbolId) -> bool {
    if id.is_none() {
        return false;
    }
    let s = st.get(id);
    if !matches!(s.kind, SymKind::Term | SymKind::Method) || s.owner.is_none() {
        return false;
    }
    let owner = s.owner;
    // A template self alias is not an inherited member.  It denotes the
    // *particular enclosing instance* whose template declared it, even when
    // the anonymous class also inherits that template.  Treating it like an
    // ordinary inherited member made a lambda in `new Rep { self.index(...) }`
    // emit a `$outer` read while the capture pass omitted the field.
    if st.get(owner).self_alias == Some(id)
        && outer_chain(st, current)
            .into_iter()
            .any(|outer| outer == owner)
    {
        return true;
    }
    // A member inherited by the class being emitted is reached through its
    // own `this`; only a member belonging to a lexical owner outside it is an
    // enclosing-instance read.
    if owner == current || st.is_ancestor_of(owner, current) {
        return false;
    }
    outer_chain(st, current)
        .into_iter()
        .any(|outer| outer_supplies_owner(st, outer, owner))
}

/// Does a source parent name the instance of `outer` explicitly (`p.C`)?
///
/// This is the typer-side counterpart of the backend's
/// `parent_prefix_instance`.  Such a prefix is captured directly (when it is
/// a local), so it must not force retention of the anonymous class's lexical
/// `$outer`.
fn parent_has_instance_prefix(st: &SymbolTable, parent: &Tree, outer: SymbolId) -> bool {
    let mut head = parent;
    while let TreeKind::Apply { fun, .. } = &head.kind {
        head = fun;
    }
    prefix_supplies_outer(st, head, outer)
}

fn prefix_supplies_outer(st: &SymbolTable, tree: &Tree, outer: SymbolId) -> bool {
    let qual = match &tree.kind {
        TreeKind::Select { qual, .. } => qual,
        TreeKind::AppliedTypeTree { tpt, .. }
        | TreeKind::TypeApply { fun: tpt, .. }
        | TreeKind::AnnotatedTypeTree { tpt, .. } => {
            return prefix_supplies_outer(st, tpt, outer);
        }
        _ => return false,
    };
    st.class_sym_of(&qual.ty)
        .is_some_and(|p| outer_supplies_owner(st, p, outer))
}

/// A nested trait has no field of its own: its default methods call an
/// expanded outer accessor implemented by the anonymous class.  Therefore a
/// class with an otherwise empty body can still need its lexical `$outer`.
/// Keep it only when that lexical chain supplies the trait's enclosing
/// instance; path-prefixed parents (`new p.T {}`) return `p` directly.
fn inherited_trait_needs_outer(st: &SymbolTable, current: SymbolId, parents: &[Tree]) -> bool {
    let outers = outer_chain(st, current);
    crate::lin::linearize(st, current)
        .into_iter()
        .skip(1)
        .filter(|&parent| crate::lin::is_interface(st, parent))
        .any(|parent| {
            let Some(want) = enclosing_instance(st, parent) else {
                return false;
            };
            outers
                .iter()
                .copied()
                .any(|outer| outer_supplies_owner(st, outer, want))
                && !parents
                    .iter()
                    .any(|p| parent_has_instance_prefix(st, p, want))
        })
}

fn this_needs_outer(st: &SymbolTable, current: SymbolId, id: SymbolId) -> bool {
    if id.is_none() || id == current {
        return false;
    }
    let outers = outer_chain(st, current);
    // A qualified `Outer.this` / `Outer.super` names the lexical instance by
    // identity even when the anonymous class happens to inherit `Outer`.
    // Test the lexical chain before the ordinary inherited-`this` shortcut.
    if outers.contains(&id) {
        return true;
    }
    if st.is_ancestor_of(id, current) {
        return false;
    }
    outers.into_iter().any(|outer| st.is_ancestor_of(id, outer))
}

/// Scan one class body without descending into nested class/module bodies.
fn class_uses_outer(class_def: &Tree, st: &SymbolTable, current: SymbolId) -> bool {
    let TreeKind::ClassDef { impl_, .. } = &class_def.kind else {
        return false;
    };
    inherited_trait_needs_outer(st, current, &impl_.parents)
        || impl_.parents.iter().any(|t| scan_outer(t, st, current))
        || impl_.body.iter().any(|t| scan_outer(t, st, current))
}

fn scan_outer(tree: &Tree, st: &SymbolTable, current: SymbolId) -> bool {
    match &tree.kind {
        TreeKind::Ident { .. } => member_needs_outer(st, current, tree.sym),
        TreeKind::This { .. } | TreeKind::Super { .. } => this_needs_outer(st, current, tree.sym),
        TreeKind::ClassDef { .. } | TreeKind::ModuleDef { .. } => false,
        _ => {
            let mut found = false;
            each_child(tree, &mut |c| {
                if !found && scan_outer(c, st, current) {
                    found = true;
                }
            });
            found
        }
    }
}

/// A local `trait` has no constructor, so it cannot hold the enclosing-method
/// locals its own body reads: the value has to come from the instance. Every
/// class mixing such a trait in therefore captures what the trait captures
/// too, and exposes each one through an accessor the trait's implementation
/// reads back (`crates/backend/src/gen.rs`, `trait_capture_accessors`).
///
/// Only the *direct* captures of each ancestor are added, but the whole
/// linearization is consulted, so one pass reaches every level.
fn inherit_trait_captures(st: &mut SymbolTable, classes: &[SymbolId]) {
    let mut updates: Vec<(SymbolId, Vec<SymbolId>)> = Vec::new();
    for &cls in classes {
        let mut caps = st.get(cls).captures.clone();
        let before = caps.len();
        for p in crate::lin::linearize(st, cls).into_iter().skip(1) {
            if !crate::lin::is_interface(st, p) {
                continue;
            }
            for c in st.get(p).captures.clone() {
                if !caps.contains(&c) {
                    caps.push(c);
                }
            }
        }
        if caps.len() != before {
            updates.push((cls, caps));
        }
    }
    for (cls, caps) in updates {
        st.get_mut(cls).captures = caps;
    }
}

fn walk(
    tree: &Tree,
    st: &SymbolTable,
    out: &mut Vec<(SymbolId, Vec<SymbolId>)>,
    classes: &mut Vec<SymbolId>,
) {
    if let TreeKind::ClassDef { .. } = &tree.kind {
        if !tree.sym.is_none() {
            classes.push(tree.sym);
            let caps = class_captures(tree, st);
            if !caps.is_empty() {
                out.push((tree.sym, caps));
            }
        }
    }
    each_child(tree, &mut |c| walk(c, st, out, classes));
}

/// Free enclosing-method terms of a `ClassDef`, in first-reference order.
fn class_captures(class_def: &Tree, st: &SymbolTable) -> Vec<SymbolId> {
    let TreeKind::ClassDef {
        vparamss, impl_, ..
    } = &class_def.kind
    else {
        return Vec::new();
    };
    let mut bound: HashSet<SymbolId> = HashSet::new();
    for p in vparamss.iter().flatten() {
        if !p.sym.is_none() {
            bound.insert(p.sym);
        }
    }
    // Class members bind over the whole template, not just later statements.
    for s in &impl_.body {
        if !s.sym.is_none() {
            bound.insert(s.sym);
        }
    }
    let mut out = Vec::new();
    for p in &impl_.parents {
        free(p, &bound, &mut out, st);
    }
    for s in &impl_.body {
        free(s, &bound, &mut out, st);
    }
    out
}

fn consider(id: SymbolId, bound: &HashSet<SymbolId>, out: &mut Vec<SymbolId>, st: &SymbolTable) {
    if id.is_none() || bound.contains(&id) || out.contains(&id) {
        return;
    }
    let s = st.get(id);
    if s.kind != SymKind::Term || s.owner.is_none() {
        return;
    }
    // Method-owned terms are locals; class members are reached via `this`.
    // A local of a block written directly in a template (`object T extends
    // App { { val x = 2; class L { def get = x } } }`) is owned by the class
    // yet is no member of it -- it lives in the constructor's frame -- and a
    // class defined in that block captures it the same way (the rule
    // `lambda_lift::consider_capture` already applies to nested defs).
    // Without it `L.get` read `x` off `this`: `NoSuchFieldError: x`.
    let owner = st.get(s.owner);
    // A self alias (`trait Named { self => ... }`) is owned by the class and
    // is no member either, but it is `this`, not a local.
    let template_local = owner.is_class_like()
        && !s.flags.contains(scala_rs_parser::Flags::PARAM)
        && owner.self_alias != Some(id)
        && !owner.members.contains(&id);
    if owner.kind != SymKind::Method && !template_local {
        return;
    }
    out.push(id);
}

fn free(tree: &Tree, bound: &HashSet<SymbolId>, out: &mut Vec<SymbolId>, st: &SymbolTable) {
    match &tree.kind {
        TreeKind::Ident { .. } => consider(tree.sym, bound, out, st),
        TreeKind::Function { vparams, body } => {
            let mut b = bound.clone();
            for p in vparams {
                if !p.sym.is_none() {
                    b.insert(p.sym);
                }
            }
            free(body, &b, out, st);
        }
        TreeKind::Block { stats, expr } => {
            let mut b = bound.clone();
            for s in stats {
                if matches!(s.kind, TreeKind::ValDef { .. }) && !s.sym.is_none() {
                    b.insert(s.sym);
                }
                free(s, &b, out, st);
            }
            free(expr, &b, out, st);
        }
        TreeKind::DefDef { vparamss, rhs, .. } => {
            let mut b = bound.clone();
            for p in vparamss.iter().flatten() {
                if !p.sym.is_none() {
                    b.insert(p.sym);
                }
            }
            for p in vparamss.iter().flatten() {
                free(p, &b, out, st);
            }
            free(rhs, &b, out, st);
        }
        TreeKind::ClassDef { .. } => {
            // A nested class re-exports what it captures: the value has to be
            // reachable here so the inner `new` can forward it.
            for id in class_captures(tree, st) {
                consider(id, bound, out, st);
            }
        }
        TreeKind::Match { selector, cases } => {
            free(selector, bound, out, st);
            for c in cases {
                let mut b = bound.clone();
                pattern_binders(&c.pat, &mut b);
                free(&c.pat, &b, out, st);
                free(&c.guard, &b, out, st);
                free(&c.body, &b, out, st);
            }
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            free(block, bound, out, st);
            for c in catches {
                let mut b = bound.clone();
                pattern_binders(&c.pat, &mut b);
                free(&c.pat, &b, out, st);
                free(&c.guard, &b, out, st);
                free(&c.body, &b, out, st);
            }
            free(finalizer, bound, out, st);
        }
        TreeKind::LabelDef { params, rhs, .. } => {
            let mut b = bound.clone();
            for p in params {
                if !p.sym.is_none() {
                    b.insert(p.sym);
                }
            }
            free(rhs, &b, out, st);
        }
        _ => each_child(tree, &mut |c| free(c, bound, out, st)),
    }
}

/// Names a pattern introduces: a `Bind`, and an `Ident` that is a variable
/// rather than a stable identifier -- the same lowercase test the typer uses.
fn pattern_binders(pat: &Tree, out: &mut HashSet<SymbolId>) {
    // A stable pattern is an expression, including any lowered accessor
    // arguments. Its referenced locals are captures, never new binders.
    if pat.stable_pat {
        return;
    }
    match &pat.kind {
        TreeKind::Bind { .. } => {
            if !pat.sym.is_none() {
                out.insert(pat.sym);
            }
        }
        TreeKind::Ident { name } => {
            let varid = scala_rs_parser::ast::is_variable_name(name);
            if varid && !pat.sym.is_none() {
                out.insert(pat.sym);
            }
        }
        _ => {}
    }
    each_child(pat, &mut |c| pattern_binders(c, out));
}

/// Visit every sub-tree that can hold expressions.
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
        // Pattern shapes. Without these the walk stops at the first `x @ p`,
        // so the binders *inside* `p` never reach `pattern_binders`, the case
        // body's reads of them look free, and the enclosing trait ends up
        // "capturing" its own method's pattern variables.
        TreeKind::Bind { body, .. } => f(body),
        TreeKind::Star { elem } => f(elem),
        TreeKind::Alternative { trees } => trees.iter().for_each(f),
        TreeKind::LabelDef { params, rhs, .. } => {
            params.iter().for_each(&mut *f);
            f(rhs);
        }
        _ => {}
    }
}
