//! Turn a parsed pickle into class signatures, and resolve members across
//! inheritance by loading the pickles of parent classes on demand.
//!
//! This is the bridge layer between [`crate::read`] (bytes -> entries)
//! and a symbol table: entry indices are resolved into names and a
//! self-contained [`SigType`] tree, and [`SigLoader`] walks parents so that
//! `List#filter` is found on `scala.collection.IterableOps` without anyone
//! having to say where it lives.
//!
//! It stops short of the typer's `Type`: the last hop
//! (`SigType` -> `scala_rs_parser::Type`) lives in
//! `crates/typer/src/pickle_supply.rs`, which is where the symbol table is.

use std::collections::HashMap;
use std::rc::Rc;

use crate::read::{pflags, read_pickle, Constant, Entry, Idx, Pickle, ReadError};

/// A type recovered from a pickle, with all entry references resolved.
#[derive(Clone, Debug, PartialEq)]
pub enum SigType {
    /// `NOtpe` / `NOPREFIXtpe`, and the result of a reference we could not
    /// resolve (always reported rather than silently dropped, see
    /// [`ClassSig::unresolved`]).
    None,
    /// `C.this.type`.
    This(String),
    /// `p.x.type`.
    Single {
        prefix: Box<SigType>,
        sym: String,
    },
    /// SIP-23 literal type.
    Constant(Constant),
    /// `sym[args]`. `sym` is the dotted full name for classes, or the plain
    /// name for type parameters and abstract type members.
    Ref {
        sym: String,
        args: Vec<SigType>,
    },
    Bounds {
        lo: Box<SigType>,
        hi: Box<SigType>,
    },
    Refined {
        parents: Vec<SigType>,
        decls: Vec<Member>,
    },
    /// One parameter list. Curried methods nest: `Method { result: Method }`.
    Method {
        params: Vec<Param>,
        implicit: bool,
        result: Box<SigType>,
    },
    /// `[tparams] result`. nsc writes a `POLYtpe` with no tparams for
    /// `NullaryMethodType`, i.e. a parameterless `def`; that case is kept as
    /// `Poly { tparams: [], .. }` so it stays distinguishable from a `val`.
    Poly {
        tparams: Vec<TParam>,
        result: Box<SigType>,
    },
    Existential {
        quantified: Vec<TParam>,
        result: Box<SigType>,
    },
    /// `T @ann` — the annotation itself is dropped, the type is not.
    Annotated(Box<SigType>),
    Super {
        this_tpe: Box<SigType>,
        super_tpe: Box<SigType>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: SigType,
    pub by_name: bool,
    /// Raw pickled flags; see [`crate::read::pflags`]. `DEFAULTPARAM` says the
    /// caller may omit this argument, in which case the value comes from the
    /// class's `<method>$default$<n>` getter.
    pub flags: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TParam {
    pub name: String,
    pub bounds: SigType,
    pub variance: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    /// `def` (VALsym with METHOD).
    Def,
    /// `val` / `var` / parameter accessor (VALsym without METHOD).
    Val,
    /// `type T >: L <: H`.
    AbstractType,
    /// `type T = U`.
    TypeAlias,
    /// Nested class or trait.
    Class,
    /// Companion / nested `object`.
    Module,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub name: String,
    pub kind: MemberKind,
    /// Raw pickled flags; see [`pflags`].
    pub flags: u64,
    pub ty: SigType,
}

impl Member {
    pub fn has(&self, flag: u64) -> bool {
        self.flags & flag != 0
    }

    /// Members a caller can see: not private, not synthetic plumbing.
    pub fn is_public_api(&self) -> bool {
        !self.has(pflags::PRIVATE)
            && !self.has(pflags::BRIDGE)
            && !self.has(pflags::SYNTHETIC)
            && !self.has(pflags::LOCAL)
            && !self.name.contains("$default$")
            && !self.name.ends_with(' ')
    }
}

/// One class or module class recovered from a pickle.
#[derive(Clone, Debug)]
pub struct ClassSig {
    /// Dotted full name (`scala.collection.immutable.List`). A module class
    /// carries the same name as its companion class, distinguished by
    /// [`ClassSig::is_module`].
    pub full_name: String,
    pub is_module: bool,
    pub flags: u64,
    pub tparams: Vec<TParam>,
    /// Parent types from the `CLASSINFOtpe`, in declaration order.
    pub parents: Vec<SigType>,
    /// Members declared directly on this class.
    pub members: Vec<Member>,
    /// References this class's types could not resolve: symbols with no
    /// nameable owner chain, and entries a type pointed at that were not
    /// types. Never silently dropped, so a caller can report a real gap
    /// instead of quietly serving a wrong signature.
    pub unresolved: Vec<String>,
}

impl ClassSig {
    pub fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.name == name)
    }

