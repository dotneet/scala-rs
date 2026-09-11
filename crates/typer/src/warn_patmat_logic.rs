//! nsc's `patmat/Logic.scala` and `patmat/Solving.scala`: propositions over
//! "variable = constant" atoms, their reduction to pure boolean logic with the
//! equality axioms (`removeVarEq`), the CNF conversion (Plaisted, with the
//! already-in-CNF shortcut), and the DPLL solver that enumerates models.
//!
//! The port keeps nsc's orders wherever they reach the output: operands are
//! insertion-ordered sets (`LogicLinkedHashSet`), clauses are Scala immutable
//! sets (`scala_coll::ScalaSet`, whose `head` the solver branches on), the
//! equality symbols of a variable are sorted by their `toString`, and symbol
//! ids come from one counter for the whole run, as nsc's `Sym.nextSymId` does.
//! The model the solver finds, and therefore the counter-examples reported,
//! depend on every one of these.

use crate::scala_coll::{ScalaHash, ScalaSet};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub(crate) type VarId = usize;
pub(crate) type ConstId = usize;
pub(crate) type SymId = usize;

/// A proposition. `And`/`Or` operands are insertion-ordered sets; equality
/// between two of them is set equality, as for nsc's `LinkedHashSet`s.
#[derive(Clone, Debug)]
pub(crate) enum Prop {
    Eq(VarId, ConstId),
    And(Vec<Prop>),
    Or(Vec<Prop>),
    Not(Box<Prop>),
    AtMostOne(Vec<SymId>),
    True,
    False,
    Sym(SymId),
}

impl PartialEq for Prop {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Prop::Eq(a, b), Prop::Eq(c, d)) => a == c && b == d,
            (Prop::And(a), Prop::And(b)) | (Prop::Or(a), Prop::Or(b)) => {
                a.len() == b.len() && a.iter().all(|x| b.contains(x))
            }
            (Prop::Not(a), Prop::Not(b)) => a == b,
            (Prop::AtMostOne(a), Prop::AtMostOne(b)) => a == b,
            (Prop::True, Prop::True) | (Prop::False, Prop::False) => true,
            (Prop::Sym(a), Prop::Sym(b)) => a == b,
            _ => false,
        }
    }
}
impl Eq for Prop {}

impl Hash for Prop {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Prop::Eq(a, b) => {
                0u8.hash(state);
                a.hash(state);
                b.hash(state);
            }
            Prop::And(ops) | Prop::Or(ops) => {
                (if matches!(self, Prop::And(_)) { 1u8 } else { 2u8 }).hash(state);
                // Order-independent, like a set's hash.
                let mut acc: u64 = 0;
                for o in ops {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    o.hash(&mut h);
                    acc = acc.wrapping_add(h.finish());
                }
                acc.hash(state);
            }
            Prop::Not(a) => {
                3u8.hash(state);
                a.hash(state);
            }
            Prop::AtMostOne(v) => {
                4u8.hash(state);
                v.hash(state);
            }
            Prop::True => 5u8.hash(state),
            Prop::False => 6u8.hash(state),
            Prop::Sym(s) => {
                7u8.hash(state);
                s.hash(state);
            }
        }
    }
}

/// Insert into an insertion-ordered set.
fn add_unique(v: &mut Vec<Prop>, p: Prop) {
    if !v.contains(&p) {
        v.push(p);
    }
}

pub(crate) fn and_create(ps: impl IntoIterator<Item = Prop>) -> Prop {
    let mut v = Vec::new();
    for p in ps {
        add_unique(&mut v, p);
    }
    Prop::And(v)
}

pub(crate) fn or_create(ps: impl IntoIterator<Item = Prop>) -> Prop {
    let mut v = Vec::new();
    for p in ps {
        add_unique(&mut v, p);
    }
    Prop::Or(v)
}

/// nsc `/\`: `True` for none, the operand itself for one.
pub(crate) fn big_and(ps: Vec<Prop>) -> Prop {
    let mut v = Vec::new();
    for p in ps {
        add_unique(&mut v, p);
    }
    match v.len() {
        0 => Prop::True,
        1 => v.pop().unwrap(),
        _ => Prop::And(v),
    }
}

