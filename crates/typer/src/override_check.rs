//! SLS 5.1.4 "Overriding" — nsc's `RefChecks.checkOverride`, plus the
//! "needs to be abstract" check that goes with it.
//!
//! Until this module existed scala-rs had **no** conformance check on an
//! override at all, so
//!
//! ```scala
//! trait It[A] { def next(): A }
//! val i = new It[Int] { def next(): String = "x" }
//! ```
//!
//! type-checked and then threw `ClassCastException` at the caller's unbox.
//! Likewise a class that simply forgot to implement an abstract member
//! compiled and threw `AbstractMethodError`.
//!
//! Everything here is a pure function of the symbol table, so `check.rs` only
//! grows call sites — the same shape as `traitparent.rs`. Every diagnostic
//! string was read off real scalac 2.13.16 (see
//! `crates/cli/tests/override.rs`), not guessed from `javap`.
//!
//! ## Deliberate conservatism
//!
//! This slice only ever *adds* rejections, so a false diagnostic breaks working
//! code while a missed one is merely a gap. Three predicates keep it on the
//! safe side, and between them they took slick's 184 files from 502 errors back
//! to the 346 it reported before the checks existed — the same multiset:
//!
//! * [`uncertain`] — a type the signature pass never filled in (`NoType`), an
//!   unresolved `Type::Named`, an `Error` already reported.
//! * [`robust`] — a type scala-rs's own subtyping compares only approximately:
//!   a type parameter, an abstract type member, an unreduced application, a
//!   wildcard, a path-dependent head. Comparisons over those say "matches".
//! * [`modifiers_are_known`] — a symbol whose flag word really reflects its
//!   source. The prelude marks every member `FINAL` and can say nothing about
//!   `DEFERRED`; `PickleSupply` allocates members `Flags::EMPTY`. `final` and
//!   "needs to be abstract" are therefore checked only against source and Java
//!   class-file members.

use crate::lin::{is_interface, linearize};
use crate::symbol::{subst_tparams_slice, SymKind, SymbolTable};
use scala_rs_parser::{Flags, SymbolId, Type};

/// One diagnostic, attached to the member symbol it is about so the caller can
/// find that member's span in the template body (scalac points at the member,
/// not at the class).
pub struct OverrideError {
    /// The offending member of the class being checked.
    pub sym: SymbolId,
    pub message: String,
}

// ---------------------------------------------------------------- predicates

/// A term member of a template: a `def`, a `val`/`var`, or the field a `val`
/// constructor parameter becomes. Not a constructor, not a bridge, not a type
/// or a class. A bare constructor parameter is `private[this]` and so is
/// dropped later, by `is_private_to_owner`.
fn is_member_like(st: &SymbolTable, m: SymbolId) -> bool {
    let s = st.get(m);
    if !matches!(s.kind, SymKind::Method | SymKind::Term) {
        return false;
    }
    if s.name == "<init>" || s.name.contains('$') {
        return false;
    }
    let f = s.flags;
    !(f.contains(Flags::CONSTRUCTOR)
        || f.contains(Flags::BRIDGE)
        || f.contains(Flags::MODULE)
        || f.contains(Flags::PACKAGE))
}

/// A member the *source* wrote, and so one the override rules have something
/// to say about. A case class's `productArity` / `copy` / `equals` are the
/// compiler's, not the programmer's: scalac never asks them for an `override`
/// modifier, and reporting them would make every case class an error.
fn is_overridable_kind(st: &SymbolTable, m: SymbolId) -> bool {
    is_member_like(st, m) && !st.get(m).flags.contains(Flags::SYNTHETIC)
}

/// nsc: a `private` member is not inherited, so it is never overridden and can
/// never be the thing an `override` refers to. `private[C]` *is* inherited.
fn is_private_to_owner(st: &SymbolTable, m: SymbolId) -> bool {
    let s = st.get(m);
    s.flags.contains(Flags::PRIVATE) && s.private_within.is_none()
}

/// nsc `DEFERRED`: no implementation here. A `def` without a body carries
/// `ABSTRACT` (the namer sets it); a `val`/`var` without one carries
/// [`crate::symbol::Symbol::deferred_val`]. `abstract override` is *not*
/// deferred in this sense — `traitparent::check_abstract_override_grounded`
/// owns that rule, and reporting it here too would double-count.
fn is_deferred(st: &SymbolTable, m: SymbolId) -> bool {
    let s = st.get(m);
    if s.abstract_override {
        return false;
    }
    match s.kind {
        SymKind::Method => s.flags.contains(Flags::ABSTRACT),
        SymKind::Term => s.deferred_val,
        _ => false,
    }
}

