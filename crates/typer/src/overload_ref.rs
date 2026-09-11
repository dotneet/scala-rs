//! An overloaded method reference in value position, with no function type
//! expected: nsc's `inferExprAlternative`, followed by `adapt`.
//!
//! ```scala
//! def over(a: Int): Int = a
//! def over(a: String): Int = 1
//! val o = over          // ambiguous reference to overloaded definition,
//!                       // both method over ... and method over ...
//! def f(a: Any) = 1; def f(a: String) = 2
//! val g = f             // missing argument list for method f (the String one)
//! ```
//!
//! nsc keeps the alternatives whose type, read as a function, is compatible
//! with the expected type, and takes the most specific one. When there is
//! one, the reference is that method, which in value position is the
//! "missing argument list" of `check_method_value` (or its eta-expansion
//! under `-Xsource:3`); when there is none, the reference is ambiguous.
//! scala-rs left the `Overload` type standing and compiled `val o = over`.
//!
//! Only groups whose every alternative takes an explicit first parameter
//! clause are this rule's: a nullary alternative is auto-applied, an
//! implicit-only one is what value position keeps, and a value alternative
//! is the value (`maybe_auto_apply`, `implicit_only_alternative`,
//! `function_value_alternative` decide those first).

use crate::check::Typer;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;

impl Typer {
    /// The alternatives `tree`'s overloaded type stands for, with their types
    /// as seen from the receiver.
    fn overloaded_ref_alternatives(&self, tree: &Tree) -> Option<Vec<(SymbolId, Type)>> {
        let Type::Overload(tys) = &tree.ty else {
            return None;
        };
        if tree.sym.is_none()
            || !matches!(tree.kind, TreeKind::Ident { .. } | TreeKind::Select { .. })
        {
            return None;
        }
        if let Some(g) = self.overload_member_types.get(&tree.sym.0) {
            if g.len() == tys.len() && g.iter().zip(tys).all(|((_, a), b)| a == b) {
                return Some(g.clone());
            }
        }
        let name = self.st.get(tree.sym).name.clone();
        let syms = self.drop_overridden(self.overload_alternatives(tree.sym, &name));
        if syms.len() != tys.len() {
            return None;
        }
        Some(
            syms.into_iter()
                .map(|s| (s, self.st.get(s).ty.clone()))
                .collect(),
        )
    }

    /// nsc's `isStrictlyMoreSpecific` between two method alternatives: the
    /// relative weight of "as specific as" plus "owned by a proper subclass".
    fn alt_strictly_more_specific(&self, a: (SymbolId, &[Type]), b: (SymbolId, &[Type])) -> bool {
        let weight = |x: (SymbolId, &[Type]), y: (SymbolId, &[Type])| -> u8 {
            u8::from(self.is_as_specific_method(x.0, y.0, x.1, y.1, false))
                + u8::from(self.owner_is_proper_subclass(x.0, y.0))
        };
        weight(a, b) > weight(b, a)
    }

