//! Quasiquotes: `q"..."`, `tq"..."`, `pq"..."`, `cq"..."`.
//!
//! In nsc these are **compiler-internal (fast track) macros**, not library
//! code: `scala-reflect.jar` holds no implementation for them, so the JVM
//! bridge that runs an ordinary def macro cannot run these. scala-rs has to
//! desugar them itself. See `docs/macros.md` §6.2.
//!
//! What this module does today is the **front end** of that desugaring: it
//! recognises a quasiquote, rebuilds the Scala source hidden inside the
//! interpolation (each `$x` / `${...}` / `..$xs` / `...$xss` replaced by a
//! placeholder name), and parses it. That is enough to tell two very different
//! failures apart, and to say which is which:
//!
//! * the body uses syntax scala-rs cannot parse, reported as
//!   `unimplemented syntax: quasiquote ...` at the quasiquote's own span;
//! * the body is fine, and what is missing is the reification step — building
//!   the `internal.reificationSupport.Syntactic*` calls and running the macro.
//!
//! Neither is silently accepted. Before this, every quasiquote came out as
//! `value q is not a member of StringContext`, which is simply wrong: `q` is a
//! member of `Quasiquotes.Quasiquote`, and the real gap is expansion.

use scala_rs_parser::{Tree, TreeKind};

/// Which of the four quasiquote interpolators a prefix names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum QuasiKind {
    /// `q"..."` — a term.
    Term,
    /// `tq"..."` — a type.
    Type,
    /// `pq"..."` — a pattern.
    Pattern,
    /// `cq"..."` — one `case` clause.
    Case,
}

impl QuasiKind {
    pub(crate) fn of(prefix: &str) -> Option<QuasiKind> {
        match prefix {
            "q" => Some(QuasiKind::Term),
            "tq" => Some(QuasiKind::Type),
            "pq" => Some(QuasiKind::Pattern),
            "cq" => Some(QuasiKind::Case),
            _ => None,
        }
    }

    pub(crate) fn prefix(self) -> &'static str {
        match self {
            QuasiKind::Term => "q",
            QuasiKind::Type => "tq",
            QuasiKind::Pattern => "pq",
            QuasiKind::Case => "cq",
        }
    }
}

/// The source text of the quasiquote, with holes replaced by placeholder names.
///
/// `parts` are the literal pieces and `nargs` the number of holes between them,
/// exactly as the parser produced them for the interpolation.
///
/// A hole's *rank* -- `$x` one tree, `..$xs` a list, `...$xss` a list of lists
/// -- is written as the dots at the end of the preceding part. One placeholder
/// name stands in for any of them: every position a `..$` may appear in (an
/// argument list, a statement list, a parameter list, a pattern list) also
/// accepts a single element, which is all this needs to parse the body. The
/// rank does matter to reification, and is where this will grow.
fn splice_placeholders(parts: &[String], nargs: usize) -> String {
    let mut out = String::new();
    for i in 0..nargs {
        let raw = parts.get(i).map(String::as_str).unwrap_or("");
        out.push_str(strip_rank(raw));
        out.push_str(&hole_name(i));
    }
    if let Some(last) = parts.get(nargs) {
        out.push_str(last);
    }
    out
}

/// Drop the `..` / `...` that give a hole its rank off the end of a part.
fn strip_rank(part: &str) -> &str {
    part.strip_suffix("...")
        .or_else(|| part.strip_suffix(".."))
        .unwrap_or(part)
}

/// Wrap the body in the smallest program that puts it in the right position.
fn wrap(kind: QuasiKind, body: &str) -> String {
    match kind {
        QuasiKind::Term => format!("object qqProbe {{ def qqBody = {{\n{body}\n}} }}\n"),
        QuasiKind::Type => format!("object qqProbe {{ type qqBody = {body} }}\n"),
        QuasiKind::Pattern => format!(
            "object qqProbe {{ def qqBody(qqScrut: Any) = qqScrut match {{ case {body} => () }} }}\n"
        ),
        QuasiKind::Case => {
            format!("object qqProbe {{ def qqBody(qqScrut: Any) = qqScrut match {{\n{body}\n}} }}\n")
        }
    }
}

/// Check the body of a quasiquote.
///
/// `Ok(())` means it parsed; `Err(reason)` names the syntax that did not, for
/// an `unimplemented syntax: quasiquote ...` diagnostic. An empty body is an
/// error in nsc too (`q""` has nothing to build).
pub(crate) fn check_body(kind: QuasiKind, parts: &[String], nargs: usize) -> Result<(), String> {
    parse_program(kind, parts, nargs).map(|_| ())
}

