//! Turn a parsed pickle into class signatures, and resolve members across
//! inheritance by loading the pickles of parent classes on demand.
//!
//! This is the bridge layer between [`crate::pickle_read`] (bytes -> entries)
//! and a symbol table: entry indices are resolved into names and a
//! self-contained [`SigType`] tree, and [`SigLoader`] walks parents so that
//! `List#filter` is found on `scala.collection.IterableOps` without anyone
//! having to say where it lives.
//!
//! It deliberately stops short of the typer's `Type`: `crates/typer` cannot
//! depend on `crates/backend`, so the last hop (SigType -> `scala_rs_parser::Type`
//! inside the typer) is a separate step. See README, "ScalaSignature からの
//! シンボル自動供給".

use std::collections::HashMap;
use std::rc::Rc;

use crate::pickle_read::{pflags, read_pickle, Constant, Entry, Idx, Pickle, ReadError};

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
        let (ty, by_name) = match info {
            Some(i) => {
                let t = self.ty(i.info, 0);
                let by_name = i.has(pflags::BYNAMEPARAM);
                (t, by_name)
            }
            None => (SigType::None, false),
        };
        Param { name, ty, by_name }
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

/// Loads and caches [`ClassSig`]s, following parents on demand.
pub struct SigLoader<S: ClassSource> {
    src: S,
    cache: HashMap<String, Result<Rc<ClassSig>, LoadError>>,
}

impl<S: ClassSource> SigLoader<S> {
    pub fn new(src: S) -> Self {
        SigLoader {
            src,
            cache: HashMap::new(),
        }
    }

    /// The signature of a class by dotted full name. `module` selects the
    /// module class (`object List`) over the class (`class List`).
    pub fn class_sig(&mut self, full_name: &str, module: bool) -> Result<Rc<ClassSig>, LoadError> {
        let key = if module {
            format!("{full_name}$")
        } else {
            full_name.to_string()
        };
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let got = self.load(full_name, module);
        self.cache.insert(key, got.clone());
        got
    }

    fn load(&mut self, full_name: &str, module: bool) -> Result<Rc<ClassSig>, LoadError> {
        let internal = full_name.replace('.', "/");
        // scalac pickles a companion pair once, on the *class* file: in 2.13.16
        // `List$.class` has no `ScalaSignature` at all, so a module class has to
        // fall back to its companion's classfile. Try both, in the order most
        // likely to hit, and only report a failure once neither worked.
        let candidates: [String; 2] = if module {
            [format!("{internal}$"), internal.clone()]
        } else {
            [internal.clone(), format!("{internal}$")]
        };
        let mut last = LoadError::NotFound(full_name.to_string());
        for c in &candidates {
            let Some(bytes) = self.src.class_bytes(c) else {
                continue;
            };
            let Some(raw) = crate::load::scala_signature_bytes(&bytes) else {
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

    /// Find every overload of `name` visible on `full_name`, searching the
    /// class itself and then its parents breadth-first (nsc linearization is
    /// finer-grained, but for "does this member exist and with what type" a
    /// BFS over parents gives the same answer for non-overridden members).
    ///
    /// Returns `(declaring class, member)` pairs, and separately the parents
    /// that could not be loaded, so a caller can tell "no such member" from
    /// "we could not look".
    pub fn lookup(
        &mut self,
        full_name: &str,
        module: bool,
        name: &str,
    ) -> (Vec<(String, Member)>, Vec<LoadError>) {
        let mut found = Vec::new();
        let mut errs = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut queue: Vec<(String, bool)> = vec![(full_name.to_string(), module)];
        while let Some((cur, cur_module)) = queue.first().cloned() {
            queue.remove(0);
            if seen.contains(&cur) {
                continue;
            }
            seen.push(cur.clone());
            match self.class_sig(&cur, cur_module) {
                Ok(sig) => {
                    for m in sig.members_named(name) {
                        found.push((cur.clone(), m.clone()));
                    }
                    for p in sig.parent_names() {
                        queue.push((p, false));
                    }
                }
                Err(e) => errs.push(e),
            }
        }
        (found, errs)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pickle;

    fn sigs_of(src: &str) -> Vec<ClassSig> {
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let mut out = Vec::new();
        for raw in pickle::pickle_all(&st).values() {
            let p = read_pickle(raw).expect("read our own pickle");
            out.extend(class_sigs(&p));
        }
        out
    }

    #[test]
    fn recovers_a_polymorphic_method_signature() {
        let sigs = sigs_of(
            r#"
class Box[A](val get: A) {
  def map[B](f: A => B): Box[B] = new Box(f(get))
  def size: Int = 1
}
"#,
        );
        let boxc = sigs
            .iter()
            .find(|c| c.full_name == "Box" && !c.is_module)
            .expect("Box");
        assert_eq!(boxc.tparams.len(), 1);
        assert_eq!(boxc.tparams[0].name, "A");
        let map = boxc.member("map").expect("Box#map");
        assert_eq!(map.kind, MemberKind::Def);
        let rendered = render(&map.ty);
        assert!(rendered.starts_with("[B](f: "), "{rendered}");
        assert!(rendered.ends_with("Box[B]"), "{rendered}");
        // A parameterless `def` stays a (nullary) method, not a val.
        let size = boxc.member("size").expect("Box#size");
        assert_eq!(render(&size.ty), "=> scala.Int");
        assert_eq!(size.kind, MemberKind::Def);
    }

    #[test]
    fn parents_and_module_classes_are_recovered() {
        let sigs = sigs_of(
            r#"
trait Show { def show: String }
class Impl extends Show { def show: String = "" }
object Impl { val tag: String = "i" }
"#,
        );
        let imp = sigs
            .iter()
            .find(|c| c.full_name == "Impl" && !c.is_module)
            .expect("Impl class");
        assert!(
            !imp.parents.is_empty(),
            "expected at least one parent for Impl"
        );
        assert!(imp.member("show").is_some(), "Impl#show");
        let obj = sigs
            .iter()
            .find(|c| c.full_name == "Impl" && c.is_module)
            .expect("Impl module class");
        assert!(obj.member("tag").is_some(), "Impl.tag");
    }
}
