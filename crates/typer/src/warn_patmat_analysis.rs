//! nsc's `MatchAnalysis` and the domain half of `Logic.scala`
//! (`TreesAndTypesDomain`): the approximation of a match's `TreeMaker`s as
//! propositions (`TreeMakersToProps`), the variables and constants they are
//! over, `removeVarEq`, and the two analyses built on them --
//! `unreachableCase` and `exhaustive` with its counter-examples.

use crate::check::Typer;
use crate::symbol::SymKind;
use crate::warn_patmat::*;
use crate::warn_patmat_logic::*;
use crate::warn_patmat_translate::{bail, Unsupported};
use crate::warn_patmat_types::{sealed_children, CVal, NTy, Types};
use scala_rs_parser::ast::*;
use std::collections::{HashMap, HashSet};

struct VarInfo {
    path: PTree,
    static_tp: NTy,
    checkable: NTy,
    /// `symForEqualsTo`
    sym_for: Vec<(ConstId, SymId)>,
    may_be_null: bool,
    domain: Option<Option<Vec<ConstId>>>,
    domain_syms: Option<Option<Vec<SymId>>>,
    sym_static: Option<Option<SymId>>,
    implications: Option<Vec<(SymId, Vec<SymId>, Vec<SymId>)>>,
    grouped: Option<Vec<Vec<SymId>>>,
}

pub(crate) struct ConstInfo {
    pub(crate) tp: NTy,
    pub(crate) wide: NTy,
    pub(crate) is_value: bool,
    pub(crate) text: String,
    pub(crate) is_null: bool,
    /// A `TypeConst` (as opposed to a `ValueConst` or `NullConst`).
    pub(crate) is_type: bool,
}

struct SymInfo {
    var: VarId,
    konst: ConstId,
    id: usize,
}

/// Which `TreeMaker`s the approximation treats as unknown, and as what.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unknown {
    /// `unreachableCase`: `refutableRewrite`, else the default.
    Reach(bool),
    /// `exhaustive`: `fullRewrite`, else `True` for a body and `False` for
    /// an extractor, product or guard (strict mode).
    Exhaust,
}

/// One `TreeMakersToProps`, with the hash-consing of variables and constants
/// it resets (`prepareNewAnalysis`).
pub(crate) struct Approx<'x> {
    pub(crate) t: &'x mut Typer,
    binders: &'x [Binder],
    root: B,
    sym_ids: &'x mut usize,
    points_to_bound: HashSet<B>,
    trees: Vec<(PTree, NTy)>,
    extract_binders: Vec<((SymbolId, PTree), B)>,
    normalize: Subst,
    accum: Subst,
    computed: bool,
    eq_props: HashMap<(PTree, PatKey), Prop>,
    nonnull_props: HashMap<PTree, Prop>,
    type_props: HashMap<(PTree, NTy), Prop>,
    pat_trees: HashMap<PatKey, PatVal>,
    vars: Vec<VarInfo>,
    var_by_path: HashMap<PTree, VarId>,
    pub(crate) consts: Vec<ConstInfo>,
    const_uniques: Vec<(NTy, ConstId)>,
    null_const: ConstId,
    syms: Vec<SymInfo>,
    backoff: bool,
    children_cache: HashMap<SymbolId, Option<Vec<SymbolId>>>,
    pickled_cache: HashMap<SymbolId, u64>,
}

impl<'x> Approx<'x> {
    pub(crate) fn new(t: &'x mut Typer, binders: &'x [Binder], root: B, sym_ids: &'x mut usize) -> Self {
        let mut a = Approx {
            t,
            binders,
            root,
            sym_ids,
            points_to_bound: HashSet::from([root]),
            trees: Vec::new(),
            extract_binders: Vec::new(),
            normalize: Subst::default(),
            accum: Subst::default(),
            computed: false,
            eq_props: HashMap::new(),
            nonnull_props: HashMap::new(),
            type_props: HashMap::new(),
            pat_trees: HashMap::new(),
            vars: Vec::new(),
            var_by_path: HashMap::new(),
            consts: Vec::new(),
            const_uniques: Vec::new(),
            null_const: 0,
            syms: Vec::new(),
            backoff: false,
            children_cache: HashMap::new(),
            pickled_cache: HashMap::new(),
        };
        a.consts.push(ConstInfo {
            tp: NTy::Null,
            wide: NTy::Null,
            is_value: true,
            text: "null".into(),
            is_null: true,
            is_type: false,
        });
        a.null_const = 0;
        a
    }

