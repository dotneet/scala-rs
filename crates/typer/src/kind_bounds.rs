//! Kind conformance of higher-kinded type arguments, and the variance of a
//! type lambda's own parameters.
//!
//! Two checks nsc runs that this compiler did not have, found when the
//! `Applied`/`Applied` arm of `is_sub_type` started reading an abstract
//! constructor's arguments at the constructor's declared variance: `neg/t7872b`
//! and `neg/t7872c` had been rejected for that arm's bug and compiled once it
//! was fixed.
//!
//! 1. **`checkKindBounds`** (`Kinds.scala`, `checkKindBoundsHK`). A type
//!    argument passed to a higher-kinded parameter must have parameters of
//!    the same number, compatible variance and no stricter bounds, level by
//!    level: `List` (whose `A` is covariant) is not an `F[-_]`, `Set` is not an
//!    `F[+_]`, a `class Bnd[A <: AnyVal]` is not an `F[_]`. Variance is
//!    absolute -- the parameter's own variance does not flip its higher-order
//!    parameters -- but the *direction* of the comparison alternates with
//!    every level, because type application is contravariant: `M[_[_]]`
//!    accepts `Functor[F[_]]` and refuses `CFunctor[F[+_]]`, while `M[_[+_]]`
//!    accepts both. `Any` and `Nothing` are kind-overloaded and always pass,
//!    and a wildcard argument is an expectation nothing has decided yet.
//!
//!    nsc runs it from `checkBounds`, so it applies to written type
//!    arguments of a method, inferred ones (prefixed "inferred "), and the
//!    arguments of a written class type (`new Foo[Set]`), and a kind error
//!    replaces the bounds error that site would otherwise report.
//!
//! 2. **Variance of a type lambda's body** (`Variances.scala`,
//!    `validateDefinition` with `base` the lambda). `[-a]List[a]` puts a
//!    contravariant parameter in a covariant position; nsc reports it for the
//!    refinement member itself, wherever the refinement is written, and the
//!    same rule applies to a named higher-kinded alias (`type l[-a] = Cov[a]`)
//!    and to an abstract member's bounds. A parameter of the member occurring
//!    in its own *lower* bound keeps its position (`type G[+x] >: F[x]` is
//!    legal); the class's parameters are flipped there as usual. nsc exempts
//!    a definition local to a block (its owner is a term), and so does this.
//!
//! Messages follow nsc's wording so the corpus scores them; prefixes and
//! qualification of printed types follow `display_type`.

use crate::check::*;
use crate::symbol::{subst_tparams_slice, SymKind};
use scala_rs_parser::ast::*;
use scala_rs_span::Span;

/// One parameter of a type constructor as the kind check reads it, with its
/// bounds already read through whatever the constructor was applied to.
#[derive(Clone, Debug)]
struct KParam {
    name: String,
    /// `+` is 1, `-` is -1, invariant 0.
    variance: i8,
    lo: Option<Type>,
    hi: Option<Type>,
    /// The symbol, for substituting this parameter into a declared bound;
    /// `NONE` for `Array`'s element parameter, which the table does not hold.
    sym: SymbolId,
    nested: Vec<KParam>,
}

#[derive(Default)]
struct KindErrors {
    arity: Vec<String>,
    variance: Vec<String>,
    strictness: Vec<String>,
}

impl KindErrors {
    fn is_empty(&self) -> bool {
        self.arity.is_empty() && self.variance.is_empty() && self.strictness.is_empty()
    }

    /// nsc `KindErrors.errorMessage`: the explanation that follows the first line.
    fn message(&self, targ: &str, tparam: &str) -> String {
        let mut s =
            format!("{targ}'s type parameters do not match {tparam}'s expected parameters:");
        for list in [&self.arity, &self.variance, &self.strictness] {
            if !list.is_empty() {
                s.push('\n');
                s.push_str(&list.join(", "));
            }
        }
        s
    }
}

