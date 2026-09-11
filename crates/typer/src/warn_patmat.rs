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