    fn tys(&self) -> Types<'_> {
        Types { st: &self.t.st }
    }

    // --- binders and paths -------------------------------------------------

    fn unique(&mut self, t: PTree, tp: NTy) -> PTree {
        if let Some((orig, _)) = self.trees.iter().find(|(o, _)| *o == t) {
            return orig.clone();
        }
        self.trees.push((t.clone(), tp));
        t
    }

    fn tree_type(&self, t: &PTree) -> NTy {
        self.trees
            .iter()
            .find(|(o, _)| o == t)
            .map(|(_, tp)| tp.clone())
            .unwrap_or(NTy::Unknown)
    }

    /// `binderToUniqueTree`
    fn binder_tree(&mut self, b: B) -> PTree {
        let t = self.accum.apply(&self.normalize.apply(&PTree::Ref(b)));
        let tp = self.binders[b].tp.clone();
        self.unique(t, tp)
    }

    fn mentions(t: &PTree, b: B) -> bool {
        match t {
            PTree::Ref(x) => *x == b,
            PTree::Sel(p, _) => Self::mentions(p, b),
        }
    }

    /// `TreeMakerToProp.updateSubstitution`
    fn update_substitution(&mut self, m: &Maker) {
        let subst = m.sub_patterns_as_substitution();
        if let TM::Extractor { ext, has_extra: false, next, prev, .. } = &m.tm {
            let arg = self.accum.apply(&self.normalize.apply(&PTree::Ref(*prev)));
            let key = (ext.unapply, arg);
            match self.extract_binders.iter().find(|(k, _)| *k == key).map(|(_, b)| *b) {
                Some(reuse) => {
                    let to = self.binder_tree(reuse);
                    self.normalize = self.normalize.then(&Subst::one(*next, to));
                }
                None => self.extract_binders.push((key, *next)),
            }
        }
        let mut bound_from = Vec::new();
        let mut bound_to = Vec::new();
        let mut unbound_from = Vec::new();
        let mut unbound_to = Vec::new();
        for (f, t) in subst.from.iter().zip(&subst.to) {
            match t {
                PTree::Ref(sym) if self.points_to_bound.contains(f) => {
                    bound_from.push(PTree::Ref(*f));
                    bound_to.push(*sym);
                }
                _ => {
                    unbound_from.push(*f);
                    unbound_to.push(self.normalize.apply(t));
                }
            }
        }
        self.normalize = self.normalize.then(&Subst {
            from: bound_to,
            to: bound_from,
        });
        let ok = Subst {
            from: unbound_from,
            to: unbound_to,
        };
        let is_extractor = matches!(m.tm, TM::Extractor { .. });
        for (f, t) in ok.from.iter().zip(&ok.to) {
            let hits = self.points_to_bound.iter().any(|b| Self::mentions(t, *b));
            if hits || is_extractor {
                self.points_to_bound.insert(*f);
            }
        }
        self.accum = self.accum.then(&ok);
    }

    // --- variables, constants, symbols ---------------------------------------

    fn var(&mut self, path: &PTree) -> VarId {
        if let Some(v) = self.var_by_path.get(path) {
            return *v;
        }
        let static_tp = self.tree_type(path);
        let checkable = self.tys().checkable(&static_tp);
        self.vars.push(VarInfo {
            path: path.clone(),
            static_tp,
            checkable,
            sym_for: Vec::new(),
            may_be_null: false,
            domain: None,
            domain_syms: None,
            sym_static: None,
            implications: None,
            grouped: None,
        });
        let id = self.vars.len() - 1;
        self.var_by_path.insert(path.clone(), id);
        id
    }

    fn const_unique(&mut self, key: NTy, make: impl FnOnce(&Self) -> ConstInfo) -> ConstId {
        if let Some((_, c)) = self.const_uniques.iter().find(|(k, _)| *k == key) {
            return *c;
        }
        let info = make(self);
        self.consts.push(info);
        let id = self.consts.len() - 1;
        self.const_uniques.push((key, id));
        id
    }

    /// nsc's `widenToClass`.
    fn widen_to_class(&self, tp: &NTy) -> NTy {
        match tp {
            NTy::Fresh(_, b) => self.widen_to_class(b),
            other => other.clone(),
        }
    }

    /// `TypeConst(tp)`
    fn type_const(&mut self, tp: &NTy) -> ConstId {
        if *tp == NTy::Null {
            return self.null_const;
        }
        if tp.is_singleton() {
            // `ValueConst.fromType`
            let text = match tp {
                NTy::Const(c) => c.escaped(),
                NTy::Module(m) => self.t.st.get(*m).name.trim_end_matches('$').to_string(),
                other => self.tys().show(other),
            };
            let wide = self.tys().widen(tp);
            let tp2 = tp.clone();
            return self.const_unique(tp.clone(), move |_| ConstInfo {
                tp: tp2,
                wide,
                is_value: true,
                text,
                is_null: false,
                is_type: false,
            });
        }
        let wide = self.widen_to_class(tp);
        let text = self.tys().show(tp);
        let tp2 = tp.clone();
        self.const_unique(tp.clone(), move |_| ConstInfo {
            tp: tp2,
            wide,
            is_value: false,
            text,
            is_null: false,
            is_type: true,
        })
    }

    /// `ValueConst(patTree)`
    fn value_const(&mut self, p: &PatVal) -> ConstId {
        if p.tp == NTy::Null {
            return self.null_const;
        }
        let narrow = p.tp.clone();
        let wide = {
            let w = self.widen_to_class(&narrow);
            self.tys().checkable(&w)
        };
        let text = p.text.clone();
        let n2 = narrow.clone();
        self.const_unique(narrow, move |_| ConstInfo {
            tp: n2,
            wide,
            is_value: true,
            text,
            is_null: false,
            is_type: false,
        })
    }

    fn const_text(&self, c: ConstId) -> &str {
        &self.consts[c].text
    }

    fn register_equality(&mut self, v: VarId, c: ConstId) -> SymId {
        if let Some((_, s)) = self.vars[v].sym_for.iter().find(|(k, _)| *k == c) {
            return *s;
        }
        *self.sym_ids += 1;
        self.syms.push(SymInfo {
            var: v,
            konst: c,
            id: *self.sym_ids,
        });
        let s = self.syms.len() - 1;
        self.vars[v].sym_for.push((c, s));
        s
    }

    fn prop_for_equals_to(&self, v: VarId, c: ConstId) -> Prop {
        match self.vars[v].sym_for.iter().find(|(k, _)| *k == c) {
            Some((_, s)) => Prop::Sym(*s),
            None => Prop::False,
        }
    }

    fn sym_string(&self, s: SymId) -> String {
        let si = &self.syms[s];
        format!("V{}={}#{}", si.var + 1, self.const_text(si.konst), si.id)
    }

    fn register_null(&mut self, v: VarId) {
        let checkable = self.vars[v].checkable.clone();
        if self.tys().sub(&NTy::Null, &checkable) == Some(true) {
            self.vars[v].may_be_null = true;
        }
    }

    fn children(&mut self, cls: SymbolId) -> Option<Vec<SymbolId>> {
        if let Some(c) = self.children_cache.get(&cls) {
            return c.clone();
        }
        let c = sealed_children(self.t, cls);
        self.children_cache.insert(cls, c.clone());
        c
    }

    /// `enumerateSubtypes`
    fn enumerate_subtypes(&mut self, tp: &NTy, grouped: bool) -> Result<Vec<Vec<NTy>>, Unsupported> {
        let st_unit = self.t.st.unit_sym;
        let st_bool = self.t.st.boolean_sym;
        let Some(sym) = self.tys().type_symbol(tp) else {
            return Ok(Vec::new());
        };
        if sym == st_unit {
            return Ok(vec![vec![NTy::Class(st_unit, vec![])]]);
        }
        if sym == st_bool {
            return Ok(vec![vec![NTy::Const(CVal::Bool(true)), NTy::Const(CVal::Bool(false))]]);
        }
        let s = self.t.st.get(sym);
        if s.kind == SymKind::ModuleClass {
            return Ok(vec![vec![tp.clone()]]);
        }
        let is_case = s.flags.contains(Flags::CASE) && s.kind == SymKind::Class;
        if self.is_sealed(sym) {
            return self.enumerate_sealed(tp, sym, grouped);
        }
        if is_case {
            return Ok(vec![vec![tp.clone()]]);
        }
        Ok(Vec::new())
    }

    /// Our flags, completed from the pickle for a library class.
    fn class_flags(&mut self, c: SymbolId) -> (Flags, u64) {
        let ours = self.t.st.get(c).flags;
        if let Some(f) = self.pickled_cache.get(&c) {
            return (ours, *f);
        }
        let f = crate::warn_patmat_types::pickled_flags(self.t, c).unwrap_or(0);
        self.pickled_cache.insert(c, f);
        (ours, f)
    }

    fn is_sealed(&mut self, c: SymbolId) -> bool {
        use scala_rs_pickle::read::pflags;
        let (f, p) = self.class_flags(c);
        f.contains(Flags::SEALED) || p & pflags::SEALED != 0
    }

    fn is_trait(&mut self, c: SymbolId) -> bool {
        use scala_rs_pickle::read::pflags;
        let (f, p) = self.class_flags(c);
        f.contains(Flags::TRAIT) || p & pflags::TRAIT != 0
    }

    fn is_abstract_class(&mut self, c: SymbolId) -> bool {
        use scala_rs_pickle::read::pflags;
        let (f, p) = self.class_flags(c);
        f.contains(Flags::TRAIT)
            || f.contains(Flags::ABSTRACT)
            || f.contains(Flags::INTERFACE)
            || p & (pflags::TRAIT | pflags::ABSTRACT | pflags::INTERFACE) != 0
    }

    fn sort_name(&self, c: SymbolId) -> String {
        format!("{}#{}", self.t.st.get(c).name, c.0)
    }

    fn filter_and_sort(&mut self, children: Vec<SymbolId>) -> Vec<SymbolId> {
        let mut c1: Vec<SymbolId> = Vec::new();
        for c in children {
            let enum_flag = self.t.st.get(c).flags.contains(Flags::ENUM);
            let drop = self.is_sealed(c) && (self.is_abstract_class(c) || enum_flag);
            if !drop {
                c1.push(c);
            }
        }
        c1.sort_by_key(|c| self.sort_name(*c));
        c1.dedup();
        let all = c1.clone();
        let mut out = Vec::new();
        for c in c1 {
            let private = self.t.st.get(c).flags.contains(Flags::PRIVATE);
            let drop = private
                && self.is_abstract_class(c)
                && all
                    .iter()
                    .any(|o| *o != c && crate::pickle_supply::inherits_from(&self.t.st, *o, c));
            if !drop {
                out.push(c);
            }
        }
        out
    }

    fn sealed_descendants(&mut self, c: SymbolId, out: &mut Vec<SymbolId>) -> Result<(), Unsupported> {
        if !out.contains(&c) {
            out.push(c);
        } else {
            return Ok(());
        }
        if self.is_sealed(c) {
            let kids = self.children(c).ok_or_else(|| bail!())?;
            for k in kids {
                self.sealed_descendants(k, out)?;
            }
        }
        Ok(())
    }

    fn enumerate_sealed(&mut self, tp: &NTy, sym: SymbolId, grouped: bool) -> Result<Vec<Vec<NTy>>, Unsupported> {
        let groups: Vec<Vec<SymbolId>> = if grouped {
            let mut acc: Vec<Vec<SymbolId>> = Vec::new();
            let mut wl: std::collections::VecDeque<SymbolId> = std::collections::VecDeque::from([sym]);
            let mut guard = 0;
            while let Some(hd) = wl.pop_front() {
                guard += 1;
                if guard > 1000 {
                    return Err(bail!());
                }
                let kids = if self.is_sealed(hd) {
                    self.children(hd).ok_or_else(|| bail!())?
                } else {
                    Vec::new()
                };
                let children = self.filter_and_sort(kids);
                let mut traits = Vec::new();
                let mut non_traits = Vec::new();
                for c in &children {
                    if self.is_trait(*c) {
                        traits.push(*c);
                    } else {
                        non_traits.push(*c);
                    }
                }
                for t in traits {
                    acc.push(vec![t]);
                }
                acc.push(non_traits);
                wl.extend(children);
            }
            acc
        } else {
            let mut desc = Vec::new();
            self.sealed_descendants(sym, &mut desc)?;
            vec![self.filter_and_sort(desc)]
        };
        let mut out = Vec::new();
        for g in groups {
            let mut tps = Vec::new();
            for c in g {
                let s = self.t.st.get(c);
                let sub_tp = match s.kind {
                    SymKind::ModuleClass => NTy::Module(c),
                    SymKind::Module => NTy::Module(self.t.st.module_class_of(c)),
                    SymKind::Class => NTy::Class(c, vec![NTy::Wild; s.tparams.len()]),
                    _ => return Err(bail!()),
                };
                match self.tys().sub(&sub_tp, tp) {
                    Some(true) => tps.push(self.tys().checkable(&sub_tp)),
                    Some(false) => {}
                    None => return Err(bail!()),
                }
            }
            out.push(tps);
        }
        Ok(out)
    }

    fn domain(&mut self, v: VarId) -> Result<Option<Vec<ConstId>>, Unsupported> {
        if let Some(d) = &self.vars[v].domain {
            return Ok(d.clone());
        }
        let static_tp = self.vars[v].static_tp.clone();
        let subtypes = self.enumerate_subtypes(&static_tp, false)?;
        let mut sub_consts: Option<Vec<ConstId>> = match subtypes.into_iter().next() {
            Some(tps) => {
                let mut seen: Vec<NTy> = Vec::new();
                let mut consts = Vec::new();
                for tp in tps {
                    if seen.contains(&tp) {
                        continue;
                    }
                    seen.push(tp.clone());
                    let c = self.type_const(&tp);
                    self.register_equality(v, c);
                    if !consts.contains(&c) {
                        consts.push(c);
                    }
                }
                Some(consts)
            }
            None => None,
        };
        if self.vars[v].may_be_null {
            let n = self.null_const;
            self.register_equality(v, n);
            if let Some(cs) = &mut sub_consts {
                if !cs.contains(&n) {
                    cs.push(n);
                }
            }
        }
        self.vars[v].domain = Some(sub_consts.clone());
        Ok(sub_consts)
    }

    fn domain_syms(&mut self, v: VarId) -> Result<Option<Vec<SymId>>, Unsupported> {
        if let Some(d) = &self.vars[v].domain_syms {
            return Ok(d.clone());
        }
        let d = self.domain(v)?.map(|cs| {
            let mut out = Vec::new();
            for c in cs {
                if let Some((_, s)) = self.vars[v].sym_for.iter().find(|(k, _)| *k == c) {
                    if !out.contains(s) {
                        out.push(*s);
                    }
                }
            }
            out
        });
        self.vars[v].domain_syms = Some(d.clone());
        Ok(d)
    }

    fn sym_for_static(&mut self, v: VarId) -> Option<SymId> {
        if let Some(s) = self.vars[v].sym_static {
            return s;
        }
        let checkable = self.vars[v].checkable.clone();
        let c = self.type_const(&checkable);
        let s = self.vars[v].sym_for.iter().find(|(k, _)| *k == c).map(|(_, s)| *s);
        self.vars[v].sym_static = Some(s);
        s
    }

    /// `implies(lower, upper)`
    fn implies(&self, lower: ConstId, upper: ConstId) -> Result<bool, Unsupported> {
        if lower == upper {
            return Ok(true);
        }
        let l = &self.consts[lower];
        let u = &self.consts[upper];
        if l.is_null || u.is_value {
            return Ok(false);
        }
        let ltp = if l.is_value { &l.wide } else { &l.tp };
        self.tys().instance_of_implies(ltp, &u.tp).ok_or_else(|| bail!())
    }

    /// `excludes(a, b)`
    fn excludes(&self, domain: &Option<Vec<ConstId>>, a: ConstId, b: ConstId) -> bool {
        let both = domain.as_ref().is_some_and(|d| d.contains(&a) && d.contains(&b));
        let either_null = self.consts[a].is_null || self.consts[b].is_null;
        let both_values = self.consts[a].is_value && self.consts[b].is_value;
        both && (either_null || both_values) && a != b
    }

    fn implications(&mut self, v: VarId) -> Result<Vec<(SymId, Vec<SymId>, Vec<SymId>)>, Unsupported> {
        if let Some(i) = &self.vars[v].implications {
            return Ok(i.clone());
        }
        let domain = self.domain(v)?;
        let mut eq_syms: Vec<SymId> = self.vars[v].sym_for.iter().map(|(_, s)| *s).collect();
        eq_syms.sort_by_key(|s| self.sym_string(*s));
        let mut excluded_pairs: Vec<(ConstId, ConstId)> = Vec::new();
        let mut out = Vec::new();
        for &sym in &eq_syms {
            let sc = self.syms[sym].konst;
            let todo: Vec<SymId> = eq_syms
                .iter()
                .copied()
                .filter(|b| {
                    let bc = self.syms[*b].konst;
                    !(bc == sc || excluded_pairs.iter().any(|(x, y)| (*x == bc && *y == sc) || (*x == sc && *y == bc)))
                })
                .collect();
            let (excluded, not_excluded): (Vec<SymId>, Vec<SymId>) =
                todo.into_iter().partition(|b| self.excludes(&domain, sc, self.syms[*b].konst));
            let mut implied = Vec::new();
            for b in not_excluded {
                if self.implies(sc, self.syms[b].konst)? {
                    implied.push(b);
                }
            }
            for e in &excluded {
                excluded_pairs.push((sc, self.syms[*e].konst));
            }
            out.push((sym, implied, excluded));
        }
        self.vars[v].implications = Some(out.clone());
        Ok(out)
    }

    fn grouped_domains(&mut self, v: VarId) -> Result<Vec<Vec<SymId>>, Unsupported> {
        if let Some(g) = &self.vars[v].grouped {
            return Ok(g.clone());
        }
        let static_tp = self.vars[v].static_tp.clone();
        let subtypes = self.enumerate_subtypes(&static_tp, true)?;
        let mut out = Vec::new();
        for sub in subtypes {
            let mut syms = Vec::new();
            for tpe in sub {
                let c = self.type_const(&tpe);
                if let Some((_, s)) = self.vars[v].sym_for.iter().find(|(k, _)| *k == c) {
                    if !syms.contains(s) {
                        syms.push(*s);
                    }
                }
            }
            if self.vars[v].may_be_null {
                let n = self.null_const;
                if let Some((_, s)) = self.vars[v].sym_for.iter().find(|(k, _)| *k == n) {
                    if !syms.contains(s) {
                        syms.push(*s);
                    }
                }
            }
            if !syms.is_empty() {
                out.push(syms);
            }
        }
        self.vars[v].grouped = Some(out.clone());
        Ok(out)
    }

    /// `removeVarEq`
    fn remove_var_eq(&mut self, props: &[Prop], model_null: bool) -> Result<(Prop, Vec<Prop>), Unsupported> {
        let mut vars: Vec<VarId> = Vec::new();
        for p in props {
            self.gather_equalities(p, model_null, &mut vars);
        }
        if model_null {
            for &v in &vars {
                self.register_null(v);
            }
        }
        let pure: Vec<Prop> = props.iter().map(|p| self.rewrite_equals(p)).collect();
        let mut axioms: Vec<Prop> = Vec::new();
        for (i, &v) in vars.iter().enumerate() {
            let is_scrutinee = i == 0;
            if let Some(dsyms) = self.domain_syms(v)? {
                if is_scrutinee || !dsyms.is_empty() {
                    axioms.push(big_or(dsyms.iter().map(|s| Prop::Sym(*s)).collect()));
                }
            }
            if let Some(s) = self.sym_for_static(v) {
                if self.vars[v].may_be_null {
                    let n = self.null_const;
                    axioms.push(or_create([self.prop_for_equals_to(v, n), Prop::Sym(s)]));
                } else {
                    axioms.push(Prop::Sym(s));
                }
            }
            let imps = self.implications(v)?;
            let groups = self.grouped_domains(v)?;
            for (sym, implied, excluded) in imps {
                for i in implied {
                    axioms.push(or_create([Prop::Not(Box::new(Prop::Sym(sym))), Prop::Sym(i)]));
                }
                for e in excluded {
                    let exclusive = groups.iter().any(|d| d.contains(&sym) && d.contains(&e));
                    if !exclusive {
                        axioms.push(or_create([
                            Prop::Not(Box::new(Prop::Sym(sym))),
                            Prop::Not(Box::new(Prop::Sym(e))),
                        ]));
                    }
                }
            }
            for g in groups {
                if g.len() > 1 {
                    axioms.push(Prop::AtMostOne(g));
                }
            }
        }
        Ok((and_create(axioms), pure))
    }

    fn gather_equalities(&mut self, p: &Prop, model_null: bool, vars: &mut Vec<VarId>) {
        match p {
            Prop::Eq(v, c) => {
                if !vars.contains(v) {
                    vars.push(*v);
                }
                if !(*c == self.null_const && !model_null) {
                    self.register_equality(*v, *c);
                }
            }
            Prop::And(ops) | Prop::Or(ops) => {
                for o in ops {
                    self.gather_equalities(o, model_null, vars);
                }
            }
            Prop::Not(a) => self.gather_equalities(a, model_null, vars),
            _ => {}
        }
    }

    fn rewrite_equals(&self, p: &Prop) -> Prop {
        match p {
            Prop::Eq(v, c) => self.prop_for_equals_to(*v, *c),
            Prop::And(ops) => and_create(ops.iter().map(|o| self.rewrite_equals(o))),
            Prop::Or(ops) => or_create(ops.iter().map(|o| self.rewrite_equals(o))),
            Prop::Not(a) => Prop::Not(Box::new(self.rewrite_equals(a))),
            other => other.clone(),
        }
    }

    // --- the approximation (`TreeMakerToProp.apply`) -------------------------

    fn eq_prop(&mut self, path: PTree, pat: &PatVal) -> Prop {
        let key = (path.clone(), pat.key.clone());
        if let Some(p) = self.eq_props.get(&key) {
            return p.clone();
        }
        let pat = self.pat_trees.entry(pat.key.clone()).or_insert_with(|| pat.clone()).clone();
        let v = self.var(&path);
        let c = self.value_const(&pat);
        let p = Prop::Eq(v, c);
        self.eq_props.insert(key, p.clone());
        p
    }

    fn nonnull_prop(&mut self, path: PTree) -> Prop {
        if let Some(p) = self.nonnull_props.get(&path) {
            return p.clone();
        }
        let v = self.var(&path);
        let p = Prop::Not(Box::new(Prop::Eq(v, self.null_const)));
        self.nonnull_props.insert(path, p.clone());
        p
    }

    fn type_prop(&mut self, path: PTree, pt: &NTy) -> Prop {
        let key = (path.clone(), pt.clone());
        if let Some(p) = self.type_props.get(&key) {
            return p.clone();
        }
        let v = self.var(&path);
        let checkable = self.tys().checkable(pt);
        let c = self.type_const(&checkable);
        let p = Prop::Eq(v, c);
        self.type_props.insert(key, p.clone());
        p
    }

    fn to_prop(&mut self, m: &Maker, unknown: Unknown) -> Result<Prop, Unsupported> {
        if !self.computed {
            self.update_substitution(m);
        }
        Ok(match &m.tm {
            TM::TypeTest { tested, expected, extractor_arg, .. } => {
                self.render_type_test(*tested, expected, *extractor_arg)?
            }
            TM::EqTest { prev, pat, .. } => {
                let p = self.binder_tree(*prev);
                self.eq_prop(p, pat)
            }
            TM::Alts { alts, .. } => {
                let mut ors = Vec::new();
                for alt in alts {
                    let mut ands = Vec::new();
                    for a in alt {
                        ands.push(self.to_prop(a, unknown)?);
                    }
                    ors.push(big_and(ands));
                }
                big_or(ors)
            }
            TM::Product { prev, has_extra: false, .. } => {
                let p = self.binder_tree(*prev);
                self.nonnull_prop(p)
            }
            TM::SubstOnly { .. } => Prop::True,
            TM::NonNull { prev } => {
                let p = self.binder_tree(*prev);
                self.nonnull_prop(p)
            }
            TM::Guard { constant: Some(true) } => Prop::True,
            TM::Guard { constant: Some(false) } => Prop::False,
            _ => self.handle_unknown(m, unknown),
        })
    }

    fn handle_unknown(&mut self, m: &Maker, unknown: Unknown) -> Prop {
        // `irrefutableExtractor`
        if let TM::Extractor { ext, has_extra: false, .. } = &m.tm {
            if ext.irrefutable {
                return Prop::True;
            }
        }
        match unknown {
            Unknown::Reach(default) => {
                if default {
                    Prop::True
                } else {
                    Prop::False
                }
            }
            Unknown::Exhaust => {
                // `rewriteListPattern`: `List()` as `Nil`.
                if let TM::Extractor { ext, checked_length: Some(0), prev, .. } = &m.tm {
                    if ext.seq_wrapper {
                        let nil = self.t.st.nil_sym;
                        let nil_cls = if nil.is_none() { nil } else { self.t.st.module_class_of(nil) };
                        let pv = PatVal {
                            key: PatKey::Sym(nil, None),
                            tp: NTy::Module(nil_cls),
                            stable: true,
                            text: "Nil".into(),
                            switch_const: None,
                        };
                        let p = self.binder_tree(*prev);
                        return self.eq_prop(p, &pv);
                    }
                }
                match &m.tm {
                    TM::Body { .. } => Prop::True,
                    TM::Extractor { .. } | TM::Product { .. } | TM::Guard { .. } => Prop::False,
                    _ => {
                        self.backoff = true;
                        Prop::False
                    }
                }
            }
        }
    }

    /// `TypeTestTreeMaker.renderCondition(condStrategy)`
    fn render_type_test(&mut self, tested: B, expected: &NTy, extractor_arg: bool) -> Result<Prop, Unsupported> {
        let tys = self.tys();
        let tested_wide = tys.widen(&self.binders[tested].tp);
        if tested_wide.is_unknown() || expected.is_unknown() {
            return Err(bail!());
        }
        let is_as_expected = tys.sub(&tested_wide, expected).ok_or_else(|| bail!())?;
        let is_prim = is_as_expected && tys.is_primitive_value_type(expected);
        let is_ref = is_as_expected && tys.sub(expected, &NTy::AnyRef).ok_or_else(|| bail!())?;
        if !extractor_arg && expected.is_singleton() {
            // `case _: x.type` and friends.
            return Err(bail!());
        }
        let expected_wide = tys.widen(expected);
        Ok(if is_prim {
            Prop::True
        } else if is_ref {
            let p = self.binder_tree(tested);
            self.nonnull_prop(p)
        } else {
            let p = self.binder_tree(tested);
            let nn = self.nonnull_prop(p.clone());
            let tp = self.type_prop(p, &expected_wide);
            and_create([nn, tp])
        })
    }

    fn approximate(&mut self, cases: &[Vec<Maker>], unknown: Unknown) -> Result<Vec<Vec<(Prop, bool)>>, Unsupported> {
        let mut out = Vec::new();
        for c in cases {
            let mut tests = Vec::new();
            for m in c {
                let p = self.to_prop(m, unknown)?;
                tests.push((p, matches!(m.tm, TM::Body { .. })));
            }
            out.push(tests);
        }
        self.computed = true;
        Ok(out)
    }

    fn case_without_body(tests: &[(Prop, bool)]) -> Prop {
        big_and(tests.iter().take_while(|(_, body)| !body).map(|(p, _)| p.clone()).collect())
    }

    // --- unreachability ---------------------------------------------------------

    /// `unreachableCase`: the index of the first case that cannot match.
    pub(crate) fn unreachable_case(&mut self, cases: &[Vec<Maker>]) -> Result<Option<usize>, Unsupported> {
        let ok: Vec<Prop> = self
            .approximate(cases, Unknown::Reach(true))?
            .iter()
            .map(|t| Self::case_without_body(t))
            .collect();
        let fail: Vec<Prop> = self
            .approximate(cases, Unknown::Reach(false))?
            .iter()
            .map(|t| Prop::Not(Box::new(Self::case_without_body(t))))
            .collect();
        let (ax_fail, sym_fail) = self.remove_var_eq(&fail, true)?;
        let (ax_ok, sym_ok) = self.remove_var_eq(&ok, true)?;
        let eq_axioms = simplify(&and_create([ax_ok, ax_fail]));
        let mut prefix = vec![eq_axioms];
        let mut prefix_rest: &[Prop] = &sym_fail;
        let mut current: &[Prop] = &sym_ok;
        let mut reachable = true;
        let mut case_index = 0;
        while !prefix_rest.is_empty() && reachable {
            let head = prefix_rest[0].clone();
            case_index += 1;
            prefix_rest = &prefix_rest[1..];
            if prefix_rest.is_empty() {
                reachable = true;
            } else {
                prefix.push(head);
                current = &current[1..];
                let mut ops = vec![current[0].clone()];
                ops.extend(prefix.iter().cloned());
                let and = and_create(ops);
                let solvable = eq_free_prop_to_solvable(&and).map_err(|_| bail!())?;
                reachable = has_model(&solvable);
            }
        }
        Ok(if reachable { None } else { Some(case_index) })
    }
}

