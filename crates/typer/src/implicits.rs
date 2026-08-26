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
        if local.len() == 1 {
            return ImplicitSearch::Found(local[0]);
        }
        if local.len() > 1 {
            return ImplicitSearch::Ambiguous(local);
        }
        let mut comps: Vec<SymbolId> = self
            .companion_implicits(pt)
            .into_iter()
            .filter(|id| self.implicit_provides(*id, pt))
            .collect();
        comps.sort_by_key(|id| id.0);
        comps.dedup();
        match comps.len() {
            0 => ImplicitSearch::None,
            1 => ImplicitSearch::Found(comps[0]),
            _ => ImplicitSearch::Ambiguous(comps),
        }
    }

    pub(crate) fn search_conversion(&self, from: &Type, to: &Type) -> ImplicitSearch {
        let local: Vec<SymbolId> = self
            .implicits_in_scope()
            .into_iter()
            .filter(|id| self.conversion_provides(*id, from, to))
            .collect();
        if local.len() == 1 {
            return ImplicitSearch::Found(local[0]);
        }
        if local.len() > 1 {
            return ImplicitSearch::Ambiguous(local);
        }
        let mut comps: Vec<SymbolId> = self
            .companion_implicits(to)
            .into_iter()
            .chain(self.companion_implicits(from))
            .filter(|id| self.conversion_provides(*id, from, to))
            .collect();
        comps.sort_by_key(|id| id.0);
        comps.dedup();
        match comps.len() {
            0 => ImplicitSearch::None,
            1 => ImplicitSearch::Found(comps[0]),
            _ => ImplicitSearch::Ambiguous(comps),
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