    /// All overloads of `name` declared directly on this class.
    pub fn members_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Member> + 'a {
        self.members.iter().filter(move |m| m.name == name)
    }

    /// Full names of the parent classes, ready to be loaded.
    pub fn parent_names(&self) -> Vec<String> {
        self.parents
            .iter()
            .filter_map(|p| match p {
                SigType::Ref { sym, .. } if sym.contains('.') => Some(sym.clone()),
                _ => None,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Building signatures out of a parsed pickle
// ---------------------------------------------------------------------------

/// Every class and module class in a pickle, in entry order.
pub fn class_sigs(p: &Pickle) -> Vec<ClassSig> {
    let mut owners: HashMap<Idx, Vec<Idx>> = HashMap::new();
    for (i, e) in p.entries.iter().enumerate() {
        if let Some(info) = e.sym_info() {
            owners.entry(info.owner).or_default().push(i as Idx);
        }
    }
    let mut out = Vec::new();
    for (i, e) in p.entries.iter().enumerate() {
        let Entry::ClassSym { info, .. } = e else {
            continue;
        };
        // `<refinement>` classes are the decls of a REFINEDtpe, not real classes.
        if p.name(info.name) == Some("<refinement>") {
            continue;
        }
        let mut b = Builder {
            p,
            owners: &owners,
            unresolved: Vec::new(),
        };
        let (tparams, parents) = b.class_info(info.info);
        let members = b.members_of(i as Idx);
        out.push(ClassSig {
            full_name: p.sym_full_name(i as Idx).unwrap_or_default(),
            is_module: info.has(pflags::MODULE),
            flags: info.flags,
            tparams,
            parents,
            members,
            unresolved: b.unresolved,
        });
    }
    out
}

struct Builder<'a> {
    p: &'a Pickle,
    owners: &'a HashMap<Idx, Vec<Idx>>,
    unresolved: Vec<String>,
}

impl Builder<'_> {
    fn class_info(&mut self, info: Idx) -> (Vec<TParam>, Vec<SigType>) {
        match self.p.entry(info) {
            Some(Entry::PolyTpe { result, tparams }) => {
                let tps = tparams.iter().map(|&t| self.tparam(t)).collect();
                let (_, parents) = self.class_info(*result);
                (tps, parents)
            }
            Some(Entry::ClassInfoTpe { parents, .. }) => {
                (Vec::new(), parents.iter().map(|&t| self.ty(t, 0)).collect())
            }
            _ => (Vec::new(), Vec::new()),
        }
    }

    fn members_of(&mut self, owner: Idx) -> Vec<Member> {
        let Some(ids) = self.owners.get(&owner) else {
            return Vec::new();
        };
        let ids = ids.clone();
        let mut out = Vec::new();
        for id in ids {
            if let Some(m) = self.member(id) {
                out.push(m);
            }
        }
        out
    }

    fn member(&mut self, id: Idx) -> Option<Member> {
        let e = self.p.entry(id)?;
        let info = e.sym_info()?;
        let kind = match e {
            Entry::ValSym { .. } if info.has(pflags::METHOD) => MemberKind::Def,
            Entry::ValSym { .. } => MemberKind::Val,
            Entry::TypeSym(_) => MemberKind::AbstractType,
            Entry::AliasSym(_) => MemberKind::TypeAlias,
            Entry::ModuleSym { .. } => MemberKind::Module,
            Entry::ClassSym { .. } if info.has(pflags::MODULE) => MemberKind::Module,
            Entry::ClassSym { .. } => MemberKind::Class,
            _ => return None,
        };
        // A class's own type parameters are owned by it but are not members.
        if info.has(pflags::PARAM) {
            return None;
        }
        let name = self.p.name(info.name)?.to_string();
        let ty = self.ty(info.info, 0);
        Some(Member {
            name,
            kind,
            flags: info.flags,
            ty,
        })
    }

    fn tparam(&mut self, id: Idx) -> TParam {
        let name = self
            .p
            .entry(id)
            .and_then(|e| e.sym_info())
            .and_then(|i| self.p.name(i.name))
            .unwrap_or("?")
            .to_string();
        let flags = self
            .p
            .entry(id)
            .and_then(|e| e.sym_info())
            .map(|i| i.flags)
            .unwrap_or(0);
        let variance = if flags & pflags::COVARIANT != 0 {
            1
        } else if flags & pflags::CONTRAVARIANT != 0 {
            -1
        } else {
            0
        };
        let bounds = match self.p.entry(id).and_then(|e| e.sym_info()) {
            Some(i) => self.ty(i.info, 0),
            None => SigType::None,
        };
        TParam {
            name,
            bounds,
            variance,
        }
    }

    fn param(&mut self, id: Idx) -> Param {
        let info = self.p.entry(id).and_then(|e| e.sym_info());
        let name = info
            .and_then(|i| self.p.name(i.name))
            .unwrap_or("_")
            .to_string();
        let (ty, by_name, flags) = match info {
            Some(i) => {
                let t = self.ty(i.info, 0);
                (t, i.has(pflags::BYNAMEPARAM), i.flags)
            }
            None => (SigType::None, false, 0),
        };
        Param {
            name,
            ty,
            by_name,
            flags,
        }
    }

    /// Resolve a symbol reference to the name a `SigType::Ref` should carry.
    fn sym_ref_name(&mut self, sym: Idx) -> String {
        match self.p.entry(sym) {
            // Type parameters and abstract types are only meaningful by their
            // simple name; qualifying them with the owner would be misleading.
            Some(Entry::TypeSym(i)) => self.p.name(i.name).unwrap_or("?").to_string(),
            // `<root>` and `<empty>` terminate the owner chain, so their own
            // full name is empty; they are resolved, not missing.
            Some(_) if matches!(self.p.sym_name(sym), Some("<root>") | Some("<empty>")) => {
                self.p.sym_name(sym).unwrap_or("?").to_string()
            }
            _ => match self.p.sym_full_name(sym) {
                Some(n) if !n.is_empty() => n,
                _ => {
                    let n = self.p.sym_name(sym).unwrap_or("?").to_string();
                    self.unresolved.push(n.clone());
                    n
                }
            },
        }
    }

    fn ty(&mut self, id: Idx, depth: u32) -> SigType {
        if depth > 64 {
            return SigType::None;
        }
        let d = depth + 1;
        match self.p.entry(id) {
            None | Some(Entry::NoTpe) | Some(Entry::NoPrefixTpe) => SigType::None,
            Some(Entry::ThisTpe(s)) => {
                let s = *s;
                SigType::This(self.sym_ref_name(s))
            }
            Some(Entry::SingleTpe { prefix, sym }) => {
                let (prefix, sym) = (*prefix, *sym);
                let p = self.ty(prefix, d);
                SigType::Single {
                    prefix: Box::new(p),
                    sym: self.sym_ref_name(sym),
                }
            }
            Some(Entry::SuperTpe {
                this_tpe,
                super_tpe,
            }) => {
                let (a, b) = (*this_tpe, *super_tpe);
                SigType::Super {
                    this_tpe: Box::new(self.ty(a, d)),
                    super_tpe: Box::new(self.ty(b, d)),
                }
            }
            Some(Entry::ConstantTpe(c)) => match self.p.entry(*c) {
                Some(Entry::Literal(k)) => SigType::Constant(k.clone()),
                _ => {
                    self.unresolved
                        .push(format!("CONSTANTtpe #{id} does not point at a literal"));
                    SigType::None
                }
            },
            Some(Entry::TypeRefTpe { sym, args, .. }) => {
                let (sym, args) = (*sym, args.clone());
                let name = self.sym_ref_name(sym);
                SigType::Ref {
                    sym: name,
                    args: args.into_iter().map(|a| self.ty(a, d)).collect(),
                }
            }
            Some(Entry::TypeBoundsTpe { lo, hi }) => {
                let (lo, hi) = (*lo, *hi);
                SigType::Bounds {
                    lo: Box::new(self.ty(lo, d)),
                    hi: Box::new(self.ty(hi, d)),
                }
            }
            Some(Entry::RefinedTpe { sym, parents }) => {
                let (sym, parents) = (*sym, parents.clone());
                let decls = self.members_of(sym);
                SigType::Refined {
                    parents: parents.into_iter().map(|t| self.ty(t, d)).collect(),
                    decls,
                }
            }
            Some(Entry::ClassInfoTpe { parents, .. }) => {
                let parents = parents.clone();
                SigType::Refined {
                    parents: parents.into_iter().map(|t| self.ty(t, d)).collect(),
                    decls: Vec::new(),
                }
            }
            Some(Entry::MethodTpe { result, params })
            | Some(Entry::ImplicitMethodTpe { result, params }) => {
                let (result, params) = (*result, params.clone());
                let implicit = params.first().is_some_and(|&p| {
                    self.p
                        .entry(p)
                        .and_then(|e| e.sym_info())
                        .is_some_and(|i| i.has(pflags::IMPLICIT))
                });
                let ps: Vec<Param> = params.into_iter().map(|p| self.param(p)).collect();
                SigType::Method {
                    params: ps,
                    implicit,
                    result: Box::new(self.ty(result, d)),
                }
            }
            Some(Entry::PolyTpe { result, tparams }) => {
                let (result, tparams) = (*result, tparams.clone());
                SigType::Poly {
                    tparams: tparams.into_iter().map(|t| self.tparam(t)).collect(),
                    result: Box::new(self.ty(result, d)),
                }
            }
            Some(Entry::ExistentialTpe { result, quantified }) => {
                let (result, quantified) = (*result, quantified.clone());
                SigType::Existential {
                    quantified: quantified.into_iter().map(|t| self.tparam(t)).collect(),
                    result: Box::new(self.ty(result, d)),
                }
            }
            Some(Entry::AnnotatedTpe { tpe, .. }) => {
                let tpe = *tpe;
                SigType::Annotated(Box::new(self.ty(tpe, d)))
            }
            Some(other) => {
                // A type reference pointing at something that is not a type
                // means the pickle is not shaped the way we think it is.
                self.unresolved
                    .push(format!("entry #{id} is not a type: {other:?}"));
                SigType::None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Loading signatures across classfiles
// ---------------------------------------------------------------------------

/// Where [`SigLoader`] gets classfile bytes (a jar, a directory, a test fixture).
pub trait ClassSource {
    /// `internal_name` is a JVM internal name without `.class`
    /// (`scala/collection/IterableOps`).
    fn class_bytes(&mut self, internal_name: &str) -> Option<Vec<u8>>;
}

impl<F> ClassSource for F
where
    F: FnMut(&str) -> Option<Vec<u8>>,
{
    fn class_bytes(&mut self, internal_name: &str) -> Option<Vec<u8>> {
        self(internal_name)
    }
}

/// Why a class could not be supplied from pickles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// No classfile with that name on the source.
    NotFound(String),
    /// The classfile exists but carries no `ScalaSignature` (a Java class).
    NoSignature(String),
    /// The `ScalaSignature` is there but does not parse.
    BadPickle(String, ReadError),
    /// The pickle parses but has no class with that name.
    NoSuchClass(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::NotFound(n) => write!(f, "{n}: no classfile on the source"),
            LoadError::NoSignature(n) => write!(f, "{n}: no ScalaSignature (Java class?)"),
            LoadError::BadPickle(n, e) => write!(f, "{n}: {e}"),
            LoadError::NoSuchClass(n) => write!(f, "{n}: pickle has no such class"),
        }
    }
}

/// Caches [`ClassSig`]s. The classfile source is passed in per call so a
/// caller that already owns one (the typer owns a `BinaryIndex`) does not have
/// to give it up; [`SigLoader`] is the owning convenience wrapper.
#[derive(Default)]
pub struct SigCache {
    cache: HashMap<String, Result<Rc<ClassSig>, LoadError>>,
}

/// One member found by [`SigCache::lookup`].
#[derive(Clone, Debug)]
pub struct MemberHit {
    /// Dotted name of the class that declares it.
    pub owner: String,
    /// The member, with its type already substituted into the *queried*
    /// class's type-parameter vocabulary: `List#filter` comes back returning
    /// `List[A]`, not `IterableOps`'s opaque `C`.
    pub member: Member,
}

impl SigCache {
    pub fn new() -> Self {
        SigCache::default()
    }

    /// The signature of a class by dotted full name. `module` selects the
    /// module class (`object List`) over the class (`class List`).
    pub fn class_sig<S: ClassSource + ?Sized>(
        &mut self,
        src: &mut S,
        full_name: &str,
        module: bool,
    ) -> Result<Rc<ClassSig>, LoadError> {
        let key = if module {
            format!("{full_name}$")
        } else {
            full_name.to_string()
        };
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let got = load(src, full_name, module);
        self.cache.insert(key, got.clone());
        got
    }

    /// Find every overload of `name` visible on `full_name`, in **linearization
    /// order**: the most derived declaration first.
    ///
    /// The order is the one SLS 5.1.2 specifies and nsc implements,
    /// `L(C) = C, L(Cn) +: ... +: L(C1)`, so a later parent wins over an
    /// earlier one for the ancestors they share. That is what decides which
    /// binding of an inherited type parameter a member comes back with:
    /// `immutable.Set` mixes in `Iterable[A]` before `SetOps[A, Set, Set[A]]`,
    /// so `IterableOps`'s `C` must resolve through `SetOps` (`Set[A]`) and not
    /// through `Iterable` (`Iterable[A]`). A plain breadth-first walk gets that
    /// backwards and hands back the weaker type.
    ///
    /// Each hop substitutes the parent's type parameters with the arguments the
    /// child passed it, so what comes back is expressed in the queried class's
    /// own type parameters.
    ///
    /// Classes that could not be loaded are returned separately, so a caller
    /// can tell "no such member" from "we could not look".
    pub fn lookup<S: ClassSource + ?Sized>(
        &mut self,
        src: &mut S,
        full_name: &str,
        module: bool,
        name: &str,
    ) -> (Vec<MemberHit>, Vec<LoadError>) {
        let mut errs = Vec::new();
        let lin = self.linearization(src, full_name, module, &mut errs);
        let mut found = Vec::new();
        for step in &lin {
            let Ok(sig) = self.class_sig(src, &step.class_name, step.module) else {
                continue;
            };
            for m in sig.members_named(name) {
                let mut m = m.clone();
                m.ty = apply_subst(&m.ty, &step.subst);
                found.push(MemberHit {
                    owner: step.class_name.clone(),
                    member: m,
                });
            }
        }
        (found, errs)
    }

    /// The class linearization of `full_name`, each step carrying the
    /// substitution that expresses it in `full_name`'s type parameters.
    pub fn linearization<S: ClassSource + ?Sized>(
        &mut self,
        src: &mut S,
        full_name: &str,
        module: bool,
        errs: &mut Vec<LoadError>,
    ) -> Vec<LinStep> {
        let mut walk = LinWalk {
            budget: LIN_BUDGET,
            depth: 0,
            errs,
        };
        self.lin_of(src, full_name, module, HashMap::new(), &mut walk)
    }

    fn lin_of<S: ClassSource + ?Sized>(
        &mut self,
        src: &mut S,
        class_name: &str,
        module: bool,
        subst: HashMap<String, SigType>,
        walk: &mut LinWalk<'_>,
    ) -> Vec<LinStep> {
        let here = LinStep {
            class_name: class_name.to_string(),
            module,
            subst: subst.clone(),
        };
        if walk.depth > LIN_MAX_DEPTH || walk.budget == 0 {
            return vec![here];
        }
        walk.budget -= 1;
        let sig = match self.class_sig(src, class_name, module) {
            Ok(sig) => sig,
            Err(e) => {
                walk.errs.push(e);
                return vec![here];
            }
        };
        // acc = L(C1); then acc = L(Ci) ++ (acc minus L(Ci)) for i = 2..n,
        // which is SLS's `L(Cn) +: ... +: L(C1)` written left to right.
        let mut acc: Vec<LinStep> = Vec::new();
        for p in &sig.parents {
            let SigType::Ref { sym, args } = p else {
                continue;
            };
            if !sym.contains('.') {
                continue;
            }
            // The parent's arguments are written in `class_name`'s vocabulary;
            // lifting them through `subst` puts them in the queried class's.
            let lifted: Vec<SigType> = args.iter().map(|a| apply_subst(a, &subst)).collect();
            let mut next = HashMap::new();
            if let Ok(psig) = self.class_sig(src, sym, false) {
                for (tp, arg) in psig.tparams.iter().zip(lifted) {
                    next.insert(tp.name.clone(), arg);
                }
            }
            walk.depth += 1;
            let plin = self.lin_of(src, sym, false, next, walk);
            walk.depth -= 1;
            let names: Vec<&str> = plin.iter().map(|s| s.class_name.as_str()).collect();
            let kept: Vec<LinStep> = acc
                .into_iter()
                .filter(|e| !names.contains(&e.class_name.as_str()))
                .collect();
            acc = plin;
            acc.extend(kept);
        }
        let mut out = vec![here];
        let mut seen: Vec<String> = vec![class_name.to_string()];
        for e in acc {
            if seen.contains(&e.class_name) {
                continue;
            }
            seen.push(e.class_name.clone());
            out.push(e);
        }
        out
    }
}

/// One class in a linearization, with the substitution that expresses its type
/// parameters in the queried class's vocabulary.
#[derive(Clone, Debug)]
pub struct LinStep {
    pub class_name: String,
    pub module: bool,
    pub subst: HashMap<String, SigType>,
}

/// Bookkeeping for one linearization walk.
struct LinWalk<'a> {
    budget: u32,
    depth: u32,
    errs: &'a mut Vec<LoadError>,
}

/// The collection hierarchy is wide and repetitive; these bound the work when a
/// pickle describes something pathological.
const LIN_BUDGET: u32 = 4096;
const LIN_MAX_DEPTH: u32 = 32;

/// Classfiles that may carry the pickle describing `full_name`.
///
/// A pickle names classes with dots throughout, so `scala.reflect.api.Names`
/// and `scala.reflect.api.Names.TermNameExtractor` look alike even though only
/// the first is a file. Two rules recover the file:
///
/// * a nested class lives in `Outer$Inner.class`, so trailing dots become `$`;
/// * scalac writes **one** `ScalaSignature` per top-level class, covering
///   every class nested inside it, and nested classfiles carry none at all
///   (`Names$TermNameExtractor.class` has no signature; `Names.class` has the
///   signature for both). So the enclosing files are candidates too.
///
/// Splits are tried right to left, most specific first, and the companion
/// (`X$.class`) variant of each: scalac pickles a companion pair once, on the
/// class file, so `List$.class` has no signature of its own.
pub fn pickle_files_for(full_name: &str, module: bool) -> Vec<String> {
    let parts: Vec<&str> = full_name.split('.').collect();
    let mut out: Vec<String> = Vec::new();
    let mut push = |base: String| {
        let (a, b) = if module {
            (format!("{base}$"), base)
        } else {
            (base.clone(), format!("{base}$"))
        };
        for c in [a, b] {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    };
    // `k` is where packages end and classes begin. The rightmost split is the
    // common case (`a.b.C`), so it is tried first and nothing changes for a
    // top-level class.
    for k in (1..parts.len()).rev() {
        push(format!("{}/{}", parts[..k].join("/"), parts[k..].join("$")));
        if k + 1 < parts.len() {
            // The top-level class under that split: its file holds the pickle.
            push(format!("{}/{}", parts[..k].join("/"), parts[k]));
        }
    }
    if parts.len() == 1 {
        push(full_name.to_string());
    }
    out
}

fn load<S: ClassSource + ?Sized>(
    src: &mut S,
    full_name: &str,
    module: bool,
) -> Result<Rc<ClassSig>, LoadError> {
    let candidates = pickle_files_for(full_name, module);
    let mut last = LoadError::NotFound(full_name.to_string());
    for c in &candidates {
        let Some(bytes) = src.class_bytes(c) else {
            continue;
        };
        let Some(raw) = crate::classfile::scala_signature_bytes(&bytes) else {
            last = LoadError::NoSignature(full_name.to_string());
            continue;
        };
        let p = match read_pickle(&raw) {
            Ok(p) => p,
            Err(e) => {
                last = LoadError::BadPickle(full_name.to_string(), e);
                continue;
            }
        };
        match class_sigs(&p)
            .into_iter()
            .find(|c| c.full_name == full_name && c.is_module == module)
        {
            Some(sig) => return Ok(Rc::new(sig)),
            None => last = LoadError::NoSuchClass(full_name.to_string()),
        }
    }
    Err(last)
}

/// Replace type-parameter references by name. Binders (`Poly`, `Existential`)
/// shadow, so their own names are dropped from the map before descending.
pub fn apply_subst(t: &SigType, map: &HashMap<String, SigType>) -> SigType {
    if map.is_empty() {
        return t.clone();
    }
    let go = |x: &SigType| apply_subst(x, map);
    match t {
        SigType::Ref { sym, args } => {
            let args: Vec<SigType> = args.iter().map(go).collect();
            match map.get(sym) {
                // `CC` bound to `List`, used as `CC[B]`, is `List[B]`.
                Some(SigType::Ref {
                    sym: s2,
                    args: rargs,
                }) if rargs.is_empty() => SigType::Ref {
                    sym: s2.clone(),
                    args,
                },
                Some(other) if args.is_empty() => other.clone(),
                // A non-`Ref` replacement cannot absorb arguments; leaving the
                // original alone is the honest answer (the caller then sees an
                // unresolvable name and declines to supply the member).
                _ => SigType::Ref {
                    sym: sym.clone(),
                    args,
                },
            }
        }
        SigType::This(_) | SigType::Constant(_) | SigType::None => t.clone(),
        SigType::Single { prefix, sym } => SigType::Single {
            prefix: Box::new(go(prefix)),
            sym: sym.clone(),
        },
        SigType::Bounds { lo, hi } => SigType::Bounds {
            lo: Box::new(go(lo)),
            hi: Box::new(go(hi)),
        },
        SigType::Refined { parents, decls } => SigType::Refined {
            parents: parents.iter().map(go).collect(),
            decls: decls
                .iter()
                .map(|d| Member {
                    ty: go(&d.ty),
                    ..d.clone()
                })
                .collect(),
        },
        SigType::Method {
            params,
            implicit,
            result,
        } => SigType::Method {
            params: params
                .iter()
                .map(|p| Param {
                    ty: go(&p.ty),
                    ..p.clone()
                })
                .collect(),
            implicit: *implicit,
            result: Box::new(go(result)),
        },
        SigType::Poly { tparams, result } => {
            let inner = without(map, tparams);
            let (tparams, result) = avoid_capture(tparams, result, &inner);
            SigType::Poly {
                tparams: tparams
                    .iter()
                    .map(|tp| TParam {
                        bounds: apply_subst(&tp.bounds, &inner),
                        ..tp.clone()
                    })
                    .collect(),
                result: Box::new(apply_subst(&result, &inner)),
            }
        }
        SigType::Existential { quantified, result } => {
            let inner = without(map, quantified);
            let (quantified, result) = avoid_capture(quantified, result, &inner);
            SigType::Existential {
                quantified: quantified
                    .iter()
                    .map(|tp| TParam {
                        bounds: apply_subst(&tp.bounds, &inner),
                        ..tp.clone()
                    })
                    .collect(),
                result: Box::new(apply_subst(&result, &inner)),
            }
        }
        SigType::Annotated(t) => SigType::Annotated(Box::new(go(t))),
        SigType::Super {
            this_tpe,
            super_tpe,
        } => SigType::Super {
            this_tpe: Box::new(go(this_tpe)),
            super_tpe: Box::new(go(super_tpe)),
        },
    }
}

/// Every bare (undotted) name a signature mentions -- the shape a reference to
/// a type *parameter* has, as against the dotted full name of a class.
fn bare_names(t: &SigType, out: &mut Vec<String>) {
    match t {
        SigType::Ref { sym, args } => {
            if !sym.contains('.') && !out.contains(sym) {
                out.push(sym.clone());
            }
            for a in args {
                bare_names(a, out);
            }
        }
        SigType::This(_) | SigType::Constant(_) | SigType::None => {}
        SigType::Single { prefix, .. } => bare_names(prefix, out),
        SigType::Bounds { lo, hi } => {
            bare_names(lo, out);
            bare_names(hi, out);
        }
        SigType::Refined { parents, decls } => {
            for p in parents {
                bare_names(p, out);
            }
            for d in decls {
                bare_names(&d.ty, out);
            }
        }
        SigType::Method { params, result, .. } => {
            for p in params {
                bare_names(&p.ty, out);
            }
            bare_names(result, out);
        }
        SigType::Poly { tparams, result } => {
            for tp in tparams {
                bare_names(&tp.bounds, out);
            }
            bare_names(result, out);
        }
        SigType::Existential { quantified, result } => {
            for tp in quantified {
                bare_names(&tp.bounds, out);
            }
            bare_names(result, out);
        }
        SigType::Annotated(t) => bare_names(t, out),
        SigType::Super {
            this_tpe,
            super_tpe,
        } => {
            bare_names(this_tpe, out);
            bare_names(super_tpe, out);
        }
    }
}

/// Rename a binder whose name occurs free in the *range* of `map`.
///
/// Looking a member up walks the linearization, and every hop substitutes the
/// parent's type parameters with the arguments the child passed it
/// ([`SigCache::lookup`]). `Iterator.GroupedIterator[B] extends
/// AbstractIterator[Seq[B]]` makes one of those substitutions `A := Seq[B]`,
/// and the `Iterator.map[B](f: A => B): Iterator[B]` it is applied to binds a
/// `B` of its own. Without renaming, the *class's* `B` inside `Seq[B]` lands
/// under the *method's* binder and the two become one type: `g.map(f)` then
/// takes an `Int` where it takes a `Seq[Int]`, which type-checks a lambda
/// against the wrong element type and is a `VerifyError` once it runs.
fn avoid_capture(
    tparams: &[TParam],
    result: &SigType,
    map: &HashMap<String, SigType>,
) -> (Vec<TParam>, SigType) {
    if map.is_empty() {
        return (tparams.to_vec(), result.clone());
    }
    let mut free: Vec<String> = Vec::new();
    for v in map.values() {
        bare_names(v, &mut free);
    }
    let mut rename: HashMap<String, SigType> = HashMap::new();
    let mut out = tparams.to_vec();
    for tp in out.iter_mut() {
        if !free.contains(&tp.name) {
            continue;
        }
        let mut n = 1;
        let mut fresh = format!("{}${n}", tp.name);
        while free.contains(&fresh) || tparams.iter().any(|o| o.name == fresh) {
            n += 1;
            fresh = format!("{}${n}", tp.name);
        }
        rename.insert(
            tp.name.clone(),
            SigType::Ref {
                sym: fresh.clone(),
                args: Vec::new(),
            },
        );
        tp.name = fresh;
    }
    if rename.is_empty() {
        return (out, result.clone());
    }
    for tp in out.iter_mut() {
        tp.bounds = apply_subst(&tp.bounds, &rename);
    }
    (out, apply_subst(result, &rename))
}

fn without(map: &HashMap<String, SigType>, bound: &[TParam]) -> HashMap<String, SigType> {
    let mut m = map.clone();
    for tp in bound {
        m.remove(&tp.name);
    }
    m
}

/// Owns a [`ClassSource`] and a [`SigCache`]. Convenience for callers that do
/// not already hold the source elsewhere.
pub struct SigLoader<S: ClassSource> {
    src: S,
    cache: SigCache,
}

impl<S: ClassSource> SigLoader<S> {
    pub fn new(src: S) -> Self {
        SigLoader {
            src,
            cache: SigCache::new(),
        }
    }

    pub fn class_sig(&mut self, full_name: &str, module: bool) -> Result<Rc<ClassSig>, LoadError> {
        self.cache.class_sig(&mut self.src, full_name, module)
    }

    pub fn lookup(
        &mut self,
        full_name: &str,
        module: bool,
        name: &str,
    ) -> (Vec<MemberHit>, Vec<LoadError>) {
        self.cache.lookup(&mut self.src, full_name, module, name)
    }

    pub fn linearization(&mut self, full_name: &str, module: bool) -> Vec<LinStep> {
        let mut errs = Vec::new();
        self.cache
            .linearization(&mut self.src, full_name, module, &mut errs)
    }
}

// ---------------------------------------------------------------------------
// Rendering (diagnostics and tests)
// ---------------------------------------------------------------------------

/// Scala-ish rendering of a recovered type, for diagnostics and tests.
pub fn render(t: &SigType) -> String {
    match t {
        SigType::None => "<none>".into(),
        SigType::This(n) => format!("{n}.this.type"),
        SigType::Single { prefix, sym } => match &**prefix {
            SigType::None => format!("{sym}.type"),
            p => format!("{}.{sym}.type", render(p)),
        },
        SigType::Constant(c) => format!("{c:?}"),
        SigType::Ref { sym, args } if args.is_empty() => sym.clone(),
        SigType::Ref { sym, args } => {
            let a: Vec<String> = args.iter().map(render).collect();
            format!("{sym}[{}]", a.join(", "))
        }
        SigType::Bounds { lo, hi } => format!(">: {} <: {}", render(lo), render(hi)),
        SigType::Refined { parents, decls } => {
            let ps: Vec<String> = parents.iter().map(render).collect();
            if decls.is_empty() {
                ps.join(" with ")
            } else {
                let ds: Vec<String> = decls
                    .iter()
                    .map(|d| format!("{}: {}", d.name, render(&d.ty)))
                    .collect();
                format!("{} {{ {} }}", ps.join(" with "), ds.join("; "))
            }
        }
        SigType::Method {
            params,
            implicit,
            result,
        } => {
            let ps: Vec<String> = params
                .iter()
                .map(|p| {
                    let arrow = if p.by_name { "=> " } else { "" };
                    format!("{}: {arrow}{}", p.name, render(&p.ty))
                })
                .collect();
            let imp = if *implicit { "implicit " } else { "" };
            format!("({imp}{}){}", ps.join(", "), render(result))
        }
        SigType::Poly { tparams, result } if tparams.is_empty() => format!("=> {}", render(result)),
        SigType::Poly { tparams, result } => {
            let ts: Vec<String> = tparams.iter().map(|t| t.name.clone()).collect();
            format!("[{}]{}", ts.join(", "), render(result))
        }
        SigType::Existential { quantified, result } => {
            let qs: Vec<String> = quantified.iter().map(|t| t.name.clone()).collect();
            format!("{} forSome {{ {} }}", render(result), qs.join("; "))
        }
        SigType::Annotated(t) => render(t),
        SigType::Super {
            this_tpe,
            super_tpe,
        } => format!("{}.super[{}]", render(this_tpe), render(super_tpe)),
    }
}