    /// nsc's `MethodType.toString`: `[T](a: T)(implicit b: Int): R`.
    pub(crate) fn nsc_method_type_string(&self, sym: SymbolId, ty: &Type) -> String {
        let s = self.st.get(sym);
        let mut out = String::new();
        if !s.tparams.is_empty() {
            let names: Vec<String> = s
                .tparams
                .iter()
                .map(|t| self.st.get(*t).name.clone())
                .collect();
            out.push_str(&format!("[{}]", names.join(", ")));
        }
        let Type::Method { paramss, ret } = ty else {
            return format!("{out}{}", self.st.display_type(ty));
        };
        for (i, clause) in paramss.iter().enumerate() {
            let ids = s.paramss.get(i).cloned().unwrap_or_default();
            let implicit = ids
                .first()
                .is_some_and(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT));
            let parts: Vec<String> = clause
                .iter()
                .enumerate()
                .map(|(j, t)| {
                    let n = ids
                        .get(j)
                        .map(|p| self.st.get(*p).name.clone())
                        .unwrap_or_else(|| format!("x${j}"));
                    format!("{n}: {}", self.st.display_type(t))
                })
                .collect();
            out.push('(');
            if implicit {
                out.push_str("implicit ");
            }
            out.push_str(&parts.join(", "));
            out.push(')');
        }
        format!("{out}: {}", self.st.display_type(ret))
    }

    /// Apply the rule to `tree` in value position against `pt` (`NoType`
    /// for none). Returns whether it applied; `tree` is then an error, or,
    /// under `-Xsource:3`, the eta-expanded most specific alternative.
    pub(crate) fn adapt_overloaded_value(&mut self, tree: &mut Tree, pt: &Type) -> bool {
        let Some(alts) = self.overloaded_ref_alternatives(tree) else {
            return false;
        };
        if alts.len() < 2 {
            return false;
        }
        let mut firsts: Vec<(SymbolId, Vec<Type>)> = Vec::new();
        for (s, t) in &alts {
            let Type::Method { paramss, .. } = t else {
                return false;
            };
            let Some(first) = paramss.first().filter(|c| !c.is_empty()) else {
                return false;
            };
            let sym = self.st.get(*s);
            if sym.kind != SymKind::Method || sym.name == "<init>" {
                return false;
            }
            if sym.paramss.first().is_some_and(|c| {
                c.iter()
                    .any(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT))
            }) {
                return false;
            }
            firsts.push((*s, first.clone()));
        }
        // nsc `isWeaklyCompatible`: a method is compatible with `pt` as the
        // function it would eta-expand to. None compatible is a plain type
        // mismatch, which `adapt` reports.
        let compatible: Vec<usize> = (0..alts.len())
            .filter(|&i| {
                if pt.is_no_type() || matches!(pt, Type::Any) {
                    return true;
                }
                let Type::Method { paramss, ret } = &alts[i].1 else {
                    return false;
                };
                let fn_ty = Type::Function {
                    params: paramss[0].clone(),
                    ret: ret.clone(),
                };
                self.st.is_sub_type(&fn_ty, pt)
            })
            .collect();
        if compatible.is_empty() {
            return false;
        }
        // nsc's alternatives come most recently entered first, and a
        // subclass's members before the ones it inherits.
        let mut order: Vec<usize> = compatible;
        order.sort_by(|&a, &b| {
            let (sa, sb) = (firsts[a].0, firsts[b].0);
            if self.owner_is_proper_subclass(sa, sb) {
                std::cmp::Ordering::Less
            } else if self.owner_is_proper_subclass(sb, sa) {
                std::cmp::Ordering::Greater
            } else {
                sb.0.cmp(&sa.0)
            }
        });
        let bests: Vec<usize> = order
            .iter()
            .copied()
            .filter(|&a| {
                order.iter().all(|&b| {
                    a == b
                        || self.alt_strictly_more_specific(
                            (firsts[a].0, &firsts[a].1),
                            (firsts[b].0, &firsts[b].1),
                        )
                })
            })
            .collect();
        if let [best] = bests.as_slice() {
            let (s, t) = alts[*best].clone();
            tree.sym = s;
            tree.ty = t;
            if let Some(g) = self.overload_groups.get(&firsts[0].0 .0).cloned() {
                self.overload_groups.insert(s.0, g);
            }
            // `adapt_method_value` reports it, or eta-expands it.
            return false;
        }
        let (a, b) = (order[0], order[1]);
        let describe = |this: &Self, i: usize| -> String {
            let (s, t) = &alts[i];
            let owner = this.st.get(*s).owner;
            format!(
                "method {} in {} of type {}",
                this.st.get(*s).name,
                this.defining_owner_desc(owner),
                this.nsc_method_type_string(*s, t)
            )
        };
        let shown_pt = if pt.is_no_type() {
            "?".to_string()
        } else {
            self.st.display_type(pt)
        };
        let msg = format!(
            "ambiguous reference to overloaded definition,\nboth {}\nand  {}\nmatch expected type {shown_pt}",
            describe(self, a),
            describe(self, b)
        );
        self.error(tree.span, msg);
        tree.ty = Type::Error;
        true
    }
}