fn variance_of(flags: Flags) -> i8 {
    if flags.contains(Flags::COVARIANT) {
        1
    } else if flags.contains(Flags::CONTRAVARIANT) {
        -1
    } else {
        0
    }
}

fn variance_word(v: i8) -> &'static str {
    match v {
        1 => "covariant",
        -1 => "contravariant",
        _ => "invariant",
    }
}

fn variance_sign(v: i8) -> &'static str {
    match v {
        1 => "+",
        -1 => "-",
        _ => "",
    }
}

/// nsc `countElementsAsString(n, "type parameter")`.
fn count_tparams(n: usize) -> String {
    match n {
        0 => "no type parameters".to_string(),
        1 => "1 type parameter".to_string(),
        n => format!("{n} type parameters"),
    }
}

/// Whether a type still names something unresolved (or erroneous), which no
/// check here may judge.
fn has_unresolved(ty: &Type) -> bool {
    match ty {
        Type::Named { .. } | Type::Error | Type::NoType => true,
        Type::Class { args, .. } | Type::Tuple(args) => args.iter().any(has_unresolved),
        Type::Applied { ctor, args } => has_unresolved(ctor) || args.iter().any(has_unresolved),
        Type::Function { params, ret } => params.iter().any(has_unresolved) || has_unresolved(ret),
        Type::Method { paramss, ret } => {
            paramss.iter().flatten().any(has_unresolved) || has_unresolved(ret)
        }
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => has_unresolved(t),
        Type::Annotated { tpe, .. } => has_unresolved(tpe),
        Type::BoundedWildcard { lo, hi } => {
            lo.as_deref().is_some_and(has_unresolved) || hi.as_deref().is_some_and(has_unresolved)
        }
        Type::Refined { parents, .. } => parents.iter().any(has_unresolved),
        _ => false,
    }
}

impl Typer {
    // ---------------------------------------------------------------------
    // 1. checkKindBounds
    // ---------------------------------------------------------------------

    /// nsc `checkBounds`' first half: kind-check `targs` against `tparams`
    /// and report nsc's `KindBoundErrors` message. `prefix` is `"inferred "`
    /// or empty; `location` is `tparams.head.locationString` (` in class Foo`
    /// for a class's parameters, nothing for a method's). Returns whether an
    /// error was reported, in which case the caller skips its bounds check as
    /// nsc does.
    pub(crate) fn check_kind_bounds(
        &mut self,
        tparams: &[SymbolId],
        targs: &[Type],
        prefix: &str,
        location: &str,
        span: Span,
    ) -> bool {
        if tparams.len() != targs.len() || tparams.is_empty() {
            return false;
        }
        // Only a higher-kinded parameter has anything to compare; the arity
        // of a proper argument against a constructor parameter (and back) is
        // `apply_types`' and the kind-arity checks' business.
        if tparams.iter().all(|tp| self.st.get(*tp).tparams.is_empty()) {
            return false;
        }
        let mut explanations = Vec::new();
        for (tp, targ) in tparams.iter().zip(targs) {
            if let Some(errs) = self.kind_errors_for(*tp, targ, tparams, targs) {
                let targ_s = self.st.display_type(targ);
                let tp_s = format!("type {}", self.st.get(*tp).name);
                explanations.push(errs.message(&targ_s, &tp_s));
            }
        }
        if explanations.is_empty() {
            return false;
        }
        let targs_s = targs
            .iter()
            .map(|t| self.st.display_type(t))
            .collect::<Vec<_>>()
            .join(",");
        let tparams_s = tparams
            .iter()
            .map(|tp| format!("type {}", self.st.get(*tp).name))
            .collect::<Vec<_>>()
            .join(",");
        self.error(
            span,
            format!(
                "{prefix}kinds of the type arguments ({targs_s}) do not conform to the expected kinds of the type parameters ({tparams_s}){location}.\n{}",
                explanations.join("\n")
            ),
        );
        true
    }

