//! In-scope implicit vals/defs (including those inherited from parent
//! class/trait), companions of the parts of the target type (type constructor,
//! type arguments, nested prefixes), imported implicits, and package objects
//! of the enclosing package.
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
            // Instance implicits on this class/module, walking parents (nsc
            // linearization is not reproduced; inheritance is).
            let mut work = vec![self.st.this_class];
            let mut walked = std::collections::HashSet::new();
            while let Some(id) = work.pop() {
                if id.is_none() || !walked.insert(id.0) {
                    continue;
                }
                for m in self.st.get(id).members.clone() {
                    if self.st.get(m).flags.contains(Flags::IMPLICIT) && seen.insert(m.0) {
                        out.push(m);
                    }
                }
                for p in self.st.get(id).parents.clone() {
                    if let Some(ps) = self.st.class_sym_of(&p) {
                        work.push(ps);
                    }
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

    /// Implicit members of the companion module of `class_id` (or the module
    /// class itself when `class_id` is already a module / module class).
    fn companion_implicits_of_class(&self, class_id: SymbolId) -> Vec<SymbolId> {
        let mut out = Vec::new();
        if class_id.is_none() {
            return out;
        }
        let mcls = match self.st.get(class_id).kind {
            SymKind::Module => self.st.module_class_of(class_id),
            SymKind::ModuleClass => class_id,
            _ => {
                let Some(module) = self.st.companion_module(class_id) else {
                    return out;
                };
                self.st.module_class_of(module)
            }
        };
        for mem in &self.st.get(mcls).members {
            if self.st.get(*mem).flags.contains(Flags::IMPLICIT) {
                out.push(*mem);
            }
        }
        out
    }

    /// nsc-style parts of a type: the type constructor, type arguments, and
    /// enclosing class/module prefixes of nested types.
    fn collect_type_parts(
        &self,
        ty: &Type,
        out: &mut Vec<SymbolId>,
        seen: &mut std::collections::HashSet<u32>,
    ) {
        match ty {
            Type::Class { sym, args } => {
                self.collect_class_and_enclosing(*sym, out, seen);
                for a in args {
                    self.collect_type_parts(a, out, seen);
                }
            }
            Type::Applied { ctor, args } => {
                self.collect_type_parts(ctor, out, seen);
                for a in args {
                    self.collect_type_parts(a, out, seen);
                }
            }
            Type::Named { args, .. } => {
                if let Some(id) = self.st.class_sym_of(ty) {
                    self.collect_class_and_enclosing(id, out, seen);
                }
                for a in args {
                    self.collect_type_parts(a, out, seen);
                }
            }
            Type::ModuleRef(s) => self.collect_class_and_enclosing(*s, out, seen),
            Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => {
                self.collect_type_parts(t, out, seen);
            }
            Type::Function { params, ret } => {
                for p in params {
                    self.collect_type_parts(p, out, seen);
                }
                self.collect_type_parts(ret, out, seen);
            }
            Type::Tuple(ts) => {
                for t in ts {
                    self.collect_type_parts(t, out, seen);
                }
            }
            Type::Method { paramss, ret } => {
                for c in paramss {
                    for p in c {
                        self.collect_type_parts(p, out, seen);
                    }
                }
                self.collect_type_parts(ret, out, seen);
            }
            _ => {
                if let Some(id) = self.st.class_sym_of(ty) {
                    self.collect_class_and_enclosing(id, out, seen);
                }
            }
        }
    }

    fn collect_class_and_enclosing(
        &self,
        id: SymbolId,
        out: &mut Vec<SymbolId>,
        seen: &mut std::collections::HashSet<u32>,
    ) {
        if id.is_none() || !seen.insert(id.0) {
            return;
        }
        out.push(id);
        let owner = self.st.get(id).owner;
        if owner.is_none() {
            return;
        }
        match self.st.get(owner).kind {
            SymKind::Class | SymKind::ModuleClass | SymKind::Module => {
                self.collect_class_and_enclosing(owner, out, seen);
            }
            _ => {}
        }
    }

    fn companion_implicits(&self, ty: &Type) -> Vec<SymbolId> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut parts = Vec::new();
        self.collect_type_parts(ty, &mut parts, &mut std::collections::HashSet::new());
        for cls in parts {
            for mem in self.companion_implicits_of_class(cls) {
                if seen.insert(mem.0) {
                    out.push(mem);
                }
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
                empty && self.implicit_result_conforms(ret, pt)
            }
            Type::Function { params, ret } if params.is_empty() => {
                self.implicit_result_conforms(ret, pt)
            }
            t => self.implicit_result_conforms(t, pt),
        }
    }

    /// ClassTag is invariant. Covariant `is_sub_type` would let
    /// `ClassTag[Nothing]` inhabit `ClassTag[Int]` (`Nothing <: Int`) and
    /// `newArray` would then allocate `Object[]`.
    fn implicit_result_conforms(&self, have: &Type, pt: &Type) -> bool {
        match (have, pt) {
            (Type::Class { sym: s1, args: a1 }, Type::Class { sym: s2, args: a2 })
                if s1 == s2 && !a1.is_empty() && !a2.is_empty() && a1.len() == a2.len() =>
            {
                a1.iter().zip(a2.iter()).all(|(x, y)| {
                    x == y || (self.st.is_sub_type(x, y) && self.st.is_sub_type(y, x))
                })
            }
            _ => self.st.is_sub_type(have, pt),
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
    /// of `b`'s, and (for conversions) `a`'s argument type is a subtype of `b`'s,
    /// **or** `a`'s defining class is a subclass of `b`'s (origin).
    /// Type and origin can disagree (inherited more-specific vs local less-specific)
    /// and then `most_specific` reports ambiguous, matching nsc.
    fn is_as_specific(&self, a: SymbolId, b: SymbolId) -> bool {
        self.is_as_specific_type(a, b) || self.is_as_specific_origin(a, b)
    }

    fn is_as_specific_type(&self, a: SymbolId, b: SymbolId) -> bool {
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

    /// Direct owner must be class-like (nsc `owner.isSubClass`). A method-local
    /// implicit's owner is the method, so it does not win on origin against an
    /// inherited class member.
    fn is_as_specific_origin(&self, a: SymbolId, b: SymbolId) -> bool {
        let oa = self.st.get(a).owner;
        let ob = self.st.get(b).owner;
        if oa.is_none() || ob.is_none() || oa == ob {
            return false;
        }
        if !self.st.get(oa).is_class_like() || !self.st.get(ob).is_class_like() {
            return false;
        }
        self.st.is_sub_type(
            &Type::Class {
                sym: oa,
                args: vec![],
            },
            &Type::Class {
                sym: ob,
                args: vec![],
            },
        )
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
        &mut self,
        from: &Type,
        name: &str,
        span: Span,
    ) -> Option<(SymbolId, SymbolId, Type)> {
        let mut hits: Vec<(SymbolId, SymbolId, Type)> = Vec::new();
        let mut ids = self.implicits_in_scope();
        ids.extend(
            self.companion_implicits(from)
                .into_iter()
                .chain(self.companion_implicits(&Type::Any)),
        );
        ids.sort_by_key(|id| id.0);
        ids.dedup();
        for id in ids {
            let Some(to) = self.conversion_result(id, from) else {
                continue;
            };
            let Some(cls) = self.st.class_sym_of(&to) else {
                continue;
            };
            // Load the conversion *result* (e.g. ListHasAsScala) so `asScala`
            // is visible. Do not complete the *argument* type: that would
            // install `java.lang.String#toUpperCase(Locale)` onto Predef
            // String and shadow StringOps.
            self.ensure_java_loaded(cls, span);
            let members = self.st.lookup_member(cls, name);
            if let Some(m) = members.first() {
                hits.push((id, *m, to));
            }
        }
        hits.sort_by_key(|(c, m, _)| (c.0, m.0));
        hits.dedup_by_key(|(c, m, _)| (c.0, m.0));
        match hits.len() {
            1 => Some(hits.pop().unwrap()),
            0 => None,
            _ => {
                // nsc Predef: `augmentString` (StringOps) wins over `wrapString`
                // (WrappedString / Seq) because wrapString is lower priority.
                // Prefer the conversion whose result *declares* the member.
                let declared: Vec<(SymbolId, SymbolId, Type)> = hits
                    .iter()
                    .filter(|(_, m, to)| self.conversion_declares_member(to, *m))
                    .cloned()
                    .collect();
                if declared.len() == 1 {
                    return Some(declared.into_iter().next().unwrap());
                }
                let pool = if declared.is_empty() { hits } else { declared };
                let convs: Vec<SymbolId> = pool.iter().map(|(c, _, _)| *c).collect();
                let winners: Vec<SymbolId> = convs
                    .iter()
                    .copied()
                    .filter(|&a| {
                        !convs
                            .iter()
                            .any(|&b| self.conv_arg_strictly_more_specific(b, a))
                    })
                    .collect();
                if winners.len() != 1 {
                    return None;
                }
                pool.into_iter().find(|(c, _, _)| *c == winners[0])
            }
        }
    }

    fn conversion_declares_member(&self, to: &Type, member: SymbolId) -> bool {
        let Some(cls) = self.st.class_sym_of(to) else {
            return false;
        };
        self.st.get(cls).members.contains(&member)
    }

    fn conv_arg_strictly_more_specific(&self, a: SymbolId, b: SymbolId) -> bool {
        a != b
            && match (self.conversion_arg_ty(a), self.conversion_arg_ty(b)) {
                (Some(aa), Some(ab)) => {
                    let aa = self.erase_method_tparams(a, &aa);
                    let ab = self.erase_method_tparams(b, &ab);
                    self.st.is_sub_type(&aa, &ab) && !self.st.is_sub_type(&ab, &aa)
                }
                _ => false,
            }
    }

    fn erase_method_tparams(&self, id: SymbolId, ty: &Type) -> Type {
        let tps = self.st.get(id).tparams.clone();
        if tps.is_empty() {
            return ty.clone();
        }
        let wilds = vec![Type::Wildcard; tps.len()];
        crate::symbol::subst_tparams_slice(&tps, &wilds, ty)
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
                if self.conv_param_matches(id, from, &ps[0]) {
                    Some(self.instantiate_conv_type(id, from, &ps[0], (**ret).clone()))
                } else {
                    None
                }
            }
            Type::Function { params, ret } if params.len() == 1 => {
                if self.conv_param_matches(id, from, &params[0]) {
                    Some(self.instantiate_conv_type(id, from, &params[0], (**ret).clone()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn instantiate_conv_type(&self, id: SymbolId, from: &Type, param: &Type, ty: Type) -> Type {
        let tps = self.st.get(id).tparams.clone();
        if tps.is_empty() {
            return ty;
        }
        let args_t: Vec<Type> = tps
            .iter()
            .map(|tp| unify_conv_tparam(*tp, param, from).unwrap_or(Type::AnyRef))
            .collect();
        crate::symbol::subst_tparams_slice(&tps, &args_t, &ty)
    }

    fn conv_param_matches(&self, id: SymbolId, from: &Type, param: &Type) -> bool {
        let param = self.erase_method_tparams(id, param);
        self.st.is_sub_type(from, &param) || matches!(param, Type::Any | Type::Wildcard)
    }

    pub(crate) fn ref_implicit(&self, id: SymbolId, span: Span) -> Tree {
        let s = self.st.get(id);
        let ty = match &s.ty {
            Type::Method { paramss, ret }
                if paramss.is_empty() || paramss.iter().all(|c| c.is_empty()) =>
            {
                (**ret).clone()
            }
            t => t.clone(),
        };
        Tree {
            id: scala_rs_parser::NodeId(0),
            span,
            kind: TreeKind::Ident {
                name: s.name.clone(),
            },
            ty,
            sym: id,
            postfix: false,
        }
    }

    pub(crate) fn describe_implicits(&self, ids: &[SymbolId]) -> String {
        ids.iter()
            .map(|id| self.st.get(*id).name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn unify_conv_tparam(tp: SymbolId, param: &Type, from: &Type) -> Option<Type> {
    match (param, from) {
        (Type::TypeParam(id), actual) if *id == tp => Some(actual.widen_constant()),
        (Type::Array(p), Type::Array(a)) => unify_conv_tparam(tp, p, a),
        (Type::Class { args: pa, .. }, Type::Class { args: fa, .. }) => pa
            .iter()
            .zip(fa.iter())
            .find_map(|(p, f)| unify_conv_tparam(tp, p, f)),
        _ => None,
    }
}