/// nsc `\/`.
pub(crate) fn big_or(ps: Vec<Prop>) -> Prop {
    let mut v = Vec::new();
    for p in ps {
        add_unique(&mut v, p);
    }
    match v.len() {
        0 => Prop::False,
        1 => v.pop().unwrap(),
        _ => Prop::Or(v),
    }
}

/// `PropositionalLogic.simplify`: negation normal form, flattening,
/// constant folding, duplicate removal.
pub(crate) fn simplify(f: &Prop) -> Prop {
    let nnf = nnf(f);
    simplify_prop(&nnf)
}

fn nnf_not(p: &Prop) -> Prop {
    match p {
        Prop::And(ops) => Prop::Or(dedup(ops.iter().map(nnf_not))),
        Prop::Or(ops) => Prop::And(dedup(ops.iter().map(nnf_not))),
        Prop::Not(a) => nnf(a),
        Prop::True => Prop::False,
        Prop::False => Prop::True,
        other => Prop::Not(Box::new(other.clone())),
    }
}

fn nnf(p: &Prop) -> Prop {
    match p {
        Prop::And(ops) => Prop::And(dedup(ops.iter().map(nnf))),
        Prop::Or(ops) => Prop::Or(dedup(ops.iter().map(nnf))),
        Prop::Not(a) => nnf_not(a),
        other => other.clone(),
    }
}

/// `mapConserve` into a fresh `LinkedHashSet`: equal results collapse.
fn dedup(it: impl Iterator<Item = Prop>) -> Vec<Prop> {
    let mut v = Vec::new();
    for p in it {
        add_unique(&mut v, p);
    }
    v
}

fn has_impure_atom(ops: &[Prop]) -> bool {
    let check = |a: &Prop, b: &Prop| -> bool {
        match b {
            Prop::Not(bb) if **bb == *a => true,
            _ => matches!(a, Prop::Not(aa) if **aa == *b),
        }
    };
    let n = ops.len();
    if n > 10 || n < 2 {
        return false;
    }
    for i in 0..n {
        for j in i + 1..n {
            if check(&ops[i], &ops[j]) {
                return true;
            }
        }
    }
    false
}

fn simplify_and(ps: &[Prop]) -> Prop {
    let mut props = Vec::new();
    for p in ps {
        match simplify_prop(p) {
            Prop::True => {}
            Prop::And(fv) => {
                for f in fv {
                    add_unique(&mut props, f);
                }
            }
            f => add_unique(&mut props, f),
        }
    }
    if props.contains(&Prop::False) || has_impure_atom(&props) {
        Prop::False
    } else {
        big_and(props)
    }
}

fn simplify_or(ps: &[Prop]) -> Prop {
    let mut props = Vec::new();
    for p in ps {
        match simplify_prop(p) {
            Prop::False => {}
            Prop::Or(fv) => {
                for f in fv {
                    add_unique(&mut props, f);
                }
            }
            f => add_unique(&mut props, f),
        }
    }
    if props.contains(&Prop::True) || has_impure_atom(&props) {
        Prop::True
    } else {
        big_or(props)
    }
}

fn simplify_prop(p: &Prop) -> Prop {
    match p {
        Prop::And(ps) => simplify_and(ps),
        Prop::Or(ps) => simplify_or(ps),
        Prop::Not(inner) => match &**inner {
            Prop::Not(a) => simplify(a),
            other => Prop::Not(Box::new(simplify(other))),
        },
        other => other.clone(),
    }
}

