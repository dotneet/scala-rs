//! nsc `SwitchEmission` (`MatchOptimization.scala`, scala/bug#7290): while
//! turning a match on an `Int`-like or `String` scrutinee into a switch, an
//! alternative that names the same constant twice (`case 0 | 0 =>`,
//! `case 4 | (_ @ 4) =>`) is reported as "Pattern contains duplicate
//! alternatives: 0" -- a warning, so an error under `-Xfatal-warnings`.
//!
//! The cases are walked in order the way nsc's `traverseOpt` does: the walk
//! stops at the first case that cannot be a switch case (a binder, an
//! extractor, a type test), and the alternatives before it have already been
//! reported by then.

use crate::check::*;
use scala_rs_parser::ast::*;

/// A constant a switch case can test for, printed as nsc's `LIT` tree.
fn switch_constant(pat: &Tree) -> Option<String> {
    let lit = match &pat.kind {
        TreeKind::Literal { lit } => lit.clone(),
        _ => match &pat.ty {
            Type::Constant(lit) => lit.clone(),
            _ => return None,
        },
    };
    match lit {
        Lit::Int(n) => Some(n.to_string()),
        // `isIntRange`: a `Char` constant becomes its code as an `Int`.
        Lit::Char(c) => Some((c as u32).to_string()),
        Lit::String(s) => Some(format!("{s:?}")),
        Lit::Null => Some("null".into()),
        _ => None,
    }
}

impl Typer {
    pub(crate) fn warn_duplicate_alternatives(&mut self, sel_ty: &Type, cases: &[CaseDef]) {
        let switchable = matches!(
            crate::check::peel_type_annot(&sel_ty.widen_constant()),
            Type::Int | Type::Char | Type::Byte | Type::Short | Type::String
        );
        if !switchable {
            return;
        }
        let mut warnings = Vec::new();
        for c in cases {
            match &c.pat.kind {
                // The default case (a guard is fine: it moves into the body).
                TreeKind::Wildcard => {}
                TreeKind::Alternative { trees } => {
                    let Some(consts) = trees
                        .iter()
                        .map(switch_constant)
                        .collect::<Option<Vec<_>>>()
                    else {
                        break;
                    };
                    let mut seen: Vec<&String> = Vec::new();
                    let mut dups: Vec<&String> = Vec::new();
                    for k in &consts {
                        if seen.contains(&k) {
                            if !dups.contains(&k) {
                                dups.push(k);
                            }
                        } else {
                            seen.push(k);
                        }
                    }
                    if !dups.is_empty() {
                        let list = dups
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        warnings.push((
                            c.pat.span,
                            format!("Pattern contains duplicate alternatives: {list}"),
                        ));
                    }
                }
                _ if switch_constant(&c.pat).is_some() => {}
                _ => break,
            }
        }
        for (span, msg) in warnings {
            self.warning(span, msg);
        }
    }
}