/// The scrutinee's type is not one exhaustivity can say anything about
/// (`uncheckableType`).
fn uncheckable(a: &mut Approx, tp: &NTy) -> Result<bool, Unsupported> {
    if let NTy::Class(c, args) = tp {
        let st = &a.t.st;
        let name = &st.get(*c).name;
        if name.strip_prefix("Tuple").and_then(|n| n.parse::<usize>().ok()) == Some(args.len())
            && st.get(*c).jvm_name.starts_with("scala/Tuple")
        {
            for x in args.clone() {
                if !uncheckable(a, &x)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
    }
    Ok(a.enumerate_subtypes(tp, false)?.is_empty())
}

// ---------------------------------------------------------------------------
// Exhaustivity and counter-examples
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum CEx {
    Value(ConstId, String),
    Type(String),
    Negative(String, Vec<String>),
    List(Vec<CEx>),
    Tuple(Vec<CEx>),
    Ctor(String, bool, Vec<CEx>),
    Wildcard,
    No,
}

impl CEx {
    fn flatten_cons_args(&self) -> Vec<CEx> {
        match self {
            CEx::List(args) if args.len() == 2 => {
                let mut v = vec![args[0].clone()];
                v.extend(args[1].flatten_cons_args());
                v
            }
            _ => Vec::new(),
        }
    }

    fn elems(&self) -> Vec<CEx> {
        self.flatten_cons_args()
    }

    fn show(&self) -> String {
        match self {
            CEx::Value(_, s) => s.clone(),
            CEx::Type(c) => format!("(_ : {c})"),
            CEx::Negative(eq, not) => {
                let negation = if not.len() == 1 {
                    not[0].clone()
                } else {
                    let mut v = not.clone();
                    v.sort();
                    format!("({})", v.join(", "))
                };
                format!("(x: {eq} forSome x not in {negation})")
            }
            CEx::List(_) => format!(
                "List({})",
                self.elems().iter().map(|e| e.show()).collect::<Vec<_>>().join(", ")
            ),
            CEx::Tuple(args) => format!(
                "({})",
                args.iter().map(|e| e.show()).collect::<Vec<_>>().join(", ")
            ),
            CEx::Ctor(name, module, args) => {
                if *module {
                    name.clone()
                } else {
                    format!(
                        "{name}({})",
                        args.iter().map(|e| e.show()).collect::<Vec<_>>().join(", ")
                    )
                }
            }
            CEx::Wildcard => "_".into(),
            CEx::No => "??".into(),
        }
    }

    fn covered_by(&self, other: &CEx) -> bool {
        match (self, other) {
            (CEx::List(_), CEx::List(_)) => {
                let (a, b) = (self.elems(), other.elems());
                self == other || (a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x.covered_by(y)))
            }
            (CEx::Tuple(a), CEx::Tuple(b)) => {
                self == other || (a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.covered_by(y)))
            }
            _ => self == other || *other == CEx::Wildcard,
        }
    }
}