    /// nsc `Symbol.locationString` for the first of a class's type parameters:
    /// ` in class Foo` / ` in trait Foo` / ` in object Foo`, and nothing when
    /// the owner is not a class (a method's or a type member's parameters).
    pub(crate) fn tparam_location_string(&self, tp: SymbolId) -> String {
        let owner = self.st.get(tp).owner;
        if owner.is_none() {
            return String::new();
        }
        let s = self.st.get(owner);
        let word = match s.kind {
            SymKind::Class if s.flags.contains(Flags::TRAIT) => "trait",
            SymKind::Class => "class",
            SymKind::Module | SymKind::ModuleClass => "object",
            _ => return String::new(),
        };
        format!(" in {word} {}", s.name.trim_end_matches('$'))
    }

    /// The kind errors of one (parameter, argument) pair, or `None` when there
    /// are none or the pair is not this check's to judge.
    fn kind_errors_for(
        &self,
        tparam: SymbolId,
        targ: &Type,
        tparams: &[SymbolId],
        targs: &[Type],
    ) -> Option<KindErrors> {
        if self.st.get(tparam).tparams.is_empty() {
            return None;
        }
        match targ {
            // `Any` and `Nothing` are kind-overloaded; a wildcard is an
            // expectation some enclosing call has not decided.
            Type::Any
            | Type::Nothing
            | Type::Wildcard
            | Type::BoundedWildcard { .. }
            | Type::Error
            | Type::NoType => return None,
            _ => {}
        }
        let param = self.kparam_of_sym(tparam, &[], &[]);
        let arg_params = self.kparams_of_type(targ)?;
        // A top-level arity mismatch is reported elsewhere (`too many type
        // arguments`, `takes type parameters`); only the structure below it
        // is compared here.
        if arg_params.len() != param.nested.len() {
            return None;
        }
        let arg = KParam {
            name: self.st.display_type(targ),
            variance: 0,
            lo: None,
            hi: None,
            sym: SymbolId::NONE,
            nested: arg_params,
        };
        let mut errs = KindErrors::default();
        let mut under = Vec::new();
        let mut with = Vec::new();
        self.check_kind_hk(
            &arg, &param, false, tparams, targs, &mut under, &mut with, &mut errs,
        );
        if errs.is_empty() {
            None
        } else {
            Some(errs)
        }
    }

    /// nsc `checkKindBoundsHK`: compare `arg`'s parameters with `param`'s.
    /// `flip` alternates per level; `under`/`with` accumulate the parameters
    /// of the enclosing levels and what they are instantiated to, so a bound
    /// declared in terms of a sibling (`F[X, Y <: X]`) is read against the
    /// argument's own spelling of that sibling.
    #[allow(clippy::too_many_arguments)]
    fn check_kind_hk(
        &self,
        arg: &KParam,
        param: &KParam,
        flip: bool,
        tparams: &[SymbolId],
        targs: &[Type],
        under: &mut Vec<SymbolId>,
        with: &mut Vec<Type>,
        errs: &mut KindErrors,
    ) {
        let hkargs = &arg.nested;
        let hkparams = &param.nested;
        if hkargs.len() != hkparams.len() {
            errs.arity.push(format!(
                "type {} has {}, but type {} has {}",
                arg.name,
                count_tparams(hkargs.len()),
                param.name,
                hkparams.len()
            ));
            return;
        }
        for (hkarg, hkparam) in hkargs.iter().zip(hkparams) {
            if hkparam.nested.is_empty() && hkarg.nested.is_empty() {
                // Base case, kind `*`: variances must match (an invariant
                // expectation accepts anything), and the expected bounds must
                // be at least as strict as the argument's.
                let (a, p) = if flip {
                    (hkparam, hkarg)
                } else {
                    (hkarg, hkparam)
                };
                if !(p.variance == 0 || a.variance == p.variance) {
                    errs.variance.push(format!(
                        "type {} is {}, but type {} is declared {}",
                        a.name,
                        variance_word(a.variance),
                        p.name,
                        variance_word(p.variance)
                    ));
                }
                let declared = (
                    hkparam
                        .lo
                        .as_ref()
                        .map(|t| self.instantiate_declared(t, tparams, targs, under, with)),
                    hkparam
                        .hi
                        .as_ref()
                        .map(|t| self.instantiate_declared(t, tparams, targs, under, with)),
                );
                let argument = (hkarg.lo.clone(), hkarg.hi.clone());
                let ok = if flip {
                    self.bounds_conform(&argument, &declared)
                } else {
                    self.bounds_conform(&declared, &argument)
                };
                if !ok {
                    let (a_b, p_b) = if flip {
                        (&declared, &argument)
                    } else {
                        (&argument, &declared)
                    };
                    errs.strictness.push(format!(
                        "type {}'s bounds{} are stricter than type {}'s declared bounds{}",
                        a.name,
                        self.bounds_string(a_b, false),
                        p.name,
                        self.bounds_string(p_b, true)
                    ));
                }
            } else {
                let mark = under.len();
                for (p, a) in hkparam.nested.iter().zip(&hkarg.nested) {
                    if !p.sym.is_none() && !a.sym.is_none() {
                        under.push(p.sym);
                        with.push(Type::TypeParam(a.sym));
                    }
                }
                self.check_kind_hk(hkarg, hkparam, !flip, tparams, targs, under, with, errs);
                under.truncate(mark);
                with.truncate(mark);
            }
        }
    }

