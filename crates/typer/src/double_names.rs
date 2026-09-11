//! nsc `Namers.enterInScope`'s double-definition check, and the two scope
//! rules next to it: a name entered twice into one scope.
//!
//! ```scala
//! class A { val x = 1; val x = 2 }        // x is already defined as value x
//! class A(x: Int) { val x = 1 }           // the same: a class parameter is a field
//! object O { class C; trait C }           // C is already defined as class C
//! def g[T, U](x: T)(x: U) = 1             // x is already defined as value x
//! def f = { def g = 1; def g = 2; g }     // method g is defined twice
//! a match { case (x, x) => }              // x is already defined as value x
//! ```
//!
//! scala-rs entered the second symbol beside the first and compiled every one
//! of these, with whichever of the two a lookup happened to find (scala/scala
//! `neg/t693`, `neg/t5956`, `neg/t11374`, `neg/t800`, `neg/t3649`, and
//! `neg/double-def-top-level` across files).
//!
//! Like `crate::modifier_rules` this reads the parsed trees, before the namer:
//! which definitions share a scope is a fact about the source, and the namer
//! adds synthetic members of its own that must not be mistaken for the
//! programmer's. The rules are nsc's:
//!
//! * terms and types are separate namespaces;
//! * a `def` in a template may be overloaded, so it is never itself a
//!   conflict; any other definition conflicts with an earlier one of the same
//!   name unless that earlier one is a method (the erasure-level check,
//!   `crate::double_def`, judges those pairs) -- except that a parameterless
//!   `def` and a `val` of the same name always clash (`value x is defined
//!   twice`);
//! * a `case class`, and a class whose constructor has default arguments,
//!   get a compiler-generated companion `object` unless the scope writes one;
//! * top-level definitions of one package clash across the files of a run,
//!   and a class and its companion must be written in the same file.

use crate::check::Typer;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Ns {
    Term,
    Type,
}

#[derive(Clone)]
struct Entry {
    name: String,
    ns: Ns,
    /// nsc's `Symbol.toString` of the definition: `value x`, `case class C`.
    desc: String,
    span: Span,
    file: usize,
    /// A `def` (`isSourceMethod`).
    method: bool,
    /// A `def` without parameter lists.
    parameterless: bool,
    /// The companion nsc generates.
    synthetic: bool,
    /// A `class`/`trait`/`object` written at the top level of a package.
    toplevel_template: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Template,
    Block,
    Package,
}

type Out = Vec<(usize, Span, String)>;

/// What the walk found: the scopes to check, and the errors it could decide
/// on its own (pattern binders).
#[derive(Default)]
struct Found {
    scopes: Vec<(Vec<Entry>, ScopeKind)>,
    out: Out,
}

impl Typer {
    /// Report every name entered twice into one scope, across `units`.
    pub(crate) fn check_duplicate_names(&mut self, units: &[(&Tree, usize)]) {
        let mut found = Found::default();
        let mut packages: HashMap<String, Vec<Entry>> = HashMap::new();
        for (tree, file) in units {
            walk_package(tree, "", *file, &mut packages, &mut found);
        }
        let mut keys: Vec<String> = packages.keys().cloned().collect();
        keys.sort();
        let mut out = std::mem::take(&mut found.out);
        for k in keys {
            let entries = packages.remove(&k).unwrap_or_default();
            self.check_companion_files(&entries, &mut out);
            found.scopes.push((entries, ScopeKind::Package));
        }
        for (entries, kind) in &found.scopes {
            self.check_scope(entries, *kind, &mut out);
        }
        for (file, span, msg) in out {
            let saved = self.file_index;
            self.file_index = file;
            self.error(span, msg);
            self.file_index = saved;
        }
    }

    /// nsc: a class and its companion object are compiled together, so they
    /// must come from one file. Reported at whichever of the two the later
    /// file writes, naming the class's file first.
    fn check_companion_files(&self, entries: &[Entry], out: &mut Out) {
        for c in entries
            .iter()
            .filter(|e| e.ns == Ns::Type && e.toplevel_template)
        {
            for m in entries.iter().filter(|e| {
                e.ns == Ns::Term && e.toplevel_template && e.name == c.name && e.file != c.file
            }) {
                let path = |f: usize| {
                    self.source_paths
                        .get(f)
                        .cloned()
                        .unwrap_or_else(|| format!("<file {f}>"))
                };
                let at = if m.file > c.file { m } else { c };
                out.push((
                    at.file,
                    at.span,
                    format!(
                        "Companions '{}' and '{}' must be defined in same file:\n  Found in {} and {}",
                        c.desc,
                        m.desc,
                        path(c.file),
                        path(m.file)
                    ),
                ));
            }
        }
    }

