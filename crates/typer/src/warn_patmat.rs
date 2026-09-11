//! nsc's `patmat` phase, as far as the warnings it issues by default go:
//!
//! * `patterns after a variable pattern cannot match (SLS 8.1.1)` and
//!   `unreachable code due to variable pattern ...` (`MatchWarnings`);
//! * switch emission (`MatchOptimization.SwitchEmission`): `could not emit
//!   switch for @switch annotated match`, `Pattern contains duplicate
//!   alternatives: ...`, and the `unreachable code` a duplicate constant is;
//! * `MatchAnalysis`: `unreachable code` and `match may not be exhaustive.
//!   It would fail on the following input(s): ...`, computed the way nsc
//!   computes them -- the cases are translated to `TreeMaker`s
//!   (`MatchTranslation`), approximated as propositions over "variable =
//!   constant" atoms (`TreeMakersToProps`), and handed to the solver in
//!   `warn_patmat_logic`; the counter-examples are read back out of its
//!   models (`modelToCounterExample`).
//!
//! Matches are visited the way nsc's `MatchTransformer` visits them: the
//! scrutinee and the cases first, then the match itself, so a nested match
//! reports before the one around it.
//!
//! Wherever a type or a pattern shape falls outside what `warn_patmat_types`
//! models, the analysis of that match gives up and reports nothing: a
//! missing warning costs a test, a wrong one would break a valid program's
//! output.

use crate::check::Typer;
use crate::symbol::SymKind;
use crate::warn_patmat_logic::*;
use crate::warn_patmat_types::{sealed_children, CVal, NTy, Types};
use crate::warn_util::{point_of, warning_at};
use scala_rs_parser::ast::*;
use scala_rs_span::{Diagnostic, Phase};
use std::collections::HashMap;

pub(crate) fn run(t: &mut Typer, units: &mut [(&mut Tree, usize)]) {
    let mut sym_ids: usize = 0;
    let mut fresh_ids: u32 = 0;
    let mut out = Vec::new();
    for (tree, file) in units.iter() {
        let Some(src) = t.sources.get(*file).cloned() else {
            continue;
        };
        let mut pass = Patmat {
            t: &mut *t,
            src: src.clone(),
            file: *file,
            out: Vec::new(),
            sym_ids: &mut sym_ids,
            fresh_ids: &mut fresh_ids,
            owners: Vec::new(),
            pf_unchecked: false,
        };
        pass.visit(tree);
        out.extend(pass.out);
    }
    t.diags.extend(out);
}

/// One enclosing definition, for `matchingSymbolInScope`.
enum Owner {
    Method { name: String, params: Vec<(String, SymbolId)> },
    Class(SymbolId),
}

struct Patmat<'a> {
    t: &'a mut Typer,
    src: std::rc::Rc<str>,
    file: usize,
    out: Vec<Diagnostic>,
    /// nsc `Sym.nextSymId`: one counter for the whole run.
    sym_ids: &'a mut usize,
    fresh_ids: &'a mut u32,
    owners: Vec<Owner>,
    /// Inside a `{ case ... }` literal typed as a `PartialFunction`, whose
    /// match nsc synthesizes `@unchecked`.
    pf_unchecked: bool,
}

// ---------------------------------------------------------------------------
// Traversal
// ---------------------------------------------------------------------------

impl<'a> Patmat<'a> {
    fn warn(&mut self, point: u32, msg: impl Into<String>) {
        let fatal = self.t.fatal_warnings;
        self.out.push(warning_at(self.file, point, point + 1, msg, Phase::Patmat, fatal));
    }

    fn point(&self, t: &Tree) -> u32 {
        point_of(t, &self.src)
    }