/// Whether `m`'s modifier flags say what its source said.
///
/// For two large groups of symbols they do not, and both would produce
/// diagnostics scalac does not:
///
/// * **prelude symbols.** `prelude::method` stamps *every* member
///   `Flags::FINAL`, so `override def toString` looked like overriding a final
///   member; and it has no way to spell "deferred", so `Product.productArity`
///   — genuinely abstract — looked concrete and every hand-written `Product`
///   was asked for an `override` modifier.
/// * **members completed from a library pickle.** `PickleSupply` allocates
///   them `Flags::EMPTY`: the pickle's `DEFERRED` and `FINAL` bits are not
///   carried across, so neither may be read off the symbol.
///
/// Java class files *are* read faithfully (`classpath.rs` sets `ABSTRACT`), and
/// so is everything this run's own sources declare. `final` and "needs to be
/// abstract" therefore hold for those and are withheld for the rest — a gap,
/// not a wrong answer.
fn modifiers_are_known(st: &SymbolTable, m: SymbolId) -> bool {
    m.0 >= st.prelude_end && st.get(m).pickled_origin.is_empty()
}

/// nsc's `OverridingPairs.Cursor.exclude`: a **bare** constructor parameter is
/// `private[this]` and is not a member at all, so it neither overrides nor
/// implements anything. slick's `class JdbcFunction(name: String) extends
/// FunctionSymbol(name)` is the shape — scalac accepts it, and without this it
/// was told to write `override`. A `private[this] val` the programmer actually
/// wrote is *not* excluded: scalac reports that one.
fn is_bare_ctor_param(st: &SymbolTable, m: SymbolId) -> bool {
    let s = st.get(m);
    s.flags.contains(Flags::PARAM)
        && s.flags.contains(Flags::PRIVATE)
        && s.flags.contains(Flags::LOCAL)
}

/// A member of `owner`, as opposed to a symbol that merely landed in its
/// member list.
///
/// A lambda parameter written inside a field initialiser is allocated with the
/// *class* as its owner — `protected lazy val pkNames = pkSyms.map { fs => … }`
/// puts an `fs` in slick's `UpsertBuilder`, where it collided with the `fs` of
/// a base class and asked for an `override` modifier. Only the constructor
/// parameters the class actually turns into fields count as members here; a
/// `PARAM` symbol that is not one of those belongs to a method or a lambda.
fn is_member_of(st: &SymbolTable, owner: SymbolId, m: SymbolId) -> bool {
    if !is_member_like(st, m) {
        return false;
    }
    !st.get(m).flags.contains(Flags::PARAM) || st.get(owner).ctor_fields.contains(&m)
}

/// A type this compiler compares reliably: no type parameter, no abstract type
/// member, no unreduced application, no wildcard, no path-dependent head.
///
/// scala-rs's subtyping is an approximation over exactly those, and slick is
/// full of them — `ClassTag[C[Any]]` against `ClassTag[_]`,
/// `Builder[E, C[E]]`, `BasicBackend.Session`. Comparing them produced 150
/// diagnostics scalac does not report. Where a comparison cannot be trusted
/// this module says nothing; see the module header.
fn robust(ty: &Type) -> bool {
    let mut ok = true;
    walk_type(ty, &mut |t| {
        if matches!(
            t,
            Type::TypeParam(_)
                | Type::TypeMember(_)
                | Type::Applied { .. }
                | Type::Wildcard
                | Type::BoundedWildcard { .. }
                | Type::Refined { .. }
                | Type::SingleType { .. }
                | Type::ThisType(_)
                | Type::Annotated { .. }
        ) {
            ok = false;
        }
    });
    ok && !uncertain(ty)
}

/// Access strength, larger is more restrictive. `private[this]` is stricter
/// than `private[C]`, which nsc still reports as "should not be private".
fn access_level(st: &SymbolTable, m: SymbolId) -> u8 {
    let s = st.get(m);
    if s.flags.contains(Flags::PRIVATE) {
        if s.flags.contains(Flags::LOCAL) {
            4
        } else {
            3
        }
    } else if s.flags.contains(Flags::PROTECTED) {
        2
    } else {
        0
    }
}

/// A type no comparison should be trusted on: an unresolved name, a slot the
/// signature pass never filled, an error already reported. See the module
/// header — every check bails out on these rather than risk a false reject.
fn uncertain(ty: &Type) -> bool {
    let mut bad = false;
    walk_type(ty, &mut |t| {
        if matches!(
            t,
            Type::NoType | Type::Error | Type::Named { .. } | Type::Overload(_)
        ) {
            bad = true;
        }
    });
    bad
}

fn walk_type(ty: &Type, f: &mut impl FnMut(&Type)) {
    f(ty);
    match ty {
        Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => walk_type(t, f),
        Type::Tuple(ts) | Type::Overload(ts) => ts.iter().for_each(|t| walk_type(t, f)),
        Type::Function { params, ret } => {
            params.iter().for_each(|t| walk_type(t, f));
            walk_type(ret, f);
        }
        Type::Named { args, .. } | Type::Class { args, .. } => {
            args.iter().for_each(|t| walk_type(t, f))
        }
        Type::Method { paramss, ret } => {
            paramss.iter().flatten().for_each(|t| walk_type(t, f));
            walk_type(ret, f);
        }
        Type::Applied { ctor, args } => {
            walk_type(ctor, f);
            args.iter().for_each(|t| walk_type(t, f));
        }
        Type::BoundedWildcard { lo, hi } => {
            if let Some(t) = lo {
                walk_type(t, f)
            }
            if let Some(t) = hi {
                walk_type(t, f)
            }
        }
        Type::SingleType { prefix, .. } => walk_type(prefix, f),
        Type::Annotated { tpe, .. } => walk_type(tpe, f),
        Type::Refined { parents, .. } => parents.iter().for_each(|t| walk_type(t, f)),
        _ => {}
    }
}

