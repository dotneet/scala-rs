//! nsc `Namers.validate` and the parser's parameter-modifier checks: the
//! modifier combinations a definition may not carry, whatever its body says.
//!
//! These are judged on the parsed tree, before the namer runs, because they
//! are about what the source *wrote*: the namer later adds `ABSTRACT` to every
//! body-less `def` and `CASE` / `MODULE` bookkeeping of its own, after which a
//! written `abstract def f` and a plain declaration look alike.
//!
//! ```scala
//! implicit case class IntOps(i: Int)      // illegal combination of modifiers: implicit and case
//! sealed def f = 0                         // `sealed` modifier can be used only for classes
//! final trait T                            // illegal combination of modifiers: abstract and final
//! class Foo(v: Int) { implicit def this(s: String) = this(1) }
//! case class C(x: => Int)                  // `val` parameters may not be call-by-name
//! def g = { def h: Int; 1 }                // only traits and abstract classes can have declared …
//! ```
//!
//! scala-rs accepted every one of these (scala/scala `neg/t6227`, `t6597`,
//! `t631`, `t1838`, `t4882`, `t6795`, `t8217-local-alias-requires-rhs`).
//! Wording and order follow nsc's `SymValidateErrors`.
//!
//! Left to other passes: `abstract override` outside a trait
//! (`check_template`), a top-level `implicit class` (`type_class`), and the
//! members an `object` or anonymous class leaves undefined, which the
//! missing-implementation check already reports.

use crate::check::Typer;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

/// Where a definition is written: what nsc's `sym.owner` is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Package,
    /// A trait, a class, an object or an anonymous class: a template.
    Template,
    /// A block or a method body.
    Local,
}

impl Typer {
    /// Report every modifier combination nsc's namer refuses, across `tree`.
    pub(crate) fn validate_modifiers(&mut self, tree: &Tree) {
        let mut out = Vec::new();
        walk(tree, Owner::Package, &mut out);
        for (span, msg) in out {
            self.error(span, msg);
        }
    }
}

type Out = Vec<(Span, String)>;