/// The placeholder name standing in for hole `i`.
pub(crate) fn hole_name(i: usize) -> String {
    format!("qqHole{i}")
}

/// The hole a placeholder name stands for, if it is one.
pub(crate) fn hole_index(name: &str) -> Option<usize> {
    name.strip_prefix("qqHole")?.parse().ok()
}

/// The rank of each hole: `$x` is 0, `..$xs` is 1, `...$xss` is 2.
///
/// The dots are written at the *end of the part before* the hole, which is
/// where the interpolation splits them off.
pub(crate) fn hole_ranks(parts: &[String], nargs: usize) -> Vec<u8> {
    (0..nargs)
        .map(|i| {
            let raw = parts.get(i).map(String::as_str).unwrap_or("");
            if raw.ends_with("...") {
                2
            } else if raw.ends_with("..") {
                1
            } else {
                0
            }
        })
        .collect()
}

/// Parse the body of a quasiquote, with each hole standing as `qqHole<i>`.
///
/// `Err(reason)` names the syntax scala-rs could not parse, for an
/// `unimplemented syntax: quasiquote ...` diagnostic. An empty body is an
/// error in nsc too (`q""` has nothing to build).
pub(crate) fn parse_body(kind: QuasiKind, parts: &[String], nargs: usize) -> Result<Tree, String> {
    let program = parse_program(kind, parts, nargs)?;
    unwrap_body(kind, &program).ok_or_else(|| {
        format!(
            "the body of {}\"...\" is not one reification takes apart yet",
            kind.prefix()
        )
    })
}

/// Parse the wrapper program around the body. Shared by the checker, which
/// only wants to know whether it parsed, and by `parse_body`.
fn parse_program(kind: QuasiKind, parts: &[String], nargs: usize) -> Result<Tree, String> {
    let body = splice_placeholders(parts, nargs);
    if body.trim().is_empty() {
        return Err("empty quasiquote".to_string());
    }
    let src = wrap(kind, &body);
    let res = scala_rs_parser::parse_str(&src);
    if let Some(d) = res
        .diags
        .iter()
        .find(|d| d.level == scala_rs_span::Level::Error)
    {
        return Err(d.message.clone());
    }
    if let Some(what) = first_unimplemented(&res.tree) {
        return Err(what);
    }
    Ok(res.tree)
}

/// Dig the body back out of the program `wrap` built around it.
fn unwrap_body(kind: QuasiKind, tree: &Tree) -> Option<Tree> {
    let stats = match &tree.kind {
        TreeKind::PackageDef { stats, .. } => stats,
        _ => return None,
    };
    let probe = stats.iter().find_map(|s| match &s.kind {
        TreeKind::ModuleDef { name, impl_, .. } if name == "qqProbe" => Some(impl_),
        _ => None,
    })?;
    let member = probe.body.iter().find(|s| {
        matches!(&s.kind, TreeKind::DefDef { name, .. } | TreeKind::TypeDef { name, .. }
            if name == "qqBody")
    })?;
    match (kind, &member.kind) {
        // `def qqBody = { <body> }`: the wrapper's own braces come back as a
        // block, which is only a block of the quasiquote when the body really
        // had several statements.
        (QuasiKind::Term, TreeKind::DefDef { rhs, .. }) => Some(match &rhs.kind {
            TreeKind::Block { stats, expr } if stats.is_empty() => (**expr).clone(),
            _ => (**rhs).clone(),
        }),
        (QuasiKind::Type, TreeKind::TypeDef { rhs, .. }) => Some((**rhs).clone()),
        _ => None,
    }
}

/// The first `unimplemented syntax: ...` placeholder the parser left behind.
///
/// The parser reports some constructs by planting an `Unimplemented` node
/// rather than a diagnostic, so a body that "parsed" can still contain syntax
/// scala-rs does not handle. Reporting it here is what keeps a quasiquote from
/// being quietly accepted.
fn first_unimplemented(t: &Tree) -> Option<String> {
    if let TreeKind::Unimplemented { what } = &t.kind {
        return Some(what.clone());
    }
    let mut kids: Vec<&Tree> = Vec::new();
    crate::macros::push_children(t, &mut kids);
    kids.into_iter().find_map(first_unimplemented)
}