    fn visit(&mut self, tree: &Tree) {
        match &tree.kind {
            TreeKind::Match { selector, cases } => {
                self.visit(selector);
                for c in cases {
                    self.visit(&c.guard);
                    self.visit(&c.body);
                }
                self.translate_match(tree, selector, cases);
            }
            TreeKind::Try { block, catches, finalizer } => {
                self.visit(block);
                for c in catches {
                    self.visit(&c.guard);
                    self.visit(&c.body);
                }
                if !catches.is_empty() {
                    self.translate_try(catches);
                }
                self.visit(finalizer);
            }
            TreeKind::DefDef { mods, name, vparamss, tparams, rhs, tpt, .. } => {
                if mods.flags.contains(Flags::SYNTHETIC) {
                    return;
                }
                let mut params = Vec::new();
                for p in tparams.iter().chain(vparamss.iter().flatten()) {
                    if let Some(n) = def_name(p) {
                        params.push((n, p.sym));
                    }
                }
                self.owners.push(Owner::Method { name: name.clone(), params });
                self.visit(tpt);
                for p in vparamss.iter().flatten() {
                    self.visit(p);
                }
                self.visit(rhs);
                self.owners.pop();
            }
            TreeKind::ClassDef { mods, impl_, vparamss, .. } => {
                if mods.flags.contains(Flags::SYNTHETIC) {
                    return;
                }
                self.owners.push(Owner::Class(tree.sym));
                for p in vparamss.iter().flatten() {
                    self.visit(p);
                }
                self.template(impl_);
                self.owners.pop();
            }
            TreeKind::ModuleDef { mods, impl_, .. } => {
                if mods.flags.contains(Flags::SYNTHETIC) {
                    return;
                }
                let cls = if tree.sym.is_none() {
                    tree.sym
                } else {
                    self.t.st.module_class_of(tree.sym)
                };
                self.owners.push(Owner::Class(cls));
                self.template(impl_);
                self.owners.pop();
            }
            TreeKind::Function { vparams, body } => {
                for p in vparams {
                    self.visit(p);
                }
                let pf = matches!(&tree.ty, Type::Class { sym, .. }
                    if self.t.st.get(*sym).name == "PartialFunction");
                let saved = std::mem::replace(&mut self.pf_unchecked, pf);
                self.visit(body);
                self.pf_unchecked = saved;
            }
            _ => for_each_child(tree, &mut |c| self.visit(c)),
        }
    }

    fn template(&mut self, impl_: &Template) {
        for p in &impl_.parents {
            self.visit(p);
        }
        for s in &impl_.body {
            self.visit(s);
        }
    }
}

fn def_name(t: &Tree) -> Option<String> {
    match &t.kind {
        TreeKind::ValDef { name, .. } | TreeKind::TypeDef { name, .. } => Some(name.clone()),
        _ => None,
    }
}

