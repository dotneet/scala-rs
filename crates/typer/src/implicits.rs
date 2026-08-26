//! In-scope implicit vals/defs, companions of the target (and source for
//! conversions), imported implicits, and package objects of the enclosing package.
//! `no implicit` / `ambiguous implicit` are hard errors.

use scala_rs_parser::{Flags, SymbolId, Tree, TreeKind, Type};
use scala_rs_span::Span;

use crate::check::Typer;
use crate::symbol::SymKind;

#[derive(Debug)]
pub enum ImplicitSearch {
    Found(SymbolId),
    None,
    Ambiguous(Vec<SymbolId>),
}

impl Typer {
    pub(crate) fn implicits_in_scope(&self) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for sc in self.st.scopes.iter().rev() {
            for name in sc.names() {
                for id in sc.lookup(name) {
                    if self.st.get(*id).flags.contains(Flags::IMPLICIT) && seen.insert(id.0) {
                        out.push(*id);
                    }
                }
            }
        }
        if !self.st.this_class.is_none() {
            for m in self.st.get(self.st.this_class).members.clone() {
                if self.st.get(m).flags.contains(Flags::IMPLICIT) && seen.insert(m.0) {
                    out.push(m);
                }
            }
            // Package object of the enclosing package (members copied onto the
            // package symbol, plus the `package` module itself).
            let mut owner = self.st.get(self.st.this_class).owner;
            while !owner.is_none() {
                let o = self.st.get(owner);
                if o.kind == crate::symbol::SymKind::Package {
                    for m in o.members.clone() {
                        if self.st.get(m).flags.contains(Flags::IMPLICIT) && seen.insert(m.0) {
                            out.push(m);
                        }
                        if self.st.get(m).name == "package" {
                            let mcls = self.st.module_class_of(m);
                            for mem in self.st.get(mcls).members.clone() {
                                if self.st.get(mem).flags.contains(Flags::IMPLICIT)
                                    && seen.insert(mem.0)
                                {
                                    out.push(mem);
                                }
                            }
                        }
                    }
                    break;
                }
                owner = o.owner;
            }
        }
        out
    }

    fn companion_implicits(&self, ty: &Type) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let Some(cls) = self.st.class_sym_of(ty) else {
            return out;
        };
        // Prefer the class (not the module class) when `ty` is a ModuleRef.
        let class_id = if self.st.get(cls).kind == SymKind::ModuleClass {
            let name = self.st.get(cls).name.trim_end_matches('$').to_string();
            let owner = self.st.get(cls).owner;
            self.st
                .get(owner)
                .members
                .iter()
                .copied()
                .find(|&m| self.st.get(m).kind == SymKind::Class && self.st.get(m).name == name)
                .unwrap_or(cls)
        } else {
            cls
        };
        let Some(module) = self.st.companion_module(class_id) else {
            return out;
        };
        let mcls = self.st.module_class_of(module);
        for mem in &self.st.get(mcls).members {
            if self.st.get(*mem).flags.contains(Flags::IMPLICIT) {
                out.push(*mem);
            }
        }
        out
    }

    fn implicit_provides(&self, id: SymbolId, pt: &Type) -> bool {
        let s = self.st.get(id);
        if !s.flags.contains(Flags::IMPLICIT) {
            return false;
        }
        match &s.ty {
            Type::Method { paramss, ret } => {
                let empty = paramss.iter().all(|c| c.is_empty());
                empty && self.st.is_sub_type(ret, pt)
            }
            Type::Function { params, ret } if params.is_empty() => self.st.is_sub_type(ret, pt),
            t => self.st.is_sub_type(t, pt),
        }
    }

    fn conversion_provides(&self, id: SymbolId, from: &Type, to: &Type) -> bool {
        let s = self.st.get(id);
        if !s.flags.contains(Flags::IMPLICIT) {
            return false;
        }
        match &s.ty {
            Type::Method { paramss, ret } => {
                let ps = paramss.first().cloned().unwrap_or_default();
                if ps.len() != 1 {
                    return false;
                }
                self.st.is_sub_type(from, &ps[0]) && self.st.is_sub_type(ret, to)
            }
            Type::Function { params, ret } if params.len() == 1 => {
                self.st.is_sub_type(from, &params[0]) && self.st.is_sub_type(ret, to)
            }
            _ => false,
        }
    }

    pub(crate) fn search_implicit(&self, pt: &Type) -> ImplicitSearch {
        let local: Vec<SymbolId> = self
            .implicits_in_scope()
            .into_iter()
            .filter(|id| self.implicit_provides(*id, pt))
            .collect();
        if !local.is_empty() {
            return self.most_specific(local);
        }
        let mut comps: Vec<SymbolId> = self
            .companion_implicits(pt)
            .into_iter()
            .filter(|id| self.implicit_provides(*id, pt))
            .collect();
        comps.sort_by_key(|id| id.0);
        comps.dedup();
        self.most_specific(comps)
    }

    pub(crate) fn search_conversion(&self, from: &Type, to: &Type) -> ImplicitSearch {
        let local: Vec<SymbolId> = self
            .implicits_in_scope()
            .into_iter()
            .filter(|id| self.conversion_provides(*id, from, to))
            .collect();
        if !local.is_empty() {
            return self.most_specific(local);
        }
        let mut comps: Vec<SymbolId> = self
            .companion_implicits(to)
            .into_iter()
            .chain(self.companion_implicits(from))
            .filter(|id| self.conversion_provides(*id, from, to))
            .collect();
        comps.sort_by_key(|id| id.0);
        comps.dedup();
        self.most_specific(comps)
    }

    /// nsc-style: `a` is as specific as `b` when `a`'s result type is a subtype
    /// of `b`'s, and (for conversions) `a`'s argument type is a subtype of `b`'s.
    fn is_as_specific(&self, a: SymbolId, b: SymbolId) -> bool {
        let ra = self.implicit_result_ty(a);
        let rb = self.implicit_result_ty(b);
        if !self.st.is_sub_type(&ra, &rb) {
            return false;
        }
        match (self.conversion_arg_ty(a), self.conversion_arg_ty(b)) {
            (Some(aa), Some(ab)) => self.st.is_sub_type(&aa, &ab),
            (Some(_), None) => false,
            (None, Some(_)) => true,
            (None, None) => true,
        }
    }

    fn strictly_more_specific(&self, a: SymbolId, b: SymbolId) -> bool {
        a != b && self.is_as_specific(a, b) && !self.is_as_specific(b, a)
    }

    fn most_specific(&self, cands: Vec<SymbolId>) -> ImplicitSearch {
        match cands.len() {
            0 => ImplicitSearch::None,
            1 => ImplicitSearch::Found(cands[0]),
            _ => {
                let winners: Vec<SymbolId> = cands
                    .iter()
                    .copied()
                    .filter(|&a| !cands.iter().any(|&b| self.strictly_more_specific(b, a)))
                    .collect();
                match winners.len() {
                    0 => ImplicitSearch::Ambiguous(cands),
                    1 => ImplicitSearch::Found(winners[0]),
                    _ => ImplicitSearch::Ambiguous(winners),
                }
            }
        }
    }

    fn implicit_result_ty(&self, id: SymbolId) -> Type {
        match &self.st.get(id).ty {
            Type::Method { ret, .. } => (**ret).clone(),
            Type::Function { ret, .. } => (**ret).clone(),
            t => t.clone(),
        }
    }

    fn conversion_arg_ty(&self, id: SymbolId) -> Option<Type> {
        match &self.st.get(id).ty {
            Type::Method { paramss, .. } => {
                let ps = paramss.first()?;
                if ps.len() == 1 {
                    Some(ps[0].clone())
                } else {
                    None
                }
            }
            Type::Function { params, .. } if params.len() == 1 => Some(params[0].clone()),
            _ => None,
        }
    }

    /// Implicit conversion from `from` whose result type has member `name`.
    pub(crate) fn search_extension(
        &self,
        from: &Type,
        name: &str,
    ) -> Option<(SymbolId, SymbolId, Type)> {
        let mut hits: Vec<(SymbolId, SymbolId, Type)> = Vec::new();
        let mut consider = |id: SymbolId| {
            let Some(to) = self.conversion_result(id, from) else {
                return;
            };
            let Some(cls) = self.st.class_sym_of(&to) else {
                return;
            };
            let members = self.st.lookup_member(cls, name);
            if let Some(m) = members.first() {
                hits.push((id, *m, to));
            }
        };
        for id in self.implicits_in_scope() {
            consider(id);
        }
        for id in self
            .companion_implicits(from)
            .into_iter()
            .chain(self.companion_implicits(&Type::Any))
        {
            consider(id);
        }
        hits.sort_by_key(|(c, m, _)| (c.0, m.0));
        hits.dedup_by_key(|(c, m, _)| (c.0, m.0));
        match hits.len() {
            1 => Some(hits.pop().unwrap()),
            _ => None,
        }
    }

    fn conversion_result(&self, id: SymbolId, from: &Type) -> Option<Type> {
        let s = self.st.get(id);
        if !s.flags.contains(Flags::IMPLICIT) {
            return None;
        }
        match &s.ty {
            Type::Method { paramss, ret } => {
                let ps = paramss.first().cloned().unwrap_or_default();
                if ps.len() != 1 {
                    return None;
                }
                if self.st.is_sub_type(from, &ps[0]) || matches!(ps[0], Type::Any) {
                    Some((**ret).clone())
                } else {
                    None
                }
            }
            Type::Function { params, ret } if params.len() == 1 => {
                if self.st.is_sub_type(from, &params[0]) || matches!(params[0], Type::Any) {
                    Some((**ret).clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub(crate) fn ref_implicit(&self, id: SymbolId, span: Span) -> Tree {
        let s = self.st.get(id);
        let ty = s.ty.clone();
        Tree {
            id: scala_rs_parser::NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: s.name.clone(),
            },
            ty,
            sym: id,
        }
    }

    pub(crate) fn describe_implicits(&self, ids: &[SymbolId]) -> String {
        ids.iter()
            .map(|id| self.st.get(*id).name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
