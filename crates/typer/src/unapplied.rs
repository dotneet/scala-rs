//! nsc `adapt`'s "missing argument list": in 2.13 a method with a parameter
//! list is converted to a function only where a function type is expected.
//! `val w = id` for `def id(i: Int) = i` is an error, and so is `val p = +`
//! (a bare `+` is an identifier when no expression follows it).

use crate::check::*;
use crate::symbol::SymKind;
use scala_rs_parser::ast::*;

impl Typer {
    /// `rhs` of a value with no expected type. Returns whether it reported.
    pub(crate) fn reject_unapplied_method(&mut self, rhs: &mut Tree) -> bool {
        let mut head: &Tree = rhs;
        if let TreeKind::TypeApply { fun, .. } = &head.kind {
            head = fun;
        }
        if !matches!(head.kind, TreeKind::Ident { .. } | TreeKind::Select { .. }) {
            return false;
        }
        let Type::Method { .. } = &rhs.ty else {
            return false;
        };
        let sym = head.sym;
        if sym.is_none() || self.st.get(sym).kind != SymKind::Method {
            return false;
        }
        let paramss = self.st.get(sym).paramss.clone();
        // An empty or implicit-only first clause is applied, not converted.
        match paramss.first() {
            Some(first)
                if !first.is_empty()
                    && !first
                        .iter()
                        .all(|p| self.st.get(*p).flags.contains(Flags::IMPLICIT)) => {}
            _ => return false,
        }
        let name = self.st.get(sym).name.clone();
        let owner = self.st.get(sym).owner;
        let owner_desc = if owner.is_none() {
            String::new()
        } else {
            let o = self.st.get(owner);
            let kind = match o.kind {
                SymKind::Module | SymKind::ModuleClass => "object",
                _ if o.flags.contains(Flags::TRAIT) => "trait",
                SymKind::Class => "class",
                _ => "",
            };
            if kind.is_empty() {
                String::new()
            } else {
                format!(" in {kind} {}", o.name.trim_end_matches('$'))
            }
        };
        // nsc spells every clause, the implicit one included (`h(_,_,_)(_)`).
        let placeholders: String = paramss
            .iter()
            .map(|c| format!("({})", vec!["_"; c.len()].join(",")))
            .collect();
        self.error(
            rhs.span,
            format!(
                "missing argument list for method {name}{owner_desc}\n\
                 Unapplied methods are only converted to functions when a function type is expected.\n\
                 You can make this conversion explicit by writing `{name} _` or `{name}{placeholders}` instead of `{name}`."
            ),
        );
        rhs.ty = Type::Error;
        true
    }
}