// ------------------------------------------------------------------ printing

/// `(x: Int)(y: Int)`, with the parameter names scalac echoes back. A nilary
/// `def f: T` prints nothing; `def f(): T` prints `()`.
///
/// scalac echoes a member's signature at the *overriding* site, so `def
/// next(): A` in `trait It[A]` prints as `def next(): Int` under
/// `new It[Int]`; `ty` is the signature read there.
fn show_params_of(st: &SymbolTable, m: SymbolId, ty: &Type) -> String {
    let s = st.get(m);
    let paramss = match ty {
        Type::Method { paramss, .. } => paramss.clone(),
        _ => return String::new(),
    };
    if norm_paramss(&paramss).is_empty() && paramss.len() <= 1 {
        return if paramss.is_empty() {
            String::new()
        } else {
            "()".to_string()
        };
    }
    let mut out = String::new();
    for (ci, clause) in paramss.iter().enumerate() {
        let ids = s.paramss.get(ci);
        let parts: Vec<String> = clause
            .iter()
            .enumerate()
            .map(|(i, t)| match ids.and_then(|v| v.get(i)) {
                Some(&pid) if !pid.is_none() => {
                    format!("{}: {}", st.get(pid).name, st.display_type(t))
                }
                _ => st.display_type(t),
            })
            .collect();
        out.push('(');
        out.push_str(&parts.join(", "));
        out.push(')');
    }
    out
}