/// nsc's `Symbol.toString` for the kinds a modifier error names.
fn describe(t: &Tree) -> String {
    match &t.kind {
        TreeKind::ClassDef { mods, name, .. } if mods.flags.contains(Flags::TRAIT) => {
            format!("trait {name}")
        }
        TreeKind::ClassDef { name, .. } => format!("class {name}"),
        TreeKind::ModuleDef { name, .. } => format!("object {name}"),
        TreeKind::DefDef { name, .. } if name == "<init>" => "constructor".to_string(),
        TreeKind::DefDef { name, .. } => format!("method {name}"),
        TreeKind::ValDef { mods, name, .. } if mods.flags.contains(Flags::MUTABLE) => {
            format!("variable {name}")
        }
        TreeKind::ValDef { name, .. } => format!("value {name}"),
        TreeKind::TypeDef { name, .. } => format!("type {name}"),
        _ => String::new(),
    }
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

/// One definition, written in `owner`. `deferred` is nsc's `isDeferred`: a
/// `def` or `val` with no right-hand side, or a type with no alias.
fn validate(t: &Tree, owner: Owner, out: &mut Out) {
    let Some(mods) = mods_of(t) else {
        return;
    };
    let f = mods.flags;
    if f.contains(Flags::SYNTHETIC) || f.contains(Flags::JAVA) {
        return;
    }
    // nsc resets `DEFERRED` on an `@native` member before these checks.
    let native = f.contains(Flags::NATIVE)
        || mods
            .annotations
            .iter()
            .any(|a| crate::check::is_native_annot(&a.annotation_path()));
    let is_class = matches!(t.kind, TreeKind::ClassDef { .. });
    let is_trait = is_class && f.contains(Flags::TRAIT);
    let is_ctor = matches!(&t.kind, TreeKind::DefDef { name, .. } if name == "<init>");
    let is_type = matches!(t.kind, TreeKind::TypeDef { .. });
    let is_term = matches!(
        t.kind,
        TreeKind::ModuleDef { .. } | TreeKind::ValDef { .. } | TreeKind::DefDef { .. }
    );
    let conflict = |a: Flags, an: &str, b: Flags, bn: &str, out: &mut Out| {
        if f.contains(a) && f.contains(b) {
            out.push((
                t.span,
                format!(
                    "illegal combination of modifiers: {an} and {bn} for: {}",
                    describe(t)
                ),
            ));
        }
    };
    if f.contains(Flags::IMPLICIT) {
        if is_ctor {
            out.push((
                t.span,
                "`implicit` modifier not allowed for constructors".to_string(),
            ));
        }
        // An `implicit trait` is reported as an implicit class without the
        // one constructor parameter (`type_class`); nsc keeps only the first
        // error at a position.
        if !(is_term || is_class) {
            out.push((
                t.span,
                "`implicit` modifier can be used only for values, variables, methods and classes"
                    .to_string(),
            ));
        }
        // A top-level `implicit class` is reported by `type_class`.
        if owner == Owner::Package && !is_class {
            out.push((
                t.span,
                "`implicit` modifier cannot be used for top-level objects".to_string(),
            ));
        }
    }
    if is_class {
        conflict(Flags::IMPLICIT, "implicit", Flags::CASE, "case", out);
        if f.contains(Flags::OVERRIDE) && !is_trait {
            out.push((
                t.span,
                "`override` modifier not allowed for classes".to_string(),
            ));
        }
    } else {
        if f.contains(Flags::SEALED) {
            out.push((
                t.span,
                "`sealed` modifier can be used only for classes".to_string(),
            ));
        }
        // `abstract override` is nsc's `ABSOVERRIDE`, a different flag.
        if f.contains(Flags::ABSTRACT) && !f.contains(Flags::OVERRIDE) {
            out.push((
                t.span,
                "`abstract` modifier can be used only for classes; it should be omitted for abstract members"
                    .to_string(),
            ));
        }
    }
    if is_ctor && f.contains(Flags::OVERRIDE) {
        out.push((
            t.span,
            "`override` modifier not allowed for constructors".to_string(),
        ));
    }
    if is_type && f.contains(Flags::ABSTRACT) && f.contains(Flags::OVERRIDE) {
        out.push((
            t.span,
            "`abstract override` modifier not allowed for type members".to_string(),
        ));
    }
    // A trait is `ABSTRACT` in nsc whether or not the source says so.
    if is_trait && f.contains(Flags::FINAL) {
        out.push((
            t.span,
            format!(
                "illegal combination of modifiers: abstract and final for: {}",
                describe(t)
            ),
        ));
    }
    let deferred = match &t.kind {
        TreeKind::DefDef { rhs, .. } => !is_ctor && rhs.is_empty(),
        TreeKind::ValDef { rhs, .. } => !f.contains(Flags::PARAM) && rhs.is_empty(),
        TreeKind::TypeDef { rhs, .. } => rhs.is_empty(),
        _ => false,
    };
    if deferred && !native {
        // An abstract type member of a template is always allowed; the
        // members an object or an anonymous class leaves undefined are the
        // missing-implementation check's to report.
        if owner == Owner::Local {
            let mut msg =
                "only traits and abstract classes can have declared but undefined members"
                    .to_string();
            if matches!(t.kind, TreeKind::ValDef { .. }) && f.contains(Flags::MUTABLE) {
                msg.push_str("\n(Note that variables need to be initialized to be defined)");
            }
            out.push((t.span, msg));
        }
        if !is_type {
            // `private[p]` is an access boundary, not `PRIVATE`, to nsc.
            if f.contains(Flags::PRIVATE) && mods.private_within.is_none() {
                out.push((
                    t.span,
                    "abstract member may not have private modifier".to_string(),
                ));
            }
            if f.contains(Flags::FINAL) {
                out.push((
                    t.span,
                    "abstract member may not have final modifier".to_string(),
                ));
            }
        }
    }
    conflict(Flags::FINAL, "final", Flags::SEALED, "sealed", out);
    // `private[p] protected` is not the combination: nsc normalises a
    // qualified `private` away first (`private[this]` stays `PRIVATE`).
    if f.contains(Flags::PRIVATE) && f.contains(Flags::PROTECTED) && mods.private_within.is_none() {
        conflict(
            Flags::PRIVATE,
            "private",
            Flags::PROTECTED,
            "protected",
            out,
        );
    }
}

/// nsc's parser: a class parameter that is a field (`val`, `var`, or any
/// parameter of a case class's first list) cannot be by-name. (An implicit
/// by-name parameter is allowed: scalac 2.13.16 compiles
/// `def f(implicit x: => Int)`.)
fn validate_class_params(t: &Tree, out: &mut Out) {
    let TreeKind::ClassDef { mods, vparamss, .. } = &t.kind else {
        return;
    };
    let case = mods.flags.contains(Flags::CASE);
    for (i, clause) in vparamss.iter().enumerate() {
        for p in clause {
            let TreeKind::ValDef { mods: pm, .. } = &p.kind else {
                continue;
            };
            let pf = pm.flags;
            if !pf.contains(Flags::BYNAME) || pf.contains(Flags::SYNTHETIC) {
                continue;
            }
            let local = pf.contains(Flags::PRIVATE) && pf.contains(Flags::LOCAL);
            let field = pf.contains(Flags::ACCESSOR)
                || pf.contains(Flags::MUTABLE)
                || (case && i == 0 && !pf.contains(Flags::IMPLICIT));
            if field && !local {
                let kw = if pf.contains(Flags::MUTABLE) {
                    "var"
                } else {
                    "val"
                };
                out.push((p.span, format!("`{kw}` parameters may not be call-by-name")));
            }
        }
    }
}

/// Visit every definition in `t`, which is written in `owner`.
fn walk(t: &Tree, owner: Owner, out: &mut Out) {
    match &t.kind {
        TreeKind::PackageDef { stats, .. } => {
            for s in stats {
                walk_stat(s, Owner::Package, out);
            }
        }
        _ => walk_stat(t, owner, out),
    }
}

/// `t` is a statement of a package, a template or a block.
fn walk_stat(t: &Tree, owner: Owner, out: &mut Out) {
    match &t.kind {
        TreeKind::PackageDef { .. } => walk(t, Owner::Package, out),
        TreeKind::ClassDef {
            vparamss, impl_, ..
        } => {
            validate(t, owner, out);
            validate_class_params(t, out);
            for p in vparamss.iter().flatten() {
                walk_param(p, out);
            }
            walk_template(impl_, out);
        }
        TreeKind::ModuleDef { impl_, .. } => {
            validate(t, owner, out);
            walk_template(impl_, out);
        }
        TreeKind::DefDef { vparamss, rhs, .. } => {
            validate(t, owner, out);
            for p in vparamss.iter().flatten() {
                walk_param(p, out);
            }
            walk_expr(rhs, out);
        }
        TreeKind::ValDef { rhs, .. } => {
            validate(t, owner, out);
            walk_expr(rhs, out);
        }
        TreeKind::TypeDef { .. } => validate(t, owner, out),
        _ => walk_expr(t, out),
    }
}

fn walk_param(p: &Tree, out: &mut Out) {
    if let TreeKind::ValDef { rhs, .. } = &p.kind {
        walk_expr(rhs, out);
    }
}

fn walk_template(tp: &Template, out: &mut Out) {
    for p in &tp.parents {
        walk_expr(p, out);
    }
    for s in &tp.body {
        walk_stat(s, Owner::Template, out);
    }
}

/// An expression: only a block's statements are definitions; anything else
/// is searched for blocks and anonymous classes. Type trees are skipped -- a
/// refinement's or an existential's declarations are deferred by design.
fn walk_expr(t: &Tree, out: &mut Out) {
    match &t.kind {
        TreeKind::Block { stats, expr } => {
            for s in stats {
                walk_stat(s, Owner::Local, out);
            }
            walk_expr(expr, out);
        }
        TreeKind::ClassDef { .. }
        | TreeKind::ModuleDef { .. }
        | TreeKind::DefDef { .. }
        | TreeKind::ValDef { .. }
        | TreeKind::TypeDef { .. } => walk_stat(t, Owner::Local, out),
        TreeKind::CompoundTypeTree { .. }
        | TreeKind::ExistentialTypeTree { .. }
        | TreeKind::AppliedTypeTree { .. }
        | TreeKind::SelectFromTypeTree { .. }
        | TreeKind::SingletonTypeTree { .. }
        | TreeKind::AnnotatedTypeTree { .. } => {}
        TreeKind::Function { body, .. } => walk_expr(body, out),
        _ => {
            let mut kids = Vec::new();
            crate::macros::push_children(t, &mut kids);
            for k in kids {
                walk_expr(k, out);
            }
        }
    }
}