/// Every symbol of `p`, in traversal order (`gatherSymbols`).
pub(crate) fn gather_symbols(p: &Prop, out: &mut Vec<SymId>) {
    match p {
        Prop::And(ops) | Prop::Or(ops) => {
            for o in ops {
                gather_symbols(o, out);
            }
        }
        Prop::Not(a) => gather_symbols(a, out),
        Prop::Sym(s) => {
            if !out.contains(s) {
                out.push(*s);
            }
        }
        Prop::AtMostOne(ops) => {
            for s in ops {
                if !out.contains(s) {
                    out.push(*s);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// CNF and DPLL (`Solving.scala`)
// ---------------------------------------------------------------------------

/// A literal: a (possibly negated) propositional variable, `Lit(v)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Lit(pub i32);

impl Lit {
    fn neg(self) -> Lit {
        Lit(-self.0)
    }
    fn variable(self) -> i32 {
        self.0.abs()
    }
    fn positive(self) -> bool {
        self.0 >= 0
    }
}

impl ScalaHash for Lit {
    fn scala_hash(&self) -> i32 {
        // `override def hashCode = v`
        self.0
    }
}

pub(crate) type Clause = ScalaSet<Lit>;

fn clause1(l: Lit) -> Clause {
    ScalaSet::empty().incl(l)
}

fn clause2(a: Lit, b: Lit) -> Clause {
    ScalaSet::empty().incl(a).incl(b)
}

/// The formula's symbols numbered from 1, in `gatherSymbols` order.
pub(crate) struct SymbolMapping {
    pub(crate) var_for_sym: HashMap<SymId, i32>,
    pub(crate) sym_for_var: HashMap<i32, SymId>,
    /// `relevantVars`, sorted (a `BitSet`).
    pub(crate) relevant: Vec<i32>,
}

impl SymbolMapping {
    fn new(symbols: &[SymId]) -> Self {
        let mut var_for_sym = HashMap::new();
        let mut sym_for_var = HashMap::new();
        for (i, s) in symbols.iter().enumerate() {
            var_for_sym.insert(*s, (i + 1) as i32);
            sym_for_var.insert((i + 1) as i32, *s);
        }
        let mut relevant: Vec<i32> = sym_for_var.keys().map(|v| v.abs()).collect();
        relevant.sort_unstable();
        relevant.dedup();
        SymbolMapping {
            var_for_sym,
            sym_for_var,
            relevant,
        }
    }
    fn lit(&self, s: SymId) -> Lit {
        Lit(self.var_for_sym[&s])
    }
    fn size(&self) -> i32 {
        self.var_for_sym.len() as i32
    }
}

pub(crate) struct Solvable {
    pub(crate) cnf: Vec<Clause>,
    pub(crate) mapping: std::rc::Rc<SymbolMapping>,
}

/// Raised when the formula exceeds nsc's `AnalysisBudget.maxFormulaSize`.
#[derive(Debug)]
pub(crate) struct BudgetExceeded;

/// `AnalysisBudget.maxDPLLdepth` (`-Ypatmat-exhaust-depth`, default 20).
pub(crate) const MAX_DPLL_DEPTH: usize = 20;
const MAX_FORMULA_SIZE: usize = 100 * MAX_DPLL_DEPTH;

struct TransformToCnf<'m> {
    mapping: &'m SymbolMapping,
    literal_count: i32,
    buff: Vec<Clause>,
    const_true: Option<Lit>,
}

impl<'m> TransformToCnf<'m> {
    fn new_literal(&mut self) -> Lit {
        self.literal_count += 1;
        Lit(self.literal_count)
    }
    fn const_true(&mut self) -> Lit {
        if let Some(l) = self.const_true {
            return l;
        }
        let l = self.new_literal();
        self.add_clause(clause1(l));
        self.const_true = Some(l);
        l
    }
    fn const_false(&mut self) -> Lit {
        self.const_true().neg()
    }
    fn add_clause(&mut self, c: Clause) {
        if !c.is_empty() {
            self.buff.push(c);
        }
    }
    fn convert(&mut self, p: &Prop) -> Option<Lit> {
        match p {
            Prop::And(fv) => {
                let lits = self.convert_all(fv);
                Some(self.and(lits))
            }
            Prop::Or(fv) => {
                let lits = self.convert_all(fv);
                Some(self.or(lits))
            }
            Prop::Not(a) => self.convert(a).map(|l| l.neg()),
            Prop::Sym(s) => Some(self.mapping.lit(*s)),
            Prop::True => Some(self.const_true()),
            Prop::False => Some(self.const_false()),
            Prop::AtMostOne(ops) => {
                self.at_most_one(ops);
                None
            }
            Prop::Eq(..) => None,
        }
    }
    /// `fv.flatMap(convert)` into a `LinkedHashSet[Lit]`.
    fn convert_all(&mut self, fv: &[Prop]) -> Vec<Lit> {
        let mut out: Vec<Lit> = Vec::new();
        for f in fv {
            if let Some(l) = self.convert(f) {
                if !out.contains(&l) {
                    out.push(l);
                }
            }
        }
        out
    }
    fn and(&mut self, bv: Vec<Lit>) -> Lit {
        if bv.is_empty() {
            return self.const_true();
        }
        if bv.len() == 1 {
            return bv[0];
        }
        let cf = self.const_false();
        if bv.contains(&cf) {
            return cf;
        }
        let ct = self.const_true();
        // `bv.toSet - constTrue`
        let new_bv = ScalaSet::from_iter(bv).excl(&ct);
        let o = self.new_literal();
        for op in new_bv.elems().to_vec() {
            self.add_clause(clause2(op, o.neg()));
        }
        o
    }
    fn or(&mut self, bv: Vec<Lit>) -> Lit {
        if bv.is_empty() {
            return self.const_false();
        }
        if bv.len() == 1 {
            return bv[0];
        }
        let ct = self.const_true();
        if bv.contains(&ct) {
            return ct;
        }
        let cf = self.const_false();
        let new_bv = ScalaSet::from_iter(bv).excl(&cf);
        let o = self.new_literal();
        self.add_clause(new_bv.incl(o.neg()));
        o
    }
    fn at_most_one(&mut self, ops: &[SymId]) {
        if ops.len() <= 1 {
            return;
        }
        if ops.len() > 5 {
            let x1 = self.mapping.lit(ops[0]);
            let tail = &ops[1..];
            let (mid, xn) = (&tail[..tail.len() - 1], tail[tail.len() - 1]);
            let s1 = self.new_literal();
            self.add_clause(clause2(x1.neg(), s1));
            let mut si_minus = s1;
            for &sym in mid {
                let xi = self.mapping.lit(sym);
                let si = self.new_literal();
                self.add_clause(clause2(xi.neg(), si));
                self.add_clause(clause2(si_minus.neg(), si));
                self.add_clause(clause2(xi.neg(), si_minus.neg()));
                si_minus = si;
            }
            let xn = self.mapping.lit(xn);
            self.add_clause(clause2(xn.neg(), si_minus.neg()));
        } else {
            let lits: Vec<Lit> = ops.iter().map(|s| self.mapping.lit(*s)).collect();
            // `combinations(2)` in order.
            for i in 0..lits.len() {
                for j in i + 1..lits.len() {
                    self.add_clause(clause2(lits[i].neg(), lits[j].neg()));
                }
            }
        }
    }
    /// `TransformToCnf.apply`: the clauses for `p` (`buildCnf` hands the
    /// buffer over and clears it; the literal counter and `constTrue` stay).
    fn apply(&mut self, p: &Prop) -> Vec<Clause> {
        let top = self.convert(p);
        // `addClauseProcessed(convert(p).toSet)`
        let c = match top {
            Some(l) => clause1(l),
            None => ScalaSet::empty(),
        };
        self.add_clause(c);
        std::mem::take(&mut self.buff)
    }
}

/// `AlreadyInCNF.ToLiteral`.
fn to_literal(m: &SymbolMapping, f: &Prop) -> Option<Lit> {
    match f {
        Prop::Not(inner) => to_literal(m, inner).map(|l| l.neg()),
        Prop::Sym(s) => Some(m.lit(*s)),
        _ => None,
    }
}

/// `AlreadyInCNF.ToDisjunction`: `None` when `f` is not a disjunction of
/// literals; `Some(clauses)` otherwise (`False` is the one empty clause).
fn to_disjunction(m: &SymbolMapping, f: &Prop) -> Option<Vec<Clause>> {
    match f {
        Prop::Or(fv) => {
            let mut c: Clause = ScalaSet::empty();
            for x in fv {
                c = c.incl(to_literal(m, x)?);
            }
            Some(vec![c])
        }
        Prop::True => Some(Vec::new()),
        Prop::False => Some(vec![ScalaSet::empty()]),
        other => to_literal(m, other).map(|l| vec![clause1(l)]),
    }
}

fn to_cnf(m: &SymbolMapping, f: &Prop) -> Option<Vec<Clause>> {
    if let Some(cs) = to_disjunction(m, f) {
        return Some(cs);
    }
    if let Prop::And(fv) = f {
        let mut out = Vec::new();
        for x in fv {
            out.extend(to_disjunction(m, x)?);
        }
        return Some(out);
    }
    None
}

fn exceeds_size(p: &Prop) -> bool {
    match p {
        Prop::And(ops) | Prop::Or(ops) => {
            ops.len() > MAX_FORMULA_SIZE || ops.iter().any(exceeds_size)
        }
        Prop::Not(a) => exceeds_size(a),
        _ => false,
    }
}

/// `eqFreePropToSolvable`.
pub(crate) fn eq_free_prop_to_solvable(p: &Prop) -> Result<Solvable, BudgetExceeded> {
    let simplified = simplify(p);
    if exceeds_size(&simplified) {
        return Err(BudgetExceeded);
    }
    let mut syms = Vec::new();
    gather_symbols(p, &mut syms);
    let mapping = std::rc::Rc::new(SymbolMapping::new(&syms));
    // One transformer (one literal counter) for every conjunct, as in nsc.
    let mut transformer = TransformToCnf {
        mapping: &mapping,
        literal_count: mapping.size(),
        buff: Vec::new(),
        const_true: None,
    };
    let cnf_for = |prop: &Prop, t: &mut TransformToCnf| -> Vec<Clause> {
        match to_cnf(t.mapping, prop) {
            Some(cs) => cs,
            None => t.apply(prop),
        }
    };
    let cnf = match &simplified {
        Prop::And(props) => {
            let mut all = Vec::new();
            for x in props {
                all.extend(cnf_for(x, &mut transformer));
            }
            all
        }
        other => cnf_for(other, &mut transformer),
    };
    drop(transformer);
    Ok(Solvable { cnf, mapping })
}

/// A model: the relevant symbols' truth values in assignment order (a
/// `ListMap`), and the symbols left unassigned.
#[derive(Clone, Debug)]
pub(crate) struct Solution {
    pub(crate) model: Vec<(SymId, bool)>,
    pub(crate) unassigned: Vec<SymId>,
}

/// `dropUnit`.
fn drop_unit(clauses: &mut Vec<Option<Clause>>, unit: Lit) {
    let negated = unit.neg();
    let mut j = 0;
    let n = clauses.len();
    let mut i = 0;
    while i < n {
        let Some(c) = clauses[i].take() else {
            break;
        };
        if !c.contains(&unit) {
            clauses[j] = Some(c.excl(&negated));
            j += 1;
        }
        i += 1;
    }
}

/// `findTseitinModel0`: DPLL with an explicit stack. `None` is UNSAT; the
/// model lists the literals most recent first.
fn find_tseitin_model(clauses: &[Clause]) -> Option<Vec<Lit>> {
    type State = (Vec<Option<Clause>>, Vec<Lit>);
    let mut stack: Vec<State> = vec![(clauses.iter().cloned().map(Some).collect(), Vec::new())];
    while let Some((mut clauses, assignments)) = stack.pop() {
        if clauses.is_empty() || clauses[0].is_none() {
            return Some(assignments);
        }
        let mut empty_index = None;
        let mut unit_index = None;
        for (i, c) in clauses.iter().enumerate() {
            if let Some(c) = c {
                match c.len() {
                    0 => {
                        empty_index = Some(i);
                        break;
                    }
                    1 if unit_index.is_none() => unit_index = Some(i),
                    _ => {}
                }
            }
        }
        if empty_index.is_some() {
            continue;
        }
        if let Some(ui) = unit_index {
            let unit = *clauses[ui].as_ref().unwrap().head().unwrap();
            drop_unit(&mut clauses, unit);
            let mut a = assignments;
            a.insert(0, unit);
            stack.push((clauses, a));
            continue;
        }
        // Pure literals.
        let mut pos = std::collections::BTreeSet::new();
        let mut neg = std::collections::BTreeSet::new();
        for c in clauses.iter().flatten() {
            for l in c.elems() {
                if l.positive() {
                    pos.insert(l.variable());
                } else {
                    neg.insert(l.variable());
                }
            }
        }
        let pures: Vec<i32> = pos.symmetric_difference(&neg).copied().collect();
        if let Some(&pure_var) = pures.iter().min() {
            let pure_lit = if neg.contains(&pure_var) {
                Lit(-pure_var)
            } else {
                Lit(pure_var)
            };
            let simplified: Vec<Option<Clause>> = clauses
                .into_iter()
                .filter(|c| !matches!(c, Some(c) if c.contains(&pure_lit)))
                .collect();
            let mut a = assignments;
            a.insert(0, pure_lit);
            stack.push((simplified, a));
            continue;
        }
        let split = *clauses
            .iter()
            .flatten()
            .next()
            .and_then(|c| c.head())
            .expect("a non-empty clause");
        let effective = clauses.iter().position(|c| c.is_none()).unwrap_or(clauses.len());
        let mut pos_clauses: Vec<Option<Clause>> = clauses[..effective].to_vec();
        let mut neg_clauses: Vec<Option<Clause>> = clauses[..effective].to_vec();
        pos_clauses.push(Some(clause1(split)));
        neg_clauses.push(Some(clause1(split.neg())));
        // `loop(pos :: neg :: rest)`: `pos` is tried first.
        stack.push((neg_clauses, assignments.clone()));
        stack.push((pos_clauses, assignments));
    }
    None
}

pub(crate) fn has_model(s: &Solvable) -> bool {
    find_tseitin_model(&s.cnf).is_some()
}

/// `findAllModelsFor`; `Err` when the recursion budget ran out (nsc then
/// warns and keeps the models found so far).
pub(crate) fn find_all_models(s: &Solvable) -> (Vec<Solution>, bool) {
    let m = &s.mapping;
    let mut clauses = s.cnf.clone();
    let mut models: Vec<Solution> = Vec::new();
    let mut depth = MAX_DPLL_DEPTH;
    loop {
        if depth == 0 {
            return (models, true);
        }
        let Some(model) = find_tseitin_model(&clauses) else {
            return (models, false);
        };
        let unassigned: Vec<i32> = m
            .relevant
            .iter()
            .copied()
            .filter(|x| !model.iter().any(|l| l.variable() == *x))
            .collect();
        let sol_model: Vec<(SymId, bool)> = model
            .iter()
            .filter_map(|l| m.sym_for_var.get(&l.variable()).map(|s| (*s, l.positive())))
            .collect();
        // `.to(ListMap)`: a later binding of the same key keeps its place.
        let mut dedup: Vec<(SymId, bool)> = Vec::new();
        for (s, b) in sol_model {
            if let Some(e) = dedup.iter_mut().find(|(k, _)| *k == s) {
                e.1 = b;
            } else {
                dedup.push((s, b));
            }
        }
        let solution = Solution {
            model: dedup,
            unassigned: unassigned.iter().map(|v| m.sym_for_var[v]).collect(),
        };
        // The blocking clause: the negated model, relevant variables only.
        let negated: Vec<Lit> = model
            .iter()
            .filter(|l| m.relevant.binary_search(&l.variable()).is_ok())
            .map(|l| l.neg())
            .collect();
        clauses.push(ScalaSet::list_from(negated));
        models.insert(0, solution);
        depth -= 1;
    }
}