    /// A declared bound read at this instantiation: the enclosing
    /// definition's parameters become the arguments, and the higher-order
    /// parameters seen so far become the argument's own.
    fn instantiate_declared(
        &self,
        bound: &Type,
        tparams: &[SymbolId],
        targs: &[Type],
        under: &[SymbolId],
        with: &[Type],
    ) -> Type {
        let t = subst_tparams_slice(tparams, targs, bound);
        subst_tparams_slice(under, with, &t)
    }

    /// nsc `TypeBounds <:< TypeBounds`: `b1` is within `b2` when `b2.lo <:
    /// b1.lo` and `b1.hi <: b2.hi`. A bound that is unresolved, or that still
    /// mentions a type parameter after instantiation, is not judged: the
    /// answer would be a guess, and a wrong "no" rejects a valid program.
    fn bounds_conform(
        &self,
        b1: &(Option<Type>, Option<Type>),
        b2: &(Option<Type>, Option<Type>),
    ) -> bool {
        let judgeable = |t: &Type| !has_unresolved(t) && !mentions_any_tparam(t);
        let lo_ok = match (&b2.0, &b1.0) {
            (None, _) => true,
            (Some(lo2), lo1) => {
                let lo1 = lo1.clone().unwrap_or(Type::Nothing);
                lo2 == &lo1 || !judgeable(lo2) || !judgeable(&lo1) || self.st.is_sub_type(lo2, &lo1)
            }
        };
        let hi_ok = match (&b1.1, &b2.1) {
            (_, None) => true,
            (hi1, Some(hi2)) => {
                let hi1 = hi1.clone().unwrap_or(Type::Any);
                &hi1 == hi2
                    || matches!(hi2, Type::Any)
                    || !judgeable(&hi1)
                    || !judgeable(hi2)
                    || self.st.is_sub_type(&hi1, hi2)
            }
        };
        lo_ok && hi_ok
    }

    /// nsc prints bounds as ` >: Lo <: Hi`, leaving out the trivial halves;
    /// the *declared* side of a strictness message spells empty bounds out
    /// (` >: Nothing <: Any`) so the message makes sense.
    fn bounds_string(&self, b: &(Option<Type>, Option<Type>), spell_empty: bool) -> String {
        let mut s = String::new();
        if let Some(lo) = &b.0 {
            if !matches!(lo, Type::Nothing) {
                s.push_str(&format!(" >: {}", self.st.display_type(lo)));
            }
        }
        if let Some(hi) = &b.1 {
            if !matches!(hi, Type::Any) {
                s.push_str(&format!(" <: {}", self.st.display_type(hi)));
            }
        }
        if s.is_empty() && spell_empty {
            s.push_str(" >: Nothing <: Any");
        }
        s
    }

