//! nsc `Typers.checkNoDoubleDefs` for the one pair `double_def` (which
//! compares only methods the source wrote) cannot see: a `var x` and a
//! `def x_=(v: T)` in the same template. The `var` has a setter `x_=` of that
//! very signature, so nsc reports "method x_= is defined twice; the
//! conflicting variable x was defined at line L:C" (`neg/t591`). A
//! `private[this] var` has no setter and conflicts with nothing.

use crate::check::*;
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

impl Typer {
    pub(crate) fn check_setter_conflicts(&mut self, class_id: SymbolId, body: &[Tree]) {
        if class_id.is_none() {
            return;
        }
        let vars: Vec<(String, SymbolId, Span)> = body
            .iter()
            .filter_map(|t| match &t.kind {
                TreeKind::ValDef { name, mods, .. }
                    if mods.flags.contains(Flags::MUTABLE)
                        && !mods.flags.contains(Flags::LOCAL)
                        && !mods.flags.contains(Flags::PARAM)
                        && !t.sym.is_none() =>
                {
                    Some((name.clone(), t.sym, t.span))
                }
                _ => None,
            })
            .collect();
        for (name, var, var_span) in vars {
            let setter = format!("{name}_=");
            let var_ty = self.st.get(var).ty.clone();
            for d in body {
                let TreeKind::DefDef { name: dname, .. } = &d.kind else {
                    continue;
                };
                if *dname != setter || d.sym.is_none() {
                    continue;
                }
                let param = match &self.st.get(d.sym).ty {
                    Type::Method { paramss, .. } if paramss.len() == 1 && paramss[0].len() == 1 => {
                        paramss[0][0].clone()
                    }
                    _ => continue,
                };
                let same = param == var_ty
                    || (self.st.is_sub_type(&param, &var_ty)
                        && self.st.is_sub_type(&var_ty, &param));
                if !same || var_ty.is_no_type() || var_ty.is_error() {
                    continue;
                }
                let at = self
                    .var_name_line_col(var_span, &name)
                    .map(|(l, c)| format!(" at line {l}:{c}"))
                    .unwrap_or_default();
                self.error(
                    d.span,
                    format!(
                        "method {setter} is defined twice;\n  the conflicting variable {name} was defined{at}"
                    ),
                );
            }
        }
    }

    /// 1-based line and column of the name a `var` definition at `span`
    /// declares, as nsc prints a symbol's position.
    fn var_name_line_col(&self, span: Span, name: &str) -> Option<(usize, usize)> {
        let src = self.sources.get(self.file_index)?;
        let lo = span.lo.0 as usize;
        let hi = (span.hi.0 as usize).min(src.len());
        let text = src.get(lo..hi)?;
        let var_at = text.find("var")?;
        let after = &text[var_at + 3..];
        let name_at = var_at + 3 + after.find(name)?;
        let offset = lo + name_at;
        let before = &src[..offset];
        let line = before.matches('\n').count() + 1;
        let col = before
            .rsplit('\n')
            .next()
            .map(|l| l.chars().count())
            .unwrap_or(0)
            + 1;
        Some((line, col))
    }
}
