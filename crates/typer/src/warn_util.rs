//! Shared helpers for the warning passes (`warn_*`): nsc's *point* of a tree,
//! the one position a diagnostic is reported at, and warning construction.
//!
//! Our trees carry a `[lo, hi)` span; nsc's positions also carry a *point*,
//! which is what `file:line:` and the caret show. For most trees the two
//! starts agree. The ones that differ are reconstructed from the source text:
//!
//! * `Select(qual, name)` points at `name` (`a.foo` at `foo`, and an infix
//!   `a + b` at `+`);
//! * `Apply(fun, args)` written `f(x)` points at the opening parenthesis
//!   (`atPos(start, in.offset)` in `simpleExprRest`), while an infix
//!   application points at its operator.

use scala_rs_parser::ast::*;
use scala_rs_span::{Diagnostic, Phase, Span, WarnCategory};

/// The byte offset of nsc's point for `t`.
pub(crate) fn point_of(t: &Tree, src: &str) -> u32 {
    match &t.kind {
        TreeKind::Select { name, qual } => select_name_start(t, qual, name, src),
        TreeKind::Apply { fun, args } => {
            if let TreeKind::Select { qual, name } = &fun.kind {
                if is_infix_select(fun, qual, name, src) {
                    return select_name_start(fun, qual, name, src);
                }
            }
            let _ = args;
            // `f(x)` / `f[T](x)` / `f { x }`: the argument list's opener.
            let from = fun.span.hi.0 as usize;
            if let Some(off) = next_opener(src, from, t.span.hi.0 as usize) {
                return off as u32;
            }
            point_of(fun, src)
        }
        TreeKind::TypeApply { fun, .. } => point_of(fun, src),
        TreeKind::Typed { expr, .. } => {
            // `(e: T)`: nsc's `Typed` points at the colon.
            let from = expr.span.hi.0 as usize;
            let bytes = src.as_bytes();
            let mut i = from;
            while i < bytes.len() && i < t.span.hi.0 as usize && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b':' {
                i as u32
            } else {
                t.span.lo.0
            }
        }
        _ => t.span.lo.0,
    }
}

/// Whether `fun = Select(qual, name)` was written without a dot (`a op b`).
fn is_infix_select(fun: &Tree, qual: &Tree, name: &str, src: &str) -> bool {
    let name_start = select_name_start(fun, qual, name, src) as usize;
    let qual_end = qual.span.hi.0 as usize;
    if qual.span.is_dummy() || qual_end > name_start || name_start > src.len() {
        return false;
    }
    // A right-associative operator puts the receiver after the name.
    if qual.span.lo.0 as usize >= name_start {
        return true;
    }
    let between = &src[qual_end..name_start];
    !between.contains('.')
}

fn select_name_start(t: &Tree, qual: &Tree, name: &str, src: &str) -> u32 {
    let hi = t.span.hi.0 as usize;
    let bytes = src.as_bytes();
    let n = display_name(name);
    // A right-associative operator (`x :: xs` is `xs.::(x)`) or a prefix one
    // (`-x`): the Select's span starts at the operator, before the receiver.
    if !qual.span.is_dummy() && qual.span.lo.0 > t.span.lo.0 {
        let lo = t.span.lo.0 as usize;
        if src.get(lo..lo + n.len()) == Some(n.as_str()) {
            return lo as u32;
        }
    }
    if hi == 0 || hi > bytes.len() {
        return t.span.lo.0;
    }
    if bytes[hi - 1] == b'`' {
        // `a.`type``
        if let Some(open) = src[..hi - 1].rfind('`') {
            return open as u32;
        }
    }
    if hi >= n.len() && src.get(hi - n.len()..hi) == Some(n.as_str()) {
        return (hi - n.len()) as u32;
    }
    t.span.lo.0
}

/// The source spelling of a member name as our trees store it.
fn display_name(name: &str) -> String {
    name.strip_prefix("unary_").unwrap_or(name).to_string()
}

/// The first `(` or `{` at or after `from` (whitespace skipped), before `limit`.
fn next_opener(src: &str, from: usize, limit: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut i = from;
    while i < bytes.len() && i < limit {
        match bytes[i] {
            b'(' | b'{' => return Some(i),
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            _ => return None,
        }
    }
    None
}

/// A warning at nsc's point for `t`.
pub(crate) fn warning_at(
    file: usize,
    point: u32,
    end: u32,
    msg: impl Into<String>,
    phase: Phase,
    fatal: bool,
) -> Diagnostic {
    let span = Span::new(point, end.max(point + 1));
    let d = if fatal {
        Diagnostic::error(file, span, msg)
    } else {
        Diagnostic::warning(file, span, msg)
    };
    d.in_phase(phase).with_category(WarnCategory::Other)
}

/// Every child tree (expressions and definitions), in source order.
pub(crate) fn each_child(t: &Tree, f: &mut dyn FnMut(&Tree)) {
    match &t.kind {
        TreeKind::PackageDef { stats, .. } => stats.iter().for_each(f),
        TreeKind::ClassDef { impl_, .. } | TreeKind::ModuleDef { impl_, .. } => {
            impl_.parents.iter().for_each(&mut *f);
            impl_.body.iter().for_each(f)
        }
        TreeKind::ValDef { rhs, .. } => f(rhs),
        TreeKind::DefDef { rhs, .. } => f(rhs),
        TreeKind::LabelDef { rhs, .. } => f(rhs),
        TreeKind::Block { stats, expr } => {
            stats.iter().for_each(&mut *f);
            f(expr)
        }
        TreeKind::If { cond, thenp, elsep } => {
            f(cond);
            f(thenp);
            f(elsep)
        }
        TreeKind::Match { selector, cases } => {
            f(selector);
            for c in cases {
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
                f(&c.guard);
                f(&c.body);
            }
            f(finalizer)
        }
        TreeKind::Function { body, .. } => f(body),
        TreeKind::Assign { lhs, rhs } => {
            f(lhs);
            f(rhs)
        }
        TreeKind::While { cond, body } | TreeKind::DoWhile { body, cond } => {
            f(cond);
            f(body)
        }
        TreeKind::Return { expr } | TreeKind::Throw { expr } => f(expr),
        TreeKind::New { tpt } => f(tpt),
        TreeKind::Typed { expr, .. } => f(expr),
        TreeKind::TypeApply { fun, .. } => f(fun),
        TreeKind::Apply { fun, args } => {
            f(fun);
            args.iter().for_each(f)
        }
        TreeKind::Select { qual, .. } => f(qual),
        TreeKind::InterpolatedString { args, .. } => args.iter().for_each(f),
        _ => {}
    }
}