type VarAssignment = Vec<(VarId, (Vec<ConstId>, Vec<ConstId>))>;

impl<'x> Approx<'x> {
    /// `exhaustive`: the counter-examples, `Ok(None)` when the analysis
    /// backs off or the scrutinee is uncheckable.
    pub(crate) fn exhaustive(&mut self, cases: &[Vec<Maker>]) -> Result<(Vec<String>, bool), Unsupported> {
        let root_tp = self.binders[self.root].tp.clone();
        if uncheckable(self, &root_tp)? {
            return Ok((Vec::new(), false));
        }
        let symbolic: Vec<Prop> = self
            .approximate(cases, Unknown::Exhaust)?
            .iter()
            .map(|t| Self::case_without_body(t))
            .collect();
        if self.backoff {
            return Ok((Vec::new(), false));
        }
        let root = self.root;
        let root_tree = self.binder_tree(root);
        let match_fails = Prop::Not(Box::new(big_or(symbolic)));
        let (ax, pure) = self.remove_var_eq(&[match_fails], false)?;
        let pure = pure.into_iter().next().unwrap_or(Prop::True);
        let solvable = eq_free_prop_to_solvable(&and_create([ax, pure])).map_err(|_| bail!())?;
        let (models, depth_reached) = find_all_models(&solvable);
        let scrut_var = self.var(&root_tree);
        // The classes a counter-example may be built from, with their pickled
        // flags at hand (`to_counter_example` only reads them).
        let mut classes: Vec<SymbolId> = Vec::new();
        for v in &self.vars {
            if let Some(c) = self.tys().type_symbol(&v.checkable) {
                classes.push(c);
            }
        }
        for c in &self.consts {
            if let Some(s) = self.tys().type_symbol(&c.tp) {
                classes.push(s);
            }
        }
        for c in classes {
            self.class_flags(c);
        }
        let mut examples: Vec<CEx> = Vec::new();
        'models: for model in &models {
            for va in self.expand_model(model) {
                if let Some(ce) = self.model_to_counter_example(scrut_var, &va)? {
                    examples.push(ce);
                    if examples.len() >= MAX_DPLL_DEPTH {
                        break 'models;
                    }
                }
            }
        }
        let mut sorted: Vec<(String, CEx)> = examples.into_iter().map(|e| (e.show(), e)).collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let mut result: Vec<CEx> = Vec::new();
        for (_, ex) in sorted {
            if !result.iter().any(|r| ex.covered_by(r)) {
                result.push(ex);
            }
        }
        let mut out: Vec<String> = Vec::new();
        for r in result {
            let s = r.show();
            if !out.contains(&s) {
                out.push(s);
            }
        }
        Ok((out, depth_reached))
    }

    fn var_string(v: VarId) -> String {
        format!("V{}", v + 1)
    }

    /// `modelToVarAssignment`
    fn var_assignment(&self, model: &[(SymId, bool)]) -> VarAssignment {
        let mut out: VarAssignment = Vec::new();
        for (s, b) in model {
            let si = &self.syms[*s];
            let e = match out.iter().position(|(v, _)| *v == si.var) {
                Some(i) => &mut out[i].1,
                None => {
                    out.push((si.var, (Vec::new(), Vec::new())));
                    &mut out.last_mut().unwrap().1
                }
            };
            if *b {
                e.0.push(si.konst);
            } else {
                e.1.push(si.konst);
            }
        }
        out
    }

    /// `expandModel`
    fn expand_model(&self, solution: &Solution) -> Vec<VarAssignment> {
        let va = self.var_assignment(&solution.model);
        let mut grouped: Vec<(VarId, Vec<SymId>)> = Vec::new();
        for s in &solution.unassigned {
            let v = self.syms[*s].var;
            match grouped.iter_mut().find(|(g, _)| *g == v) {
                Some((_, l)) => l.push(*s),
                None => grouped.push((v, vec![*s])),
            }
        }
        grouped.sort_by_key(|(v, _)| Self::var_string(*v));
        let mut expanded: Vec<Vec<VarAssignment>> = Vec::new();
        for (variable, syms) in &grouped {
            let (equal, not_equal) = va
                .iter()
                .find(|(v, _)| v == variable)
                .map(|(_, e)| e.clone())
                .unwrap_or_default();
            let add = |eq_to: Vec<ConstId>, neq_to: Vec<ConstId>| -> VarAssignment {
                let mut e = equal.clone();
                e.extend(eq_to);
                let mut n = not_equal.clone();
                n.extend(neq_to);
                vec![(*variable, (e, n))]
            };
            let consts: Vec<ConstId> = syms.iter().map(|s| self.syms[*s].konst).collect();
            let all_not_equal = add(Vec::new(), consts.clone());
            let all_equal = add(consts.clone(), Vec::new());
            let mut opts = vec![all_equal, all_not_equal];
            if equal.is_empty() {
                for s in syms {
                    let others: Vec<ConstId> = syms
                        .iter()
                        .filter(|o| *o != s)
                        .map(|o| self.syms[*o].konst)
                        .collect();
                    opts.push(add(vec![self.syms[*s].konst], others));
                }
            }
            expanded.push(opts);
        }
        let merge = |a: &VarAssignment, b: &VarAssignment| -> VarAssignment {
            let mut out = a.clone();
            for (v, e) in b {
                match out.iter_mut().find(|(k, _)| k == v) {
                    Some(slot) => slot.1 = e.clone(),
                    None => out.push((*v, e.clone())),
                }
            }
            out
        };
        if expanded.is_empty() {
            return vec![va];
        }
        let mut acc: Vec<VarAssignment> = expanded[0].clone();
        for vs in &expanded[1..] {
            if acc.len() > MAX_DPLL_DEPTH {
                acc.truncate(MAX_DPLL_DEPTH);
                break;
            }
            let mut next = Vec::new();
            for m1 in &acc {
                for m2 in vs {
                    next.push(merge(m1, m2));
                }
            }
            acc = next;
        }
        if acc.len() > MAX_DPLL_DEPTH {
            acc.truncate(MAX_DPLL_DEPTH);
        }
        acc.iter().map(|m| merge(&va, m)).collect()
    }

    fn chop(&self, p: &PTree) -> Vec<ChopSym> {
        match p {
            PTree::Ref(b) => vec![ChopSym::Binder(*b)],
            PTree::Sel(q, f) => {
                let mut v = self.chop(q);
                v.push(ChopSym::Field(f.clone()));
                v
            }
        }
    }

    /// `modelToCounterExample`
    fn model_to_counter_example(&mut self, scrut: VarId, va: &VarAssignment) -> Result<Option<CEx>, Unsupported> {
        let mut nodes: Vec<VaNode> = Vec::new();
        let mut uniques: HashMap<VarId, usize> = HashMap::new();
        let mut keys: Vec<VarId> = va.iter().map(|(v, _)| *v).collect();
        keys.sort_by_key(|v| Self::var_string(*v));
        for v in keys {
            if v != scrut {
                self.va_apply(v, scrut, va, &mut nodes, &mut uniques);
            }
        }
        let root = self.va_apply(scrut, scrut, va, &mut nodes, &mut uniques);
        self.to_counter_example(root, false, &nodes)
    }

    fn va_unique(&self, v: VarId, va: &VarAssignment, nodes: &mut Vec<VaNode>, uniques: &mut HashMap<VarId, usize>) -> usize {
        if let Some(n) = uniques.get(&v) {
            return *n;
        }
        let (eq, neq) = va.iter().find(|(k, _)| *k == v).map(|(_, e)| e.clone()).unwrap_or_default();
        nodes.push(VaNode {
            var: v,
            equal_to: eq,
            not_equal_to: neq,
            fields: Vec::new(),
        });
        let n = nodes.len() - 1;
        uniques.insert(v, n);
        n
    }

    fn va_apply(&self, v: VarId, scrut: VarId, va: &VarAssignment, nodes: &mut Vec<VaNode>, uniques: &mut HashMap<VarId, usize>) -> usize {
        let path = self.chop(&self.vars[v].path);
        let new_ctor = self.va_unique(v, va, nodes, uniques);
        if path.len() <= 1 {
            return new_ctor;
        }
        let pre = &path[..path.len() - 1];
        let field = path.last().unwrap().clone();
        // `findVar(pre)`
        let scrut_path = self.chop(&self.vars[scrut].path);
        let pre_var = if pre.len() == 1 && scrut_path.len() == 1 && pre[0] == scrut_path[0] {
            Some(scrut)
        } else {
            va.iter().map(|(k, _)| *k).find(|k| self.chop(&self.vars[*k].path) == pre)
        };
        if let Some(pv) = pre_var {
            let outer = self.va_apply(pv, scrut, va, nodes, uniques);
            // `addField`: a case accessor only counts on the class that has it.
            let should = match &field {
                ChopSym::Field(Field::Acc(acc)) => {
                    let cls = self.node_class(&nodes[outer]);
                    cls.is_some_and(|c| self.t.st.get(c).ctor_fields.contains(acc))
                }
                _ => true,
            };
            if should {
                let node = &mut nodes[outer];
                match node.fields.iter_mut().find(|(f, _)| *f == field) {
                    Some(slot) => slot.1 = new_ctor,
                    None => node.fields.push((field, new_ctor)),
                }
            }
        }
        new_ctor
    }

    fn unique_equal_to(&self, n: &VaNode) -> Vec<ConstId> {
        let tys = self.tys();
        n.equal_to
            .iter()
            .copied()
            .filter(|&subsumed| {
                !n.equal_to.iter().any(|&better| {
                    better != subsumed
                        && tys
                            .instance_of_implies(&self.consts[better].tp, &self.consts[subsumed].tp)
                            .unwrap_or(false)
                })
            })
            .collect()
    }

    fn pruned_equal_to(&self, n: &VaNode) -> Vec<ConstId> {
        let tys = self.tys();
        let checkable = &self.vars[n.var].checkable;
        self.unique_equal_to(n)
            .into_iter()
            .filter(|&c| tys.sub(checkable, &self.consts[c].tp) != Some(true))
            .collect()
    }

    /// `ctor.safeOwner`: the class whose primary constructor builds the
    /// value, or `None` (nsc's `NoSymbol`) for a trait or a type with none.
    fn node_class(&self, n: &VaNode) -> Option<SymbolId> {
        let pruned = self.pruned_equal_to(n);
        let tp = match pruned.as_slice() {
            [c] if self.consts[*c].is_type => self.consts[*c].tp.clone(),
            _ => self.vars[n.var].checkable.clone(),
        };
        let sym = self.tys().type_symbol(&tp)?;
        let s = self.t.st.get(sym);
        match s.kind {
            SymKind::Class
                if !s.flags.contains(Flags::TRAIT)
                    && !s.flags.contains(Flags::INTERFACE)
                    && self.pickled_cache.get(&sym).is_none_or(|p| {
                        p & (scala_rs_pickle::read::pflags::TRAIT | scala_rs_pickle::read::pflags::INTERFACE) == 0
                    }) =>
            {
                if sym == self.t.st.any_sym || sym == self.t.st.anyval_sym {
                    None
                } else {
                    Some(sym)
                }
            }
            SymKind::ModuleClass => Some(sym),
            _ => None,
        }
    }

    fn to_counter_example(&self, idx: usize, brief: bool, nodes: &[VaNode]) -> Result<Option<CEx>, Unsupported> {
        let n = &nodes[idx];
        let cls = self.node_class(n);
        let case_accs: Vec<SymbolId> = cls.map(|c| self.t.st.get(c).ctor_fields.clone()).unwrap_or_default();
        // `allFieldAssignmentsLegal`
        if !self.all_fields_legal(idx, nodes) {
            return Ok(Some(CEx::No));
        }
        let unique_eq = self.unique_equal_to(n);
        let pruned = self.pruned_equal_to(n);
        let st = &self.t.st;
        if let [c] = pruned.as_slice() {
            if n.fields.is_empty() && self.consts[*c].is_value && !self.consts[*c].is_null {
                return Ok(Some(CEx::Value(*c, self.consts[*c].text.clone())));
            }
            if n.fields.is_empty() && self.consts[*c].is_null {
                return Ok(Some(CEx::Value(*c, "null".into())));
            }
        }
        if let Some(c) = cls {
            let primitive = st.is_primitive_value_class(c);
            if !primitive
                && (!unique_eq.is_empty()
                    || (!n.fields.is_empty() && pruned.is_empty() && n.not_equal_to.is_empty()))
            {
                let arg_len = case_accs.len();
                let args = |brevity: bool| -> Result<Option<Vec<CEx>>, Unsupported> {
                    let mut out = Vec::new();
                    for acc in case_accs.iter().take(arg_len) {
                        let f = ChopSym::Field(Field::Acc(*acc));
                        match n.fields.iter().find(|(k, _)| *k == f) {
                            Some((_, child)) => match self.to_counter_example(*child, brevity, nodes)? {
                                Some(e) => out.push(e),
                                None => return Ok(None),
                            },
                            None => out.push(CEx::Wildcard),
                        }
                    }
                    Ok(Some(out))
                };
                let s = st.get(c);
                let is_cons = c == st.cons_sym;
                let is_tuple = s.name.strip_prefix("Tuple").and_then(|x| x.parse::<usize>().ok()).is_some()
                    && s.jvm_name.starts_with("scala/Tuple");
                if is_cons {
                    return Ok(args(brief)?.map(|a| {
                        let a = match a.as_slice() {
                            [CEx::No, l @ CEx::List(_)] => vec![CEx::Wildcard, l.clone()],
                            _ => a,
                        };
                        CEx::List(a)
                    }));
                }
                if is_tuple {
                    return Ok(args(true)?.map(CEx::Tuple));
                }
                let pickled = self.pickled_cache.get(&c).copied().unwrap_or(0);
                {
                    use scala_rs_pickle::read::pflags;
                    let sealed = s.flags.contains(Flags::SEALED) || pickled & pflags::SEALED != 0;
                    let abstract_ = s.flags.contains(Flags::ABSTRACT)
                        || s.flags.contains(Flags::TRAIT)
                        || s.flags.contains(Flags::ENUM)
                        || pickled & (pflags::ABSTRACT | pflags::TRAIT) != 0;
                    if sealed && abstract_ {
                        return Ok(None);
                    }
                }
                let module = s.kind == SymKind::ModuleClass;
                let name = s.name.trim_end_matches('$').to_string();
                return Ok(args(brief)?.map(|a| CEx::Ctor(name, module, a)));
            }
        }
        if let [c] = pruned.as_slice() {
            if n.fields.is_empty() {
                return Ok(Some(CEx::Type(self.consts[*c].text.clone())));
            }
        }
        let non_trivial: Vec<ConstId> = n
            .not_equal_to
            .iter()
            .copied()
            .filter(|c| self.consts[*c].wide != NTy::Any)
            .collect();
        if pruned.is_empty() && !non_trivial.is_empty() {
            if brief {
                return Ok(Some(CEx::Wildcard));
            }
            let eq_to = match n.equal_to.first() {
                Some(c) => self.consts[*c].text.clone(),
                None => self.tys().show(&self.vars[n.var].checkable),
            };
            return Ok(Some(CEx::Negative(
                eq_to,
                non_trivial.iter().map(|c| self.consts[*c].text.clone()).collect(),
            )));
        }
        // `inSameDomain` (strict mode reports a wildcard either way).
        let domain_syms = self.vars[n.var].domain_syms.clone().flatten();
        let in_same_domain = unique_eq.iter().all(|c| {
            domain_syms
                .as_ref()
                .is_some_and(|d| d.iter().any(|s| self.consts[self.syms[*s].konst].tp == self.consts[*c].tp))
        });
        if in_same_domain {
            return Ok(Some(CEx::Wildcard));
        }
        Ok(Some(CEx::No))
    }

    fn all_fields_legal(&self, idx: usize, nodes: &[VaNode]) -> bool {
        let n = &nodes[idx];
        let accs: Vec<SymbolId> = self
            .node_class(n)
            .map(|c| self.t.st.get(c).ctor_fields.clone())
            .unwrap_or_default();
        n.fields.iter().all(|(f, child)| {
            matches!(f, ChopSym::Field(Field::Acc(a)) if accs.contains(a)) && self.all_fields_legal(*child, nodes)
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ChopSym {
    Binder(B),
    Field(Field),
}

struct VaNode {
    var: VarId,
    equal_to: Vec<ConstId>,
    not_equal_to: Vec<ConstId>,
    fields: Vec<(ChopSym, usize)>,
}