    /// The parameters of the constructor `ty` still expects, or `None` for a
    /// shape this check does not read (an unresolved name, a proper type).
    fn kparams_of_type(&self, ty: &Type) -> Option<Vec<KParam>> {
        match ty {
            Type::Annotated { tpe, .. } => self.kparams_of_type(tpe),
            Type::Refined { .. } if crate::prefix::view_prefix(ty).is_some() => {
                self.kparams_of_type(crate::prefix::strip_view(ty))
            }
            Type::Class { sym, args } if *sym == self.st.array_sym => Some(if args.is_empty() {
                vec![KParam {
                    name: "T".to_string(),
                    variance: 0,
                    lo: None,
                    hi: None,
                    sym: SymbolId::NONE,
                    nested: Vec::new(),
                }]
            } else {
                Vec::new()
            }),
            Type::Class { sym, args } => {
                let tps = self.st.get(*sym).tparams.clone();
                if args.len() > tps.len() {
                    return None;
                }
                let (applied, rest) = tps.split_at(args.len());
                Some(
                    rest.iter()
                        .map(|tp| self.kparam_of_sym(*tp, applied, args))
                        .collect(),
                )
            }
            Type::TypeParam(id) | Type::TypeMember(id) => Some(
                self.st
                    .get(*id)
                    .tparams
                    .iter()
                    .map(|tp| self.kparam_of_sym(*tp, &[], &[]))
                    .collect(),
            ),
            Type::Applied { ctor, args } => {
                let (id, pre): (SymbolId, Vec<Type>) = match ctor.as_ref() {
                    Type::TypeParam(id) | Type::TypeMember(id) => (*id, Vec::new()),
                    _ => return None,
                };
                let _ = pre;
                let tps = self.st.get(id).tparams.clone();
                if args.len() > tps.len() {
                    return None;
                }
                let (applied, rest) = tps.split_at(args.len());
                Some(
                    rest.iter()
                        .map(|tp| self.kparam_of_sym(*tp, applied, args))
                        .collect(),
                )
            }
            _ => None,
        }
    }

    /// A type parameter symbol as a `KParam`, its bounds read with the
    /// constructor's already-applied parameters substituted.
    fn kparam_of_sym(&self, tp: SymbolId, applied: &[SymbolId], args: &[Type]) -> KParam {
        let s = self.st.get(tp);
        let read = |b: &Option<Type>| {
            b.as_ref().map(|t| {
                if applied.is_empty() {
                    t.clone()
                } else {
                    subst_tparams_slice(applied, args, t)
                }
            })
        };
        // A Java class file's `T` is `<: Object`, which is no bound in Scala
        // (`jv[java.util.ArrayList]` is fine for `F[_]`).
        let hi = read(&s.bound_hi).filter(|t| {
            !(s.flags.contains(Flags::JAVA) && matches!(t, Type::JavaObject | Type::AnyRef))
        });
        KParam {
            name: s.name.clone(),
            variance: variance_of(s.flags),
            lo: read(&s.bound_lo),
            hi,
            sym: tp,
            nested: s
                .tparams
                .iter()
                .map(|n| self.kparam_of_sym(*n, applied, args))
                .collect(),
        }
    }

    // ---------------------------------------------------------------------
    // 2. variance of a type lambda's / higher-kinded member's own parameters
    // ---------------------------------------------------------------------