    /// `line:column` of `name` inside `span`, as nsc prints a position.
    fn name_position(&self, file: usize, span: Span, name: &str) -> Option<String> {
        let src = self.sources.get(file)?;
        let lo = span.lo.0 as usize;
        let hi = (span.hi.0 as usize).min(src.len());
        if lo >= hi || !src.is_char_boundary(lo) || !src.is_char_boundary(hi) {
            return None;
        }
        let text = &src[lo..hi];
        let bytes = text.as_bytes();
        let mut at = None;
        let mut search = 0;
        while let Some(i) = text[search..].find(name) {
            let start = search + i;
            let end = start + name.len();
            let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
            let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
            if before_ok && after_ok {
                at = Some(lo + start);
                break;
            }
            search = start + name.len().max(1);
        }
        let at = at?;
        let line = src[..at].matches('\n').count() + 1;
        let col = at - src[..at].rfind('\n').map(|i| i + 1).unwrap_or(0) + 1;
        Some(format!("{line}:{col}"))
    }

    fn check_scope(&self, entries: &[Entry], kind: ScopeKind, out: &mut Out) {
        let mut seen: HashMap<(Ns, &str), &Entry> = HashMap::new();
        for e in entries {
            let key = (e.ns, e.name.as_str());
            let Some(prev) = seen.get(&key).copied() else {
                seen.insert(key, e);
                continue;
            };
            let at = |p: &Entry| {
                self.name_position(p.file, p.span, &p.name)
                    .map(|pos| format!(" was defined at line {pos}"))
                    .unwrap_or_else(|| " was defined earlier in the same scope".to_string())
            };
            if kind == ScopeKind::Package && prev.file != e.file {
                if prev.toplevel_template && e.toplevel_template {
                    out.push((
                        e.file,
                        e.span,
                        format!("{} is already defined as {}", e.name, prev.desc),
                    ));
                }
                seen.insert(key, e);
                continue;
            }
            if e.method && prev.method {
                // Overloads in a template; a local `def` may not be.
                if kind == ScopeKind::Block {
                    out.push((
                        e.file,
                        e.span,
                        format!(
                            "{} is defined twice;\n  the conflicting {}{}",
                            e.desc,
                            prev.desc,
                            at(prev)
                        ),
                    ));
                }
                continue;
            }
            if e.method || prev.method {
                // A value and a method of one name: a clash only when the
                // method has no parameter list, i.e. the same signature as
                // the value's getter (nsc `checkNoDoubleDefs`).
                let (m, v) = if e.method { (e, prev) } else { (prev, e) };
                if kind == ScopeKind::Template && m.parameterless && v.ns == Ns::Term {
                    out.push((
                        e.file,
                        e.span,
                        format!(
                            "{} is defined twice;\n  the conflicting {}{}",
                            e.desc,
                            prev.desc,
                            at(prev)
                        ),
                    ));
                }
                if !e.method {
                    seen.insert(key, e);
                }
                continue;
            }
            if e.synthetic {
                // The generated companion meets a value written before the
                // class: nsc reports the object.
                out.push((
                    e.file,
                    e.span,
                    format!(
                        "object {} is defined twice;\n  the conflicting {}{}",
                        e.name,
                        prev.desc,
                        at(prev)
                    ),
                ));
                continue;
            }
            let shown = if prev.synthetic {
                format!("(compiler-generated) case class companion {}", prev.desc)
            } else {
                prev.desc.clone()
            };
            out.push((
                e.file,
                e.span,
                format!("{} is already defined as {shown}", e.name),
            ));
            seen.insert(key, e);
        }
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

fn mods_of(t: &Tree) -> Option<&Modifiers> {
    match &t.kind {
        TreeKind::ClassDef { mods, .. }
        | TreeKind::ModuleDef { mods, .. }
        | TreeKind::ValDef { mods, .. }
        | TreeKind::DefDef { mods, .. }
        | TreeKind::TypeDef { mods, .. } => Some(mods),
        _ => None,
    }
}

fn value_desc(mods: &Modifiers, name: &str) -> String {
    if mods.flags.contains(Flags::LAZY) {
        format!("lazy value {name}")
    } else if mods.flags.contains(Flags::MUTABLE) {
        format!("variable {name}")
    } else {
        format!("value {name}")
    }
}

/// Whether a class gets a compiler-generated companion: a case class, or a
/// class with a default argument in its constructor.
fn wants_companion(t: &Tree) -> bool {
    let TreeKind::ClassDef { mods, vparamss, .. } = &t.kind else {
        return false;
    };
    if mods.flags.contains(Flags::TRAIT) {
        return false;
    }
    mods.flags.contains(Flags::CASE)
        || vparamss.iter().flatten().any(|p| {
            matches!(&p.kind, TreeKind::ValDef { mods, .. } if mods.flags.contains(Flags::DEFAULTPARAM))
        })
}

/// The entries `stats` put into their scope, in order.
fn scope_entries(stats: &[Tree], file: usize, toplevel: bool) -> Vec<Entry> {
    let written_modules: Vec<&str> = stats
        .iter()
        .filter_map(|s| match &s.kind {
            TreeKind::ModuleDef { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for s in stats {
        let Some(mods) = mods_of(s) else { continue };
        if mods.flags.contains(Flags::SYNTHETIC) {
            continue;
        }
        let base = |name: &str, ns: Ns, desc: String| Entry {
            name: name.to_string(),
            ns,
            desc,
            span: s.span,
            file,
            method: false,
            parameterless: false,
            synthetic: false,
            toplevel_template: false,
        };
        match &s.kind {
            TreeKind::ValDef { name, .. } if name != "_" => {
                out.push(base(name, Ns::Term, value_desc(mods, name)));
            }
            TreeKind::DefDef { name, vparamss, .. } if name != "<init>" => {
                let mut e = base(name, Ns::Term, format!("method {name}"));
                e.method = true;
                e.parameterless = vparamss.is_empty();
                out.push(e);
            }
            TreeKind::ModuleDef { name, .. } if name != "package" => {
                let mut e = base(name, Ns::Term, format!("object {name}"));
                e.toplevel_template = toplevel;
                out.push(e);
            }
            TreeKind::ClassDef { name, .. } => {
                let desc = if mods.flags.contains(Flags::TRAIT) {
                    format!("trait {name}")
                } else if mods.flags.contains(Flags::CASE) {
                    format!("case class {name}")
                } else {
                    format!("class {name}")
                };
                let mut e = base(name, Ns::Type, desc);
                e.toplevel_template = toplevel;
                out.push(e);
                if !toplevel && wants_companion(s) && !written_modules.contains(&name.as_str()) {
                    let mut c = base(name, Ns::Term, format!("object {name}"));
                    c.synthetic = true;
                    out.push(c);
                }
            }
            TreeKind::TypeDef { name, .. } if name != "_" => {
                out.push(base(name, Ns::Type, format!("type {name}")));
            }
            _ => {}
        }
    }
    out
}

fn param_entries(params: &[&Tree], ns: Ns, file: usize) -> Vec<Entry> {
    params
        .iter()
        .filter_map(|p| {
            let (TreeKind::ValDef { name, .. } | TreeKind::TypeDef { name, .. }) = &p.kind else {
                return None;
            };
            if name == "_" || name.is_empty() {
                return None;
            }
            if mods_of(p).is_some_and(|m| m.flags.contains(Flags::SYNTHETIC)) {
                return None;
            }
            let desc = match ns {
                Ns::Term => format!("value {name}"),
                Ns::Type => format!("type {name}"),
            };
            Some(Entry {
                name: name.clone(),
                ns,
                desc,
                span: p.span,
                file,
                method: false,
                parameterless: false,
                synthetic: false,
                toplevel_template: false,
            })
        })
        .collect()
}

/// The dotted name of a package clause's `pid`.
fn pid_path(pid: &Tree) -> String {
    match &pid.kind {
        TreeKind::Ident { name } => name.clone(),
        TreeKind::Select { qual, name } => format!("{}.{name}", pid_path(qual)),
        _ => String::new(),
    }
}

fn walk_package(
    t: &Tree,
    prefix: &str,
    file: usize,
    packages: &mut HashMap<String, Vec<Entry>>,
    out: &mut Found,
) {
    let TreeKind::PackageDef { pid, stats } = &t.kind else {
        walk(t, file, out);
        return;
    };
    let own = pid_path(pid);
    let path = if own.is_empty() || own == "<empty>" {
        prefix.to_string()
    } else if prefix.is_empty() {
        own
    } else {
        format!("{prefix}.{own}")
    };
    let entries = scope_entries(stats, file, true);
    packages.entry(path.clone()).or_default().extend(entries);
    for s in stats {
        if matches!(s.kind, TreeKind::PackageDef { .. }) {
            walk_package(s, &path, file, packages, out);
        } else {
            walk(s, file, out);
        }
    }
}

/// Check the scopes a definition opens, then walk into it.
fn walk(t: &Tree, file: usize, out: &mut Found) {
    let scope = |ts: Vec<Entry>, kind: ScopeKind, out: &mut Found| {
        if ts.len() > 1 {
            out.scopes.push((ts, kind));
        }
    };
    match &t.kind {
        TreeKind::ClassDef {
            tparams,
            vparamss,
            impl_,
            ..
        } => {
            let tps: Vec<&Tree> = tparams.iter().collect();
            scope(
                param_entries(&tps, Ns::Type, file),
                ScopeKind::Template,
                out,
            );
            let mut entries: Vec<Entry> = {
                let ps: Vec<&Tree> = vparamss.iter().flatten().collect();
                param_entries(&ps, Ns::Term, file)
            };
            entries.extend(scope_entries(&impl_.body, file, false));
            scope(entries, ScopeKind::Template, out);
            for p in vparamss.iter().flatten() {
                walk_children(p, file, out);
            }
            for s in impl_.parents.iter().chain(impl_.body.iter()) {
                walk(s, file, out);
            }
        }
        TreeKind::ModuleDef { impl_, .. } => {
            scope(
                scope_entries(&impl_.body, file, false),
                ScopeKind::Template,
                out,
            );
            for s in impl_.parents.iter().chain(impl_.body.iter()) {
                walk(s, file, out);
            }
        }
        TreeKind::DefDef {
            tparams,
            vparamss,
            rhs,
            ..
        } => {
            let tps: Vec<&Tree> = tparams.iter().collect();
            scope(
                param_entries(&tps, Ns::Type, file),
                ScopeKind::Template,
                out,
            );
            let ps: Vec<&Tree> = vparamss.iter().flatten().collect();
            scope(param_entries(&ps, Ns::Term, file), ScopeKind::Template, out);
            for p in &ps {
                walk_children(p, file, out);
            }
            walk(rhs, file, out);
        }
        TreeKind::TypeDef { tparams, .. } => {
            let tps: Vec<&Tree> = tparams.iter().collect();
            scope(
                param_entries(&tps, Ns::Type, file),
                ScopeKind::Template,
                out,
            );
        }
        TreeKind::Block { stats, expr } => {
            scope(scope_entries(stats, file, false), ScopeKind::Block, out);
            for s in stats {
                walk(s, file, out);
            }
            walk(expr, file, out);
        }
        TreeKind::Match { selector, cases } => {
            walk(selector, file, out);
            for c in cases {
                check_pattern_binders(&c.pat, file, out);
                walk(&c.guard, file, out);
                walk(&c.body, file, out);
            }
        }
        TreeKind::Try {
            block,
            catches,
            finalizer,
        } => {
            walk(block, file, out);
            for c in catches {
                check_pattern_binders(&c.pat, file, out);
                walk(&c.guard, file, out);
                walk(&c.body, file, out);
            }
            walk(finalizer, file, out);
        }
        TreeKind::CompoundTypeTree { .. } | TreeKind::ExistentialTypeTree { .. } => {}
        _ => walk_children(t, file, out),
    }
}

fn walk_children(t: &Tree, file: usize, out: &mut Found) {
    let mut kids = Vec::new();
    crate::macros::push_children(t, &mut kids);
    for k in kids {
        walk(k, file, out);
    }
}

/// `case (x, x) =>`: a pattern binds each name once.
fn check_pattern_binders(pat: &Tree, file: usize, out: &mut Found) {
    /// The variables `t` binds: `x @ p`, and a plain lower-case name in an
    /// argument position (SLS 8.1.1). An extractor's own name and a prefix
    /// are not binders, nor is a backquoted name.
    fn collect<'a>(t: &'a Tree, acc: &mut Vec<(&'a str, Span)>) {
        match &t.kind {
            TreeKind::Bind { name, body } => {
                if name != "_" {
                    acc.push((name, t.span));
                }
                collect(body, acc);
            }
            TreeKind::Ident { name } => {
                let var = name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_lowercase() || (c == '_' && name.len() > 1));
                if var && !t.stable_pat {
                    acc.push((name, t.span));
                }
            }
            TreeKind::Apply { args, .. } | TreeKind::UnApply { args, .. } => {
                for a in args {
                    collect(a, acc);
                }
            }
            TreeKind::Typed { expr, .. } => collect(expr, acc),
            TreeKind::Star { elem } => collect(elem, acc),
            // An alternative binds nothing.
            _ => {}
        }
    }
    let mut binds = Vec::new();
    collect(pat, &mut binds);
    let mut seen: Vec<&str> = Vec::new();
    for (name, span) in binds {
        if seen.contains(&name) {
            out.out.push((
                file,
                span,
                format!("{name} is already defined as value {name}"),
            ));
        } else {
            seen.push(name);
        }
    }
}