/// Show method bounds as seen from `cls` when it is known. A bound such as
/// `A <: T` on a member of `BoundApply[T]` is `A <: String` at
/// `BoundApply[String]`, including in an override diagnostic.
fn show_tparams_at(st: &SymbolTable, m: SymbolId, cls: SymbolId) -> String {
    let tps = &st.get(m).tparams;
    if tps.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = tps
        .iter()
        .map(|&t| {
            let mut o = st.get(t).name.clone();
            if let Some(hi) = &st.get(t).bound_hi {
                let hi = if cls.is_none() {
                    hi.clone()
                } else {
                    let hi = st.subst_as_seen_from(&Type::ThisType(cls), hi);
                    st.expand_type_members(cls, &hi)
                };
                o.push_str(&format!(" <: {}", st.display_type(&hi)));
            }
            if let Some(lo) = &st.get(t).bound_lo {
                let lo = if cls.is_none() {
                    lo.clone()
                } else {
                    let lo = st.subst_as_seen_from(&Type::ThisType(cls), lo);
                    st.expand_type_members(cls, &lo)
                };
                o.push_str(&format!(" >: {}", st.display_type(&lo)));
            }
            o
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

fn result_type(st: &SymbolTable, m: SymbolId) -> Type {
    match &st.get(m).ty {
        Type::Method { ret, .. } => (**ret).clone(),
        other => other.clone(),
    }
}

/// nsc echoes the *accessor*, so a `val` prints as `val` (its getter is
/// stable) and a `var` prints as `def` (its getter is not):
/// `mutable variable cannot be overridden:` is followed by `def v: Int`.
fn keyword(st: &SymbolTable, m: SymbolId) -> &'static str {
    let s = st.get(m);
    match s.kind {
        SymKind::Method => "def",
        _ if s.flags.contains(Flags::MUTABLE) => "def",
        _ => "val",
    }
}

/// `final override def f: Int` — the form scalac echoes for the *overridden*
/// member, modifiers and all.
fn show_decl(st: &SymbolTable, m: SymbolId) -> String {
    show_decl_of(st, m, &st.get(m).ty)
}

/// `show_decl` for a signature read at an overriding site; see
/// [`show_params_of`].
fn show_decl_of(st: &SymbolTable, m: SymbolId, ty: &Type) -> String {
    show_decl_of_at(st, m, ty, SymbolId::NONE)
}

fn show_decl_of_at(st: &SymbolTable, m: SymbolId, ty: &Type, cls: SymbolId) -> String {
    let s = st.get(m);
    let mut out = String::new();
    if s.flags.contains(Flags::FINAL) {
        out.push_str("final ");
    }
    if s.abstract_override {
        out.push_str("abstract override ");
    } else if s.flags.contains(Flags::OVERRIDE) {
        out.push_str("override ");
    }
    out.push_str(keyword(st, m));
    out.push(' ');
    out.push_str(&s.name);
    out.push_str(&show_tparams_at(st, m, cls));
    out.push_str(&show_params_of(st, m, ty));
    out.push_str(": ");
    let ret = match ty {
        Type::Method { ret, .. } => (**ret).clone(),
        other => other.clone(),
    };
    out.push_str(&st.display_type(&ret));
    out
}

/// `trait It` / `class B10` / `object O`.
fn owner_phrase(st: &SymbolTable, owner: SymbolId) -> String {
    let word = if is_interface(st, owner) {
        "trait"
    } else if matches!(st.get(owner).kind, SymKind::ModuleClass | SymKind::Module) {
        "object"
    } else {
        "class"
    };
    format!("{word} {}", st.get(owner).name)
}

/// `(defined in trait It)`.
fn defined_in(st: &SymbolTable, owner: SymbolId) -> String {
    format!("(defined in {})", owner_phrase(st, owner))
}

fn show_sig_at(st: &SymbolTable, m: SymbolId, ty: &Type, ret: &Type, cls: SymbolId) -> String {
    let ps = format!(
        "{}{}",
        show_tparams_at(st, m, cls),
        show_params_of(st, m, ty)
    );
    if ps.is_empty() {
        st.display_type(ret)
    } else {
        format!("{ps}: {}", st.display_type(ret))
    }
}

// ------------------------------------------------------------------ matching

/// `def f: T` and `def f(): T` match, so a single empty clause is dropped.
fn norm_paramss(paramss: &[Vec<Type>]) -> Vec<Vec<Type>> {
    if paramss.len() == 1 && paramss[0].is_empty() {
        return Vec::new();
    }
    paramss.to_vec()
}

fn paramss_of(st: &SymbolTable, m: SymbolId) -> Vec<Vec<Type>> {
    match &st.get(m).ty {
        Type::Method { paramss, .. } => norm_paramss(paramss),
        _ => Vec::new(),
    }
}

/// The base member's type read at the overriding class: the base class's own
/// type parameters replaced by the arguments `cls` passes it, and the base
/// method's type parameters aligned with the override's.
fn base_type_at(st: &SymbolTable, cls: SymbolId, base: SymbolId, child: SymbolId) -> Type {
    let ty = st.get(base).ty.clone();
    let btps = st.get(base).tparams.clone();
    let ctps = st.get(child).tparams.clone();
    let ty = if !btps.is_empty() && btps.len() == ctps.len() {
        let args: Vec<Type> = ctps.iter().map(|&c| Type::TypeParam(c)).collect();
        subst_tparams_slice(&btps, &args, &ty)
    } else {
        ty
    };
    let ty = st.subst_as_seen_from(&Type::ThisType(cls), &ty);
    // `trait B { type T; def f: T }` implemented by `class D extends B { type
    // T = Int; def f: Int = 1 }`: the declaration is stated in an abstract type
    // member the override has since fixed, so it has to be read at `cls`.
    st.expand_type_members(cls, &ty)
}

/// `base_type_at` for a member with no overriding counterpart to align type
/// parameters with: the declaration read at `cls`, so `trait T[A] { def f(x:
/// A): A }` lists as `def f(x: Int): Int` under `class D extends T[Int]`.
fn member_type_at(st: &SymbolTable, cls: SymbolId, m: SymbolId) -> Type {
    let ty = st.subst_as_seen_from(&Type::ThisType(cls), &st.get(m).ty);
    st.expand_type_members(cls, &ty)
}

/// Two class types no instantiation can make equal, whatever type parameters
/// their arguments mention.
///
/// `robust` refuses to compare anything containing a type parameter, which is
/// the right default -- but it also throws away a comparison that needs no
/// argument at all. `C[X]` and `D[X]` are the same type only if `C` and `D`
/// are the same class, and one being a *strict* subclass of the other settles
/// that: strictness is what rules out the duplicate symbols the same classfile
/// can leave behind (two readings of one class are mutual subtypes, and this
/// says nothing about them).
///
/// cats writes the same method at each level of its type-class tower --
/// `Functor.compose[G[_]: Functor]`, then `Apply.compose[G[_]: Apply]`, then
/// `Applicative`, `Traverse`, `Reducible`, `Bitraverse`, ... -- with no
/// `override` on any of them, because a different implicit parameter makes
/// each one an *overload*. All nine were "`override` modifier required".
fn definitely_different(st: &SymbolTable, a: &Type, b: &Type) -> bool {
    let (Type::Class { sym: x, .. }, Type::Class { sym: y, .. }) = (a, b) else {
        return false;
    };
    if x == y {
        return false;
    }
    let bare = |s: SymbolId| Type::Class {
        sym: s,
        args: Vec::new(),
    };
    st.is_sub_type(&bare(*x), &bare(*y)) != st.is_sub_type(&bare(*y), &bare(*x))
}

fn same_type(st: &SymbolTable, a: &Type, b: &Type) -> bool {
    if a == b {
        return true;
    }
    if definitely_different(st, a, b) {
        return false;
    }
    if !robust(a) || !robust(b) {
        return true;
    }
    st.is_sub_type(a, b) && st.is_sub_type(b, a)
}

/// nsc `matchingSymbols`: same name, same shape, and parameter types that are
/// the *same* type (SLS: parameter types are invariant under overriding — a
/// different one is an overload, not an override). The result type is not part
/// of matching; that is what `incompatible type in overriding` reports.
fn matches(st: &SymbolTable, cls: SymbolId, child: SymbolId, base: SymbolId) -> bool {
    if st.get(child).name != st.get(base).name {
        return false;
    }
    if st.get(child).tparams.len() != st.get(base).tparams.len() {
        return false;
    }
    let cps = paramss_of(st, child);
    let bty = base_type_at(st, cls, base, child);
    let bps = match &bty {
        Type::Method { paramss, .. } => norm_paramss(paramss),
        _ => Vec::new(),
    };
    if cps.len() != bps.len() {
        return false;
    }
    for (c, b) in cps.iter().zip(bps.iter()) {
        if c.len() != b.len() {
            return false;
        }
        for (ct, bt) in c.iter().zip(b.iter()) {
            if !same_type(st, ct, bt) {
                return false;
            }
        }
    }
    true
}

/// `cls` and its ancestors, nearest first, **including** the universal classes.
///
/// `lin::linearize` leaves `Any` / `AnyRef` / `Object` out on purpose — the
/// backend's super accessors and mixin forwarders must not see them. Overriding
/// does: `override def toString: String` overrides `Any.toString`, and without
/// them every class that writes one was told it "overrides nothing".
fn full_lin(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    let mut out = linearize(st, cls);
    for u in [st.object_sym, st.anyref_sym, st.any_sym] {
        if !u.is_none() && !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

/// A `case class` / `case object` really does extend `scala.Product with
/// Equals`, but `prelude_product` only adds that edge in library-ABI mode —
/// the private runtime ships no `scala.Product` classfile, so there is nothing
/// to point at. A member overriding one of `Product`'s is then "overriding
/// nothing" in our model and not in scalac's, so that single report is
/// withheld for such a class. Every other rule still applies to it.
fn product_edge_missing(st: &SymbolTable, cls: SymbolId) -> bool {
    crate::prelude_product::wants_product(st, cls)
        && !full_lin(st, cls)
            .iter()
            .any(|&b| st.get(b).name == "Product")
}

/// Every strict base class of `cls`, nearest first.
fn strict_bases(st: &SymbolTable, cls: SymbolId) -> Vec<SymbolId> {
    full_lin(st, cls).into_iter().skip(1).collect()
}

/// The members of `cls`'s base classes that `child` overrides.
fn overridden(st: &SymbolTable, cls: SymbolId, child: SymbolId) -> Vec<SymbolId> {
    let name = st.get(child).name.clone();
    let mut out = Vec::new();
    for base in strict_bases(st, cls) {
        for &m in &st.get(base).members {
            if st.get(m).owner != base || st.get(m).name != name {
                continue;
            }
            if !is_overridable_kind(st, m)
                || !is_member_of(st, base, m)
                || is_private_to_owner(st, m)
            {
                continue;
            }
            if matches(st, cls, child, m) {
                out.push(m);
            }
        }
    }
    out
}

/// Same-named, non-final base members — the "Note:" scalac appends to
/// `overrides nothing` when the name exists but nothing matched.
fn same_named(st: &SymbolTable, cls: SymbolId, name: &str) -> Vec<SymbolId> {
    let mut out = Vec::new();
    for base in strict_bases(st, cls) {
        for &m in &st.get(base).members {
            if st.get(m).owner != base || st.get(m).name != name {
                continue;
            }
            if !is_overridable_kind(st, m)
                || !is_member_of(st, base, m)
                || is_private_to_owner(st, m)
            {
                continue;
            }
            if st.get(m).flags.contains(Flags::FINAL) && modifiers_are_known(st, m) {
                continue;
            }
            out.push(m);
        }
    }
    out
}

// -------------------------------------------------------------- the two rules

/// SLS 5.1.4. One diagnostic per offending member, in nsc's own order: a
/// member that fails several rules is reported once, by the first that fires.
pub fn check_overrides(
    st: &SymbolTable,
    cls: SymbolId,
    inferred_result: &dyn Fn(SymbolId) -> bool,
) -> Vec<OverrideError> {
    let mut out = Vec::new();
    if cls.is_none() {
        return out;
    }
    for &child in &st.get(cls).members {
        if st.get(child).owner != cls
            || !is_overridable_kind(st, child)
            || !is_member_of(st, cls, child)
        {
            continue;
        }
        if is_bare_ctor_param(st, child) {
            continue;
        }
        // A signature that never completed cannot be compared against
        // anything; some other diagnostic already owns that.
        if uncertain(&st.get(child).ty) {
            continue;
        }
        let bases = overridden(st, cls, child);
        if bases.is_empty() {
            let name = st.get(child).name.clone();
            // `overrides nothing` is only sound when every same-named member
            // above the class is one whose signature we read faithfully. The
            // prelude's `Iterator.map` is a monomorphic stand-in for
            // `map[B](f: A => B)`, so slick's `override def map[B]` matched
            // nothing and was rejected; a pickled member is no better, since
            // `PickleSupply` drops overloads it cannot key. Where an
            // approximation is in the way, say nothing.
            let notes = same_named(st, cls, &name);
            let approximated = notes.iter().any(|&m| !modifiers_are_known(st, m));
            if st.get(child).flags.contains(Flags::OVERRIDE)
                && !product_edge_missing(st, cls)
                && !approximated
            {
                let word = if st.get(child).kind == SymKind::Method {
                    "method"
                } else {
                    "value"
                };
                let msg = if notes.is_empty() {
                    format!("{word} {name} overrides nothing")
                } else {
                    let decls: Vec<String> = notes.iter().map(|&m| show_decl(st, m)).collect();
                    format!(
                        "{word} {name} overrides nothing.\nNote: the super classes of {} {} contain the following, non final members named {name}:\n{}",
                        if is_interface(st, cls) { "trait" } else { "class" },
                        st.get(cls).name,
                        decls.join("\n")
                    )
                };
                out.push(OverrideError {
                    sym: child,
                    message: msg,
                });
            }
            continue;
        }
        for &base in &bases {
            if let Some(msg) = check_pair(st, cls, child, base, inferred_result) {
                out.push(OverrideError {
                    sym: child,
                    message: msg,
                });
                break;
            }
        }
    }
    out
}

/// The rules that apply once `child` really does override `base`, in the order
/// nsc reports them.
fn check_pair(
    st: &SymbolTable,
    cls: SymbolId,
    child: SymbolId,
    base: SymbolId,
    inferred_result: &dyn Fn(SymbolId) -> bool,
) -> Option<String> {
    // scalac echoes the overridden member *as the overriding class sees it*:
    // `def next(): A` in `trait It[A]` is reported as `def next(): Int` at
    // `new It[Int] { … }`.
    let base_ty = base_type_at(st, cls, base, child);
    let decl = format!(
        "{} {}",
        show_decl_of_at(st, base, &base_ty, cls),
        defined_in(st, st.get(base).owner)
    );
    let base_deferred = is_deferred(st, base);
    let child_deferred = is_deferred(st, child);

    // 5. `final` members are closed.
    if st.get(base).flags.contains(Flags::FINAL) && modifiers_are_known(st, base) {
        return Some(format!("cannot override final member:\n{decl}"));
    }

    // 5a. A macro def may only be redefined by another macro def. nsc's rule:
    //     the base has no bytecode, so the override would be the only
    //     implementation of a member every call site expands away instead of
    //     calling -- the dynamic dispatch the `override` asks for cannot
    //     happen. Only the direction the base's macro-ness makes certain is
    //     checked here; `macro_impl` is set from the source `= macro Impl.m`
    //     and from a pickled `@macroImpl`, never guessed.
    if st.get(base).macro_impl.is_some() && st.get(child).macro_impl.is_none() {
        return Some(format!(
            "macro can only be overridden by another macro:\nmacro {decl}"
        ));
    }

    // 6. Visibility may only widen. Checked *before* the missing `override`
    //    modifier: scalac reports `private[this] def f` over a public concrete
    //    `f` as a visibility error, not as a missing modifier.
    let (cl, bl) = (access_level(st, child), access_level(st, base));
    if cl > bl {
        // scalac names the *offence* when the override is private at all
        // ("override should not be private", whatever the base was) and the
        // required level otherwise.
        let want = if cl >= 3 {
            "  override should not be private"
        } else if bl == 0 {
            "  override should be public"
        } else {
            "  override should be protected"
        };
        return Some(format!(
            "weaker access privileges in overriding\n{decl}\n{want}"
        ));
    }

    // 3. `override` is required to redefine a concrete member. Re-*declaring*
    //    one as deferred in an abstract class is not an override (scalac
    //    accepts `abstract class D extends B { def f: Int }`).
    if !base_deferred
        && modifiers_are_known(st, base)
        && !child_deferred
        && !st.get(child).flags.contains(Flags::OVERRIDE)
        && !st.get(child).abstract_override
    {
        return Some(format!(
            "`override` modifier required to override concrete member:\n{decl}"
        ));
    }

    // 7. `val` may override `def`, never the reverse; a concrete `var` is not
    //    overridable at all.
    if st.get(base).kind == SymKind::Term {
        if st.get(base).flags.contains(Flags::MUTABLE) && !base_deferred {
            return Some(format!("mutable variable cannot be overridden:\n{decl}"));
        }
        if st.get(child).kind == SymKind::Method {
            return Some(format!(
                "stable, immutable value required to override:\n{decl}"
            ));
        }
    }

    // 1. / 8. The result type is covariant, and a type parameter's bound may
    //    only widen (`[A]` may override `[A <: AnyRef]`, not the reverse).
    let brt = match &base_ty {
        Type::Method { ret, .. } => (**ret).clone(),
        other => other.clone(),
    };
    let crt = result_type(st, child);
    let mismatch = || {
        format!(
            "incompatible type in overriding\n{decl};\n found   : {}\n required: {}",
            show_sig_at(st, child, &st.get(child).ty, &crt, cls),
            show_sig_at(st, base, &base_ty, &brt, cls)
        )
    };
    // 8. A type parameter's bound may only widen. This is checked whatever the
    //    result type looks like: `def f[A](x: A): A` returns a type parameter,
    //    which the conformance test below cannot compare.
    if !tparam_bounds_ok(st, cls, child, base) {
        return Some(mismatch());
    }
    // nsc's namer types a member with no written result type *at* the
    // overridden member's result type, so an inferred one can never fail to
    // conform. Ours is only an inference, and slick's
    // `override def toString = { … }` came out `Any`; comparing it would
    // reject working code over a gap that is not the programmer's.
    if inferred_result(child) {
        return None;
    }
    if !robust(&brt) || !robust(&crt) {
        return None;
    }
    if st.is_sub_type(&crt, &brt) {
        return None;
    }
    Some(mismatch())
}

/// The override's type parameters must accept at least what the base's do:
/// scalac takes `override def f[A]` over `def f[A <: AnyRef]` and rejects the
/// reverse. Only stated upper bounds are compared; an unstated one is `Any`.
/// Bounds from a generic owner are read through the overriding class, so the
/// `T` in `BoundApply[T]` becomes the argument in `BoundApply[String]` before
/// the method bounds are compared.
fn tparam_bounds_ok(st: &SymbolTable, cls: SymbolId, child: SymbolId, base: SymbolId) -> bool {
    let btps = st.get(base).tparams.clone();
    let ctps = st.get(child).tparams.clone();
    if btps.len() != ctps.len() {
        return true;
    }
    let args: Vec<Type> = ctps.iter().map(|&c| Type::TypeParam(c)).collect();
    for (&b, &c) in btps.iter().zip(ctps.iter()) {
        let bhi = st.get(b).bound_hi.clone();
        let chi = st.get(c).bound_hi.clone();
        let (Some(chi), bhi) = (chi, bhi) else {
            continue;
        };
        let bhi = match bhi {
            Some(h) => subst_tparams_slice(&btps, &args, &h),
            None => Type::Any,
        };
        let bhi = st.subst_as_seen_from(&Type::ThisType(cls), &bhi);
        if uncertain(&bhi) || uncertain(&chi) {
            continue;
        }
        if !st.is_sub_type(&bhi, &chi) {
            return false;
        }
    }
    true
}

/// A `-cp` / jar **Java** interface — not a Scala `trait` read from a pickle.
fn is_java_interface(st: &SymbolTable, id: SymbolId) -> bool {
    !id.is_none() && st.get(id).flags.contains(Flags::JAVA) && is_interface(st, id)
}

/// SLS 5.2.6: a concrete class (or object, or `new C { }`) must implement every
/// deferred member it inherits, or the JVM throws `AbstractMethodError` at the
/// first call. scalac 2.13.16:
///
/// ```text
/// class D10 needs to be abstract.
/// Missing implementation for member of class B10:
///   def f: Int = ???
/// ```
///
/// `headline` is the first line — `object creation impossible.` for an object
/// or an anonymous class, `class C needs to be abstract.` for a class.
pub fn check_missing_implementations(
    st: &SymbolTable,
    cls: SymbolId,
    headline: &str,
) -> Option<String> {
    if cls.is_none() {
        return None;
    }
    let lin = full_lin(st, cls);
    let universals: Vec<SymbolId> = [st.object_sym, st.anyref_sym, st.any_sym]
        .into_iter()
        .filter(|s| !s.is_none() && *s != cls)
        .collect();
    let mut missing: Vec<SymbolId> = Vec::new();
    let mut seen_names: Vec<(String, usize)> = Vec::new();
    for (bi, &base) in lin.iter().enumerate() {
        for &m in &st.get(base).members {
            if st.get(m).owner != base
                || !is_overridable_kind(st, m)
                || !is_member_of(st, base, m)
                || !is_deferred(st, m)
            {
                continue;
            }
            if is_private_to_owner(st, m) {
                continue;
            }
            // A signature that never completed would be reported by whichever
            // check owns it; guessing here would be a false positive.
            if uncertain(&st.get(m).ty) {
                continue;
            }
            let key = (st.get(m).name.clone(), paramss_of(st, m).len());
            if seen_names.contains(&key) {
                continue;
            }
            // A `var` is one member here and two in nsc: `class Cell(var c:
            // Int) extends Counter` implements `def c: Int` *and*
            // `def c_=(v: Int): Unit`, but scala-rs models the whole `var` as
            // a single mutable `Term` named `c`, so the setter has nothing to
            // match against.
            if let Some(getter) = st.get(m).name.strip_suffix("_=") {
                let has_var = lin.iter().any(|&b| {
                    st.get(b).members.iter().any(|&c| {
                        st.get(c).owner == b
                            && st.get(c).kind == SymKind::Term
                            && st.get(c).name == getter
                            && st.get(c).flags.contains(Flags::MUTABLE)
                            && !is_deferred(st, c)
                    })
                });
                if has_var {
                    continue;
                }
            }
            // Only a *more derived* class can implement it. A deferred
            // redeclaration un-implements what is below it:
            // `class B { def f: Int = 1 }`,
            // `abstract class M extends B { override def f: Int }`,
            // `class C extends M` — scalac says `C needs to be abstract`,
            // because `M`'s declaration is what `C` inherits.
            //
            // The universal classes are the exception: `full_lin` appends
            // `Object` / `AnyRef` / `Any` *after* every real base so the
            // backend's mixin machinery never sees them, but on the JVM they
            // are the class's ultimate superclass and their concrete members
            // implement anything a trait or a Java interface re-declares
            // deferred. `trait T { def hashCode(): Int }; class D extends T`
            // compiles under scalac, and so does every Java interface that
            // re-declares `equals`/`hashCode` (JLS 9.2) —
            // `java.util.Map`, `java.util.Map.Entry`, `java.util.List`, …
            //
            // A *Java interface*'s deferred member is the same story one level
            // down. Java has no `abstract override`, so an interface can never
            // un-implement anything: whatever the superclass chain defines is
            // what the JVM resolves, wherever the interface happens to sit in
            // the linearization. `java.util.List` re-declares
            // `containsAll(Collection[_])` that `java.util.Collection` already
            // declares, `java.util.AbstractCollection` defines it, and SLS
            // 5.1.2 orders `AbstractCollection` *after* `List` for
            // `java.util.ArrayList` — so `class C extends
            // java.util.ArrayList[String]` was told to be abstract over a
            // method it plainly inherits. Only non-interface bases count: an
            // interface that redeclares a *superinterface*'s default method as
            // abstract really does leave it unimplemented.
            let java_iface_decl = is_java_interface(st, st.get(m).owner);
            let later_class_impl = java_iface_decl
                && lin[bi + 1..].iter().any(|&b| {
                    !is_interface(st, b)
                        && st.get(b).members.iter().any(|&c| {
                            st.get(c).owner == b
                                && c != m
                                && is_member_of(st, b, c)
                                && !is_private_to_owner(st, c)
                                && !is_deferred(st, c)
                                && matches(st, cls, c, m)
                        })
                });
            if later_class_impl {
                continue;
            }
            // A plain declaration is implemented by a matching definition
            // *anywhere* in the linearization, not only above it. nsc's
            // `findMember` drops a deferred symbol as soon as a concrete one
            // matches it, and the order only decides which concrete member
            // wins. gitbucket writes
            //
            // ```scala
            // trait Profile { val profile: BlockingJdbcProfile }
            // trait ProfileProvider { self: Profile =>
            //   lazy val profile = DatabaseConfig.slickDriver }
            // object Profile extends ProfileProvider with Profile
            // ```
            //
            // so the definition sits *after* the declaration in the
            // linearization, and this reported `object creation impossible.`
            // for a program scalac accepts.
            //
            // The `override`-marked declaration keeps the narrow rule: it is
            // the one shape that takes an implementation away (see above), so
            // only a base above it can put one back.
            let visible: Vec<SymbolId> = if st.get(m).flags.contains(Flags::OVERRIDE) {
                lin[..bi].to_vec()
            } else {
                lin.iter().copied().filter(|&b| b != base).collect()
            };
            let implemented = visible.iter().chain(universals.iter()).any(|&b| {
                st.get(b).members.iter().any(|&c| {
                    st.get(c).owner == b
                        && c != m
                        && is_member_of(st, b, c)
                        && !is_private_to_owner(st, c)
                        && !is_deferred(st, c)
                        && !st.get(c).abstract_override
                        && matches(st, cls, c, m)
                })
            });
            if implemented {
                continue;
            }
            seen_names.push(key);
            missing.push(m);
        }
    }
    if missing.is_empty() {
        return None;
    }
    // A member declared `override def f: Int` with no body does not *add* an
    // abstract member, it takes one away; scalac words that case differently.
    if missing
        .iter()
        .all(|&m| st.get(m).flags.contains(Flags::OVERRIDE))
    {
        let body: Vec<String> = missing
            .iter()
            .map(|&m| format!("{} {}", show_decl(st, m), defined_in(st, st.get(m).owner)))
            .collect();
        return Some(format!(
            "{headline}\nNo implementation found in a subclass for deferred declaration\n{}",
            body.join("\n")
        ));
    }
    let owners: Vec<SymbolId> = {
        let mut v: Vec<SymbolId> = missing.iter().map(|&m| st.get(m).owner).collect();
        v.dedup();
        v
    };
    let lead = if missing.len() == 1 {
        format!(
            "Missing implementation for member of {}:",
            owner_phrase(st, owners[0])
        )
    } else if owners.len() == 1 {
        format!(
            "Missing implementations for {} members of {}.",
            missing.len(),
            owner_phrase(st, owners[0])
        )
    } else {
        format!("Missing implementations for {} members.", missing.len())
    };
    let body: Vec<String> = missing
        .iter()
        .map(|&m| {
            format!(
                "  {} = ???",
                show_decl_of(st, m, &member_type_at(st, cls, m))
            )
        })
        .collect();
    Some(format!("{headline}\n{lead}\n{}", body.join("\n")))
}
