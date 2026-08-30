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

use scala_rs_parser::{SymbolId, Tree, TreeKind};

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
    // Only method-owned terms are locals; class members are reached via `this`.
    if st.get(s.owner).kind != SymKind::Method {
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
    match &pat.kind {
        TreeKind::Bind { .. } => {
            if !pat.sym.is_none() {
                out.insert(pat.sym);
            }
        }
        TreeKind::Ident { name } => {
            let varid = name
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || c == '_');
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
        TreeKind::LabelDef { params, rhs, .. } => {
            params.iter().for_each(&mut *f);
            f(rhs);
        }
        _ => {}
    }
}