/// Every direct child tree, in source order.
fn for_each_child(t: &Tree, f: &mut dyn FnMut(&Tree)) {
    match &t.kind {
        TreeKind::PackageDef { stats, .. } => stats.iter().for_each(f),
        TreeKind::ValDef { tpt, rhs, .. } => {
            f(tpt);
            f(rhs)
        }
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
        TreeKind::Function { vparams, body } => {
            vparams.iter().for_each(&mut *f);
            f(body)
        }
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

// ---------------------------------------------------------------------------
// TreeMakers (`MatchTreeMaking`)
// ---------------------------------------------------------------------------

pub(crate) type B = usize;

pub(crate) struct Binder {
    pub(crate) tp: NTy,
}

/// A field selected off a binder: what nsc's `subPatRefs` build.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Field {
    /// A case accessor (`x.a`), or `_i` of a tuple-valued extractor result.
    Acc(SymbolId),
    Tuple(usize),
    /// `seq.apply(i)`
    Index(usize),
    /// `seq.drop(n)`
    Drop(usize),
}

/// nsc's path trees (`x1`, `x1.a`, `x3._1.apply(0)`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PTree {
    Ref(B),
    Sel(Box<PTree>, Field),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Subst {
    pub(crate) from: Vec<B>,
    pub(crate) to: Vec<PTree>,
}

impl Subst {
    pub(crate) fn one(from: B, to: PTree) -> Subst {
        Subst { from: vec![from], to: vec![to] }
    }
    fn is_empty(&self) -> bool {
        self.from.is_empty()
    }
    pub(crate) fn apply(&self, t: &PTree) -> PTree {
        match t {
            PTree::Ref(b) => match self.from.iter().position(|f| f == b) {
                Some(i) => self.to[i].clone(),
                None => t.clone(),
            },
            PTree::Sel(p, f) => PTree::Sel(Box::new(self.apply(p)), f.clone()),
        }
    }
    /// nsc `this >> other`: `forall t. this(other(t)) == (this >> other)(t)`.
    pub(crate) fn then(&self, other: &Subst) -> Subst {
        if other.is_empty() {
            return self.clone();
        }
        if self.is_empty() {
            return other.clone();
        }
        let mut from = other.from.clone();
        let mut to: Vec<PTree> = other.to.iter().map(|t| self.apply(t)).collect();
        for (f, t) in self.from.iter().zip(&self.to) {
            if !other.from.contains(f) {
                from.push(*f);
                to.push(t.clone());
            }
        }
        Subst { from, to }
    }
}

/// What a stable-identifier / literal pattern compares against
/// (`EqualityTestTreeMaker.patTree`).
#[derive(Clone, Debug)]
pub(crate) struct PatVal {
    /// Equivalence for `unique(patTree)` (nsc's `equivalentTree`).
    pub(crate) key: PatKey,
    /// `p.tpe.normalize`.
    pub(crate) tp: NTy,
    /// `p.symbol.isStable`.
    pub(crate) stable: bool,
    /// `p.toString` / `p.symbol.name`.
    pub(crate) text: String,
    /// The constant a switch would use (`SwitchablePattern`).
    pub(crate) switch_const: Option<CVal>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PatKey {
    Lit(CVal),
    Sym(SymbolId, Option<Box<PatKey>>),
    Unstable(u32),
}

#[derive(Clone, Debug)]
pub(crate) struct ExtInfo {
    /// The `unapply` / `unapplySeq` method.
    pub(crate) unapply: SymbolId,
    /// The extractor call's type is `Some[...]`, or has a constant-false
    /// `isEmpty`, or is the constant `true` (`irrefutableExtractorType`).
    pub(crate) irrefutable: bool,
    /// The result binder is `List.unapplySeq`'s `UnapplySeqWrapper`.
    pub(crate) seq_wrapper: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum TM {
    TypeTest {
        prev: B,
        tested: B,
        expected: NTy,
        next: B,
        extractor_arg: bool,
    },
    EqTest {
        prev: B,
        pat: PatVal,
        next: B,
    },
    Alts {
        prev: B,
        alts: Vec<Vec<Maker>>,
        pos: u32,
        /// The alternatives' switch constants, when every one is switchable.
        switch: Option<Vec<(CVal, String)>>,
    },
    Product {
        prev: B,
        has_extra: bool,
        subs: Vec<B>,
        refs: Vec<PTree>,
    },
    Extractor {
        ext: ExtInfo,
        has_extra: bool,
        next: B,
        subs: Vec<B>,
        refs: Vec<PTree>,
        prev: B,
        checked_length: Option<usize>,
    },
    NonNull {
        prev: B,
    },
    Guard {
        constant: Option<bool>,
    },
    Body {
        pos: u32,
    },
    SubstOnly {
        prev: B,
        next: B,
    },
    Dummy,
}

#[derive(Clone, Debug)]
pub(crate) struct Maker {
    pub(crate) tm: TM,
    /// `currSub` once `propagateSubstitution` has run.
    pub(crate) sub: Option<Subst>,
}

impl Maker {
    pub(crate) fn new(tm: TM) -> Maker {
        Maker { tm, sub: None }
    }

    fn local(&self) -> Subst {
        match &self.tm {
            TM::TypeTest { prev, next, .. } | TM::EqTest { prev, next, .. } => {
                Subst::one(*prev, PTree::Ref(*next))
            }
            TM::SubstOnly { prev, next } => Subst::one(*prev, PTree::Ref(*next)),
            // `PreserveSubPatBinders` with `debugInfoEmitVars`: every sub-pattern
            // binder is stored, so none is substituted.
            _ => Subst::default(),
        }
    }

    pub(crate) fn substitution(&self) -> Subst {
        self.sub.clone().unwrap_or_else(|| self.local())
    }

    /// `subPatternsAsSubstitution`.
    pub(crate) fn sub_patterns_as_substitution(&self) -> Subst {
        match &self.tm {
            TM::Product { subs, refs, .. } | TM::Extractor { subs, refs, .. } => {
                let s = Subst { from: subs.clone(), to: refs.clone() };
                s.then(&self.substitution())
            }
            _ => self.substitution(),
        }
    }
}

/// `propagateSubstitution`: accumulate each maker's substitution, dropping
/// `SubstOnly` and `Dummy` makers.
pub(crate) fn propagate(makers: Vec<Maker>, initial: Subst) -> Vec<Maker> {
    let mut accum = initial;
    let mut out = Vec::new();
    for mut m in makers {
        if matches!(m.tm, TM::Dummy) {
            continue;
        }
        let cur = accum.then(&m.local());
        if let TM::Alts { alts, .. } = &mut m.tm {
            let taken = std::mem::take(alts);
            *alts = taken.into_iter().map(|a| propagate(a, cur.clone())).collect();
        }
        m.sub = Some(cur.clone());
        accum = cur;
        if !matches!(m.tm, TM::SubstOnly { .. }) {
            out.push(m);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// One match (`translateMatch` / `combineCases`)
// ---------------------------------------------------------------------------

/// A case as `SwitchMaker` sees it: its pattern reduced to switch constants.
#[derive(Clone, Debug)]
enum SwPat {
    Default,
    Lit(CVal),
    Alt(Vec<CVal>),
}

#[derive(Clone, Debug)]
struct SwCase {
    pat: SwPat,
    guard: Option<Option<bool>>,
    body_pos: u32,
}

impl SwCase {
    fn guarded(&self) -> bool {
        self.guard.is_some()
    }
}

fn pattern_implies(x: &SwPat, y: &SwPat) -> bool {
    match (x, y) {
        (SwPat::Alt(ps), _) => ps.iter().any(|p| pattern_implies(&SwPat::Lit(p.clone()), y)),
        (_, SwPat::Alt(qs)) => qs.iter().any(|q| pattern_implies(x, &SwPat::Lit(q.clone()))),
        (SwPat::Lit(a), SwPat::Lit(b)) => a == b,
        (SwPat::Default, _) => true,
        _ => false,
    }
}

fn pattern_equals(x: &SwPat, y: &SwPat) -> bool {
    match (x, y) {
        (SwPat::Alt(xs), SwPat::Alt(ys)) => {
            xs.iter().all(|a| ys.iter().any(|b| a == b)) && ys.iter().all(|b| xs.iter().any(|a| a == b))
        }
        (SwPat::Alt(ps), _) => ps.iter().all(|p| pattern_equals(&SwPat::Lit(p.clone()), y)),
        (_, SwPat::Alt(qs)) => qs.iter().all(|q| pattern_equals(x, &SwPat::Lit(q.clone()))),
        (SwPat::Lit(a), SwPat::Lit(b)) => a == b,
        (SwPat::Default, SwPat::Default) => true,
        _ => false,
    }
}

/// `SwitchMaker.unreachableCase`
fn switch_unreachable(cases: &[SwCase]) -> Option<usize> {
    let mut i = 0;
    while i < cases.len() {
        let head = &cases[i];
        let is_default = matches!(head.pat, SwPat::Default) && !head.guarded();
        if is_default && i + 1 < cases.len() {
            return Some(i + 1);
        }
        if !head.guarded() || head.guard == Some(Some(true)) {
            if let Some(j) = (i + 1..cases.len()).find(|j| pattern_implies(&head.pat, &cases[*j].pat)) {
                return Some(j);
            }
            i += 1;
            continue;
        }
        if head.guard == Some(Some(false)) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `collapseGuardedCases`: whether the guarded cases can be merged.
fn collapse_guarded(cases: &[SwCase]) -> bool {
    let mut remaining: Vec<SwCase> = cases.to_vec();
    let mut collapsed = 0usize;
    while !remaining.is_empty() {
        let curr = remaining[0].clone();
        let curr_default = matches!(curr.pat, SwPat::Default);
        let (implies_curr, others): (Vec<SwCase>, Vec<SwCase>) = if curr_default {
            (remaining[1..].to_vec(), Vec::new())
        } else {
            remaining[1..].iter().cloned().partition(|c| pattern_implies(&curr.pat, &c.pat))
        };
        let unguarded_ok = (!curr.guarded() && implies_curr.is_empty()) || {
            match implies_curr.iter().position(|c| !c.guarded()) {
                None => true,
                Some(k) => k + 1 == implies_curr.len(),
            }
        };
        if unguarded_ok && implies_curr.iter().all(|c| pattern_equals(&curr.pat, &c.pat)) {
            collapsed += 1;
            remaining = others;
        } else {
            return false;
        }
    }
    collapsed > 0
}

/// `Constant.toString` of the switch literal (`LIT(const.value.intValue)`
/// for anything in `Int` range).
fn switch_lit(c: &CVal) -> CVal {
    match c {
        CVal::Char(ch) => CVal::Int(*ch as i32),
        CVal::Byte(n) => CVal::Int(*n as i32),
        CVal::Short(n) => CVal::Int(*n as i32),
        other => other.clone(),
    }
}

fn is_annotated_with(t: &Tree, what: &str) -> bool {
    fn ty_has(ty: &Type, what: &str) -> bool {
        match ty {
            Type::Annotated { annot, tpe } => annot.rsplit('.').next() == Some(what) || ty_has(tpe, what),
            _ => false,
        }
    }
    fn tpt_has(tpt: &Tree, what: &str) -> bool {
        match &tpt.kind {
            TreeKind::AnnotatedTypeTree { annot, tpt } => {
                let path = annot.annotation_path();
                path.rsplit('.').next() == Some(what) || tpt_has(tpt, what)
            }
            _ => ty_has(&tpt.ty, what),
        }
    }
    match &t.kind {
        TreeKind::Typed { tpt, .. } => tpt_has(tpt, what) || ty_has(&t.ty, what),
        _ => ty_has(&t.ty, what),
    }
}

impl<'a> Patmat<'a> {
    fn scrutinee_type(&self, selector: &Tree) -> NTy {
        let tys = Types { st: &self.t.st };
        match &selector.kind {
            TreeKind::Literal { lit } => match CVal::from_lit(lit) {
                Some(CVal::Null) => NTy::Null,
                Some(c) => NTy::Const(c),
                None => NTy::Unknown,
            },
            TreeKind::Ident { .. } | TreeKind::Select { .. } => {
                match crate::warn_patmat_types::library_constant(&self.t.st, selector.sym) {
                    Some(c) => NTy::Const(c),
                    None => tys.of(&selector.ty),
                }
            }
            _ => tys.of(&selector.ty),
        }
    }

    /// The point a warning about the scrutinee is reported at (the binder's
    /// position): for `(x: @switch)`, the annotation's name.
    fn selector_point(&self, selector: &Tree) -> u32 {
        if let TreeKind::Typed { expr, .. } = &selector.kind {
            let lo = expr.span.hi.0 as usize;
            let hi = (selector.span.hi.0 as usize).min(self.src.len());
            if let Some(at) = self.src.get(lo..hi).and_then(|s| s.find('@')) {
                return (lo + at + 1) as u32;
            }
        }
        self.point(selector)
    }

    fn translate_match(&mut self, _tree: &Tree, selector: &Tree, cases: &[CaseDef]) {
        self.check_variable_patterns(cases);
        let sel_name = selector.name().unwrap_or("");
        // A `for` generator's pattern function is guarded by its
        // `withFilter`, and a pattern definition's selector is `@unchecked`
        // in nsc: neither is checked for exhaustivity.
        let synthetic_for = sel_name.starts_with("x$for");
        let pattern_def = sel_name.starts_with("x$pat");
        let pf_literal = sel_name == "x$pf" && self.pf_unchecked;
        let suppress_exhaustive =
            is_annotated_with(selector, "unchecked") || synthetic_for || pattern_def || pf_literal;
        let suppress_unreachable = synthetic_for;
        let scrut_tp = self.scrutinee_type(selector);
        if scrut_tp.is_unknown() {
            return;
        }
        let fresh = &mut *self.fresh_ids;
        let src = self.src.clone();
        let (binders, root, translated) = {
            let tys = Types { st: &self.t.st };
            let mut tr = crate::warn_patmat_translate::Translator::new(tys, fresh, &src);
            let root = tr.binder(scrut_tp.clone());
            let mut out = Vec::new();
            let mut ok = true;
            for c in cases {
                match tr.translate_case(root, c) {
                    Ok(ms) => out.push(ms),
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            (std::mem::take(&mut tr.binders), root, ok.then_some(out))
        };
        let Some(makers) = translated else {
            return;
        };
        // `emitSwitch`
        let tys = Types { st: &self.t.st };
        let widened = tys.widen(&scrut_tp);
        let switchable_tpe = matches!(&widened, NTy::Class(c, _)
            if [self.t.st.byte_sym, self.t.st.short_sym, self.t.st.int_sym, self.t.st.char_sym, self.t.st.string_sym].contains(c));
        if switchable_tpe && self.emit_switch(&makers) {
            return;
        }
        // `requiresSwitch`
        if is_annotated_with(selector, "switch") {
            let count: usize = if makers.len() >= 3 {
                3
            } else {
                makers
                    .iter()
                    .map(|c| match c.first().map(|m| &m.tm) {
                        Some(TM::Alts { alts, .. }) => alts.len().min(3),
                        _ => 1,
                    })
                    .sum()
            };
            if count > 2 {
                let p = self.selector_point(selector);
                self.warn(p, "could not emit switch for @switch annotated match");
            }
        }
        // `Switchable(scrutSym, cases)`: switchable, just not worth a switch.
        let switchable = switchable_tpe && makers.iter().all(|c| all_switchable(c));
        let suppress_exhaustive = suppress_exhaustive || switchable;
        if makers.is_empty() {
            return;
        }
        let point = self.selector_point(selector);
        self.analyze_cases(&binders, root, &makers, point, suppress_exhaustive, suppress_unreachable);
    }

    /// `analyzeCases`
    fn analyze_cases(
        &mut self,
        binders: &[Binder],
        root: B,
        makers: &[Vec<Maker>],
        point: u32,
        suppress_exhaustive: bool,
        suppress_unreachable: bool,
    ) {
        if !suppress_unreachable {
            let r = {
                let mut a = crate::warn_patmat_analysis::Approx::new(self.t, binders, root, self.sym_ids);
                a.unreachable_case(makers)
            };
            if let Ok(Some(i)) = r {
                if let Some(Maker { tm: TM::Body { pos }, .. }) = makers[i].last() {
                    self.warn(*pos, "unreachable code");
                }
            }
        }
        if !suppress_exhaustive {
            let r = {
                let mut a = crate::warn_patmat_analysis::Approx::new(self.t, binders, root, self.sym_ids);
                a.exhaustive(makers)
            };
            if let Ok((examples, depth_reached)) = r {
                if depth_reached {
                    let fatal = self.t.fatal_warnings;
                    self.out.push(warning_at(
                        self.file,
                        point,
                        point + 1,
                        "Exhaustivity analysis reached max recursion depth, not all missing cases are reported.\n(Please try with scalac -Ypatmat-exhaust-depth 40 or -Ypatmat-exhaust-depth off.)",
                        Phase::Patmat,
                        fatal,
                    ));
                }
                if !examples.is_empty() {
                    let ce = match examples.as_slice() {
                        [one] if one == "_" => String::new(),
                        [one] => format!("\nIt would fail on the following input: {one}"),
                        many => format!("\nIt would fail on the following inputs: {}", many.join(", ")),
                    };
                    self.warn(point, format!("match may not be exhaustive.{ce}"));
                }
            }
        }
    }

    /// `SwitchEmission.emitSwitch`; true when a switch is emitted (and the
    /// match is then not analysed at all).
    fn emit_switch(&mut self, makers: &[Vec<Maker>]) -> bool {
        if makers.len() < 2 {
            return false;
        }
        let mut cases: Vec<SwCase> = Vec::new();
        for c in makers {
            let (head, rest): (Option<&TM>, &[Maker]) = match c.as_slice() {
                [Maker { tm: TM::Body { .. }, .. }] | [Maker { tm: TM::Guard { .. }, .. }, Maker { tm: TM::Body { .. }, .. }] => {
                    (None, c.as_slice())
                }
                [first, rest @ ..] => (Some(&first.tm), rest),
                [] => return false,
            };
            let (guard, body_pos) = match rest {
                [Maker { tm: TM::Body { pos }, .. }] => (None, *pos),
                [Maker { tm: TM::Guard { constant }, .. }, Maker { tm: TM::Body { pos }, .. }] => {
                    (Some(*constant), *pos)
                }
                _ => return false,
            };
            let pat = match head {
                None => SwPat::Default,
                Some(TM::EqTest { pat, .. }) => match &pat.switch_const {
                    Some(k) => SwPat::Lit(switch_lit(k)),
                    None => return false,
                },
                Some(TM::Alts { switch: Some(alts), pos, .. }) => {
                    let lits: Vec<CVal> = alts.iter().map(|(k, _)| switch_lit(k)).collect();
                    // scala/bug#7290: report the first duplicate of each constant.
                    let mut distinct: Vec<CVal> = Vec::new();
                    for l in &lits {
                        if !distinct.contains(l) {
                            distinct.push(l.clone());
                        }
                    }
                    if distinct.len() < lits.len() {
                        let groups: Vec<(CVal, i32)> =
                            distinct.iter().map(|k| (k.clone(), k.scala_hash())).collect();
                        let ordered = crate::scala_coll::champ_order(&groups);
                        let dups: Vec<String> = ordered
                            .iter()
                            .filter(|k| lits.iter().filter(|l| l == k).count() > 1)
                            .map(|k| k.escaped())
                            .collect();
                        let pos = *pos;
                        self.warn(pos, format!("Pattern contains duplicate alternatives: {}", dups.join(", ")));
                    }
                    SwPat::Alt(distinct)
                }
                _ => return false,
            };
            cases.push(SwCase { pat, guard, body_pos });
        }
        if let Some(i) = switch_unreachable(&cases) {
            let pos = cases[i].body_pos;
            self.warn(pos, "unreachable code");
            return false;
        }
        if cases.iter().all(|c| !c.guarded()) {
            return true;
        }
        collapse_guarded(&cases)
    }

    /// `checkMatchVariablePatterns`
    fn check_variable_patterns(&mut self, cases: &[CaseDef]) {
        let mut vpat: Option<String> = None;
        for (i, c) in cases.iter().enumerate() {
            if let Some(v) = &vpat {
                let add = self.addendum(&c.pat);
                let p = self.point(&c.body);
                self.warn(p, format!("unreachable code due to {v}{add}"));
            } else if i + 1 < cases.len() && c.guard.is_empty() && is_default_pattern(&c.pat) {
                let name = match &c.pat.kind {
                    TreeKind::Ident { name } if name != "_" => format!(" '{name}'"),
                    TreeKind::Bind { name, .. } => format!(" '{name}'"),
                    _ => String::new(),
                };
                let line = {
                    let off = c.pat.span.lo.0 as usize;
                    self.src[..off.min(self.src.len())].matches('\n').count() + 1
                };
                vpat = Some(format!("variable pattern{name} on line {line}"));
                let add = self.addendum(&c.pat);
                let p = c.pat.span.lo.0;
                self.warn(p, format!("patterns after a variable pattern cannot match (SLS 8.1.1){add}"));
            }
        }
    }

    /// `matchingSymbolInScope`: a definition the variable's name shadows.
    fn addendum(&self, pat: &Tree) -> String {
        let name = match &pat.kind {
            TreeKind::Ident { name } if name != "_" => name.clone(),
            TreeKind::Bind { name, .. } => name.clone(),
            _ => return String::new(),
        };
        let st = &self.t.st;
        for o in self.owners.iter().rev() {
            match o {
                Owner::Method { name: m, params } => {
                    if params.iter().any(|(n, _)| *n == name) {
                        return format!(
                            "\nIf you intended to match against parameter {name} of method {m}, you must use backticks, like: case `{name}` =>"
                        );
                    }
                }
                Owner::Class(c) if !c.is_none() => {
                    let hit = st
                        .members_including_inherited(*c)
                        .into_iter()
                        .find(|m| st.get(*m).name == name && st.get(*m).kind != SymKind::TypeMember);
                    if let Some(m) = hit {
                        let s = st.get(m);
                        let kind = match s.kind {
                            SymKind::Module | SymKind::ModuleClass => "object",
                            SymKind::Method => "method",
                            SymKind::Class => "class",
                            _ if s.flags.contains(Flags::LAZY) => "lazy value",
                            _ if s.flags.contains(Flags::MUTABLE) => "variable",
                            _ => "value",
                        };
                        let owner = st.get(*c);
                        let okind = match owner.kind {
                            SymKind::ModuleClass | SymKind::Module => "object",
                            _ if owner.flags.contains(Flags::TRAIT) => "trait",
                            _ => "class",
                        };
                        let oname = owner.name.trim_end_matches('$');
                        return format!(
                            "\nIf you intended to match against {kind} {name} in {okind} {oname}, you must use backticks, like: case `{name}` =>"
                        );
                    }
                }
                _ => {}
            }
        }
        String::new()
    }

    /// `translateTry`: catch clauses are checked for unreachable cases only.
    fn translate_try(&mut self, catches: &[CaseDef]) {
        let Some(throwable) = crate::classpath::find_by_jvm(&self.t.st, "java/lang/Throwable") else {
            return;
        };
        let tys = Types { st: &self.t.st };
        // `treeInfo.isCatchCase` for every clause: a type switch.
        let mut simple: Vec<(Option<NTy>, u32)> = Vec::new();
        let mut all_simple = true;
        for c in catches {
            if !c.guard.is_empty() {
                all_simple = false;
                break;
            }
            let tpt = match &c.pat.kind {
                TreeKind::Typed { expr, .. } if matches!(&expr.kind, TreeKind::Ident { name } if name == "_") => {
                    Some(tys.of(&c.pat.ty))
                }
                TreeKind::Bind { body, .. } => match &body.kind {
                    TreeKind::Typed { expr, .. }
                        if matches!(&expr.kind, TreeKind::Ident { name } if name == "_")
                            || matches!(expr.kind, TreeKind::Wildcard) =>
                    {
                        Some(tys.of(&body.ty))
                    }
                    _ if crate::warn_patmat_translate::Translator::is_wildcard(body) => None,
                    _ => {
                        all_simple = false;
                        break;
                    }
                },
                TreeKind::Typed { expr, .. } if crate::warn_patmat_translate::Translator::is_var_pattern(expr) => {
                    Some(tys.of(&c.pat.ty))
                }
                _ if crate::warn_patmat_translate::Translator::is_wildcard(&c.pat)
                    || crate::warn_patmat_translate::Translator::is_var_pattern(&c.pat) =>
                {
                    None
                }
                _ => {
                    all_simple = false;
                    break;
                }
            };
            if let Some(t) = &tpt {
                let simple_throwable = matches!(t, NTy::Class(s, _)
                    if !self.t.st.get(*s).flags.contains(Flags::TRAIT)
                        && tys.sub(t, &NTy::Class(throwable, vec![])) == Some(true));
                if !simple_throwable {
                    all_simple = false;
                    break;
                }
            }
            simple.push((tpt, self.point(&c.body)));
        }
        if all_simple {
            // `typeSwitchMaker.unreachableCase`
            let is_default = |t: &Option<NTy>| match t {
                None => true,
                Some(NTy::Class(s, _)) => *s == throwable,
                _ => false,
            };
            let implies = |x: &Option<NTy>, y: &Option<NTy>| -> bool {
                match (x, y) {
                    (None, _) => true,
                    (Some(tx), Some(ty)) => tys.instance_of_implies(ty, tx) == Some(true),
                    _ => false,
                }
            };
            for i in 0..simple.len() {
                if is_default(&simple[i].0) && i + 1 < simple.len() {
                    let p = simple[i + 1].1;
                    self.warn(p, "unreachable code");
                    return;
                }
                if let Some(j) = (i + 1..simple.len()).find(|j| implies(&simple[i].0, &simple[*j].0)) {
                    let p = simple[j].1;
                    self.warn(p, "unreachable code");
                    return;
                }
            }
            return;
        }
        let scrut_tp = NTy::Class(throwable, vec![]);
        let fresh = &mut *self.fresh_ids;
        let src = self.src.clone();
        let (binders, root, translated) = {
            let tys = Types { st: &self.t.st };
            let mut tr = crate::warn_patmat_translate::Translator::new(tys, fresh, &src);
            let root = tr.binder(scrut_tp);
            let mut out = Vec::new();
            let mut ok = true;
            for c in catches {
                match tr.translate_case(root, c) {
                    Ok(ms) => out.push(ms),
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            (std::mem::take(&mut tr.binders), root, ok.then_some(out))
        };
        if let Some(makers) = translated {
            let point = catches[0].pat.span.lo.0;
            self.analyze_cases(&binders, root, &makers, point, true, false);
        }
    }
}

/// `Switchable`: every tree maker a switchable test (a guard is not).
fn all_switchable(c: &[Maker]) -> bool {
    c.iter().all(|m| match &m.tm {
        TM::EqTest { pat, .. } => pat.switch_const.is_some(),
        TM::Alts { alts, .. } => alts.iter().all(|a| all_switchable(a)),
        TM::Body { .. } => true,
        _ => false,
    })
}

/// `treeInfo.isDefaultCase`'s pattern half: `_`, `x`, `x @ _`.
fn is_default_pattern(p: &Tree) -> bool {
    use crate::warn_patmat_translate::Translator;
    match &p.kind {
        TreeKind::Bind { body, .. } => Translator::is_wildcard(body),
        _ => Translator::is_wildcard(p) || Translator::is_var_pattern(p),
    }
}