    /// nsc `validateDefinition(base)` for a type member with its own type
    /// parameters: each declared-variant parameter must occur at that
    /// variance in the right-hand side (an alias or a type lambda) and in the
    /// upper bound, and -- nsc's one special case -- unflipped in the lower
    /// bound. `desc` is how nsc names the definition (`type l`, or `value
    /// <local l>` for a refinement written in a term's type).
    pub(crate) fn check_hk_member_own_variance(
        &mut self,
        tparams: &[SymbolId],
        rhs: Option<&Type>,
        lo: Option<&Type>,
        hi: Option<&Type>,
        span: Span,
        desc: &str,
    ) {
        let vars: Vec<(SymbolId, i8, String)> = tparams
            .iter()
            .map(|tp| {
                let s = self.st.get(*tp);
                (*tp, variance_of(s.flags), s.name.clone())
            })
            .collect();
        if vars.iter().all(|(_, v, _)| *v == 0) {
            return;
        }
        if rhs.is_some_and(has_unresolved)
            || lo.is_some_and(has_unresolved)
            || hi.is_some_and(has_unresolved)
        {
            return;
        }
        let shown = self.poly_type_display(&vars, rhs, lo, hi);
        for t in [rhs, lo, hi].into_iter().flatten() {
            self.check_variance_ty_in(&vars, t, 1, span, desc, Some(&shown));
        }
    }

    /// nsc's spelling of a higher-kinded member's info: `[-a]List[a]`,
    /// `[-x] >: List[x]`, `[+x] >: L <: H`.
    fn poly_type_display(
        &self,
        vars: &[(SymbolId, i8, String)],
        rhs: Option<&Type>,
        lo: Option<&Type>,
        hi: Option<&Type>,
    ) -> String {
        let params = vars
            .iter()
            .map(|(_, v, n)| format!("{}{n}", variance_sign(*v)))
            .collect::<Vec<_>>()
            .join(",");
        let body = match rhs {
            Some(t) => self.st.display_type(t),
            None => self.bounds_string(&(lo.cloned(), hi.cloned()), false),
        };
        format!("[{params}]{body}")
    }

    /// The class-parameter half of `validateDefinition` for a type member of
    /// a class (`check_class_variance` handles the terms): an alias's
    /// right-hand side is an invariant position for the class's parameters,
    /// an abstract member's upper bound a covariant one and its lower bound a
    /// contravariant one. `private[this]` members are exempt, as in nsc.
    pub(crate) fn check_type_member_class_variance(
        &mut self,
        vars: &[(SymbolId, i8, String)],
        member: SymbolId,
        span: Span,
    ) {
        let s = self.st.get(member);
        if s.kind != SymKind::TypeMember
            || s.flags.contains(Flags::SYNTHETIC)
            || s.flags.contains(Flags::LOCAL)
        {
            return;
        }
        let name = s.name.clone();
        let own: Vec<(SymbolId, i8, String)> = s
            .tparams
            .iter()
            .map(|tp| {
                let t = self.st.get(*tp);
                (*tp, variance_of(t.flags), t.name.clone())
            })
            .collect();
        let lo = s.bound_lo.clone();
        let hi = s.bound_hi.clone();
        let rhs = if s.is_type_alias && !matches!(s.ty, Type::TypeMember(_)) {
            Some(s.ty.clone())
        } else {
            None
        };
        if rhs.as_ref().is_some_and(has_unresolved)
            || lo.as_ref().is_some_and(has_unresolved)
            || hi.as_ref().is_some_and(has_unresolved)
        {
            return;
        }
        let desc = format!("type {name}");
        let shown = if own.is_empty() {
            match &rhs {
                Some(t) => self.st.display_type(t),
                None => self.bounds_string(&(lo.clone(), hi.clone()), false),
            }
        } else {
            self.poly_type_display(&own, rhs.as_ref(), lo.as_ref(), hi.as_ref())
        };
        if let Some(t) = &rhs {
            self.check_variance_ty_in(vars, t, 0, span, &desc, Some(&shown));
        }
        if let Some(t) = &hi {
            self.check_variance_ty_in(vars, t, 1, span, &desc, Some(&shown));
        }
        if let Some(t) = &lo {
            self.check_variance_ty_in(vars, t, -1, span, &desc, Some(&shown));
        }
    }
}
