//! Complete standard-library members from their `ScalaSignature` pickles, on
//! demand, when the hand-written prelude does not have them.
//!
//! `prelude.rs` declares library members by hand. That does not scale to a
//! 2.13-compatible surface, so this module fills the gaps: when member
//! resolution on a `scala.*` receiver fails outright, the receiver's pickle is
//! read (and its parents', through `scala-rs-pickle`'s `SigCache`) and the
//! missing member is installed on the receiver's class symbol.
//!
//! Three rules keep it honest:
//!
//! 1. **The prelude always wins.** This runs only when `lookup_member` found
//!    *nothing*, so a hand-written declaration is never shadowed or replaced.
//! 2. **A member we cannot express is not supplied.** If the pickled type does
//!    not map onto `scala_rs_parser::Type`, or if we cannot pin down the erased
//!    JVM descriptor to call it with, the member is skipped and the user gets
//!    the usual "is not a member" error. A wrong type would be worse than none.
//! 3. **Nothing is read ahead of time.** One classfile per (receiver, name)
//!    miss, cached.

use std::collections::{HashMap, HashSet};

use scala_rs_parser::{Flags, SymbolId, Type};
use scala_rs_pickle::sym::{MemberKind, SigCache, SigType};
use scala_rs_pickle::ClassSource;

use crate::javaclass::{parse_java_classfile, BinaryIndex, JavaClass};
use crate::symbol::{SymKind, SymbolTable};

const ACC_STATIC: u16 = 0x0008;
const ACC_BRIDGE: u16 = 0x0040;
const ACC_SYNTHETIC: u16 = 0x1000;

/// A `ClassSource` view of the typer's `BinaryIndex`.
struct BinSource<'a>(&'a mut BinaryIndex);

impl ClassSource for BinSource<'_> {
    fn class_bytes(&mut self, internal_name: &str) -> Option<Vec<u8>> {
        self.0.find_class(internal_name).ok().flatten()
    }
}

#[derive(Default)]
pub struct PickleSupply {
    sigs: SigCache,
    /// `(receiver class, member name)` pairs already attempted, so a miss
    /// costs one lookup, not one per mention.
    tried: HashSet<(u32, String)>,
    /// Parsed classfiles, for erased descriptors.
    classes: HashMap<String, Option<JavaClass>>,
}

impl PickleSupply {
    pub fn new() -> Self {
        PickleSupply::default()
    }

    /// Try to install `name` on `class_sym` from the library pickles.
    /// Returns true if at least one member was installed.
    pub fn complete(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        name: &str,
    ) -> bool {
        if class_sym.is_none() || name.is_empty() {
            return false;
        }
        if !self.tried.insert((class_sym.0, name.to_string())) {
            return false;
        }
        // Operator members are mangled on the JVM (`++` is `$plus$plus`); until
        // that encoding is shared with the backend, only plain names are safe
        // to resolve a descriptor for.
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
        let sym = st.get(class_sym);
        if !sym.is_class_like() {
            return false;
        }
        let internal = sym.jvm_name.clone();
        // Scoped to the standard library: those are the pickles we validate
        // against, and a user classfile on `-cp` already has its own path in.
        if !internal.starts_with("scala/") {
            return false;
        }
        let is_module = sym.kind == SymKind::ModuleClass;
        let full = internal.trim_end_matches('$').replace('/', ".");

        let (hits, _errs) = {
            let mut src = BinSource(bin);
            self.sigs.lookup(&mut src, &full, is_module, name)
        };
        if hits.is_empty() {
            return false;
        }

        // The receiver's own type parameters are the vocabulary the looked-up
        // types are already expressed in.
        let mut class_scope: HashMap<String, Type> = HashMap::new();
        for tp in &st.get(class_sym).tparams {
            class_scope.insert(st.get(*tp).name.clone(), Type::TypeParam(*tp));
        }

        let mut installed = 0usize;
        let mut seen_shapes: HashSet<String> = HashSet::new();
        for hit in hits {
            let m = hit.member;
            if m.kind != MemberKind::Def || !m.is_public_api() {
                continue;
            }
            let Some(shape) = read_shape(&m.ty) else {
                continue;
            };
            // The same member is reachable through several parents; keep one.
            let key = format!("{}/{}", shape.tparams.len(), shape.arity());
            if !seen_shapes.insert(key) {
                continue;
            }
            if self.install(st, bin, class_sym, &internal, name, &shape, &class_scope) {
                installed += 1;
            }
        }
        installed > 0
    }

    #[allow(clippy::too_many_arguments)]
    fn install(
        &mut self,
        st: &mut SymbolTable,
        bin: &mut BinaryIndex,
        class_sym: SymbolId,
        internal: &str,
        name: &str,
        shape: &Shape,
        class_scope: &HashMap<String, Type>,
    ) -> bool {
        // The erased descriptor comes from the classfile itself rather than
        // from re-deriving scalac's erasure: the bytes are the truth, and a
        // descriptor we merely guessed would fail to link.
        let Some(desc) = self.erased_desc(bin, internal, name, shape.arity()) else {
            return false;
        };

        // Allocated ownerless, so a conversion failure leaves nothing behind:
        // `SymbolTable::alloc` pushes into the owner's member list.
        let m = st.alloc(name, SymbolId::NONE, SymKind::Method, Flags::EMPTY, desc);

        let mut scope = class_scope.clone();
        let mut tparams = Vec::new();
        for tp in &shape.tparams {
            let id = st.alloc(&tp.name, m, SymKind::TypeParam, Flags::EMPTY, "");
            st.get_mut(id).ty = Type::TypeParam(id);
            scope.insert(tp.name.clone(), Type::TypeParam(id));
            tparams.push(id);
        }
        // Bounds are resolved after every parameter is in scope (`A <: B`).
        for (tp, id) in shape.tparams.iter().zip(tparams.iter().copied()) {
            if let Some(hi) = &tp.hi {
                if let Some(t) = conv(st, &scope, hi) {
                    st.get_mut(id).bound_hi = Some(t);
                }
            }
        }

        let mut paramss_ty: Vec<Vec<Type>> = Vec::new();
        let mut paramss_sym: Vec<Vec<SymbolId>> = Vec::new();
        for clause in &shape.clauses {
            let mut tys = Vec::new();
            let mut syms = Vec::new();
            for p in &clause.params {
                let Some(mut t) = conv(st, &scope, &p.ty) else {
                    return false;
                };
                if p.by_name && !matches!(t, Type::ByName(_)) {
                    t = Type::ByName(Box::new(t));
                }
                let flags = if clause.implicit {
                    Flags::PARAM.with(Flags::IMPLICIT)
                } else {
                    Flags::PARAM
                };
                let ps = st.alloc(&p.name, m, SymKind::Term, flags, "");
                st.get_mut(ps).ty = t.clone();
                tys.push(t);
                syms.push(ps);
            }
            paramss_ty.push(tys);
            paramss_sym.push(syms);
        }
        let Some(ret) = conv(st, &scope, &shape.ret) else {
            return false;
        };

        st.get_mut(m).tparams = tparams;
        st.get_mut(m).params = paramss_sym.iter().flatten().copied().collect();
        st.get_mut(m).paramss = paramss_sym;
        st.get_mut(m).ty = Type::Method {
            paramss: paramss_ty,
            ret: Box::new(ret),
        };
        st.get_mut(m).owner = class_sym;
        st.get_mut(class_sym).members.push(m);
        true
    }

    /// The declared descriptor of `name` with `arity` value parameters,
    /// searched from `internal` up through superclasses and interfaces.
    ///
    /// Returns `None` when nothing matches, or when two same-arity overloads
    /// tie at the same level: picking one arbitrarily would silently call the
    /// wrong method.
    fn erased_desc(
        &mut self,
        bin: &mut BinaryIndex,
        internal: &str,
        name: &str,
        arity: usize,
    ) -> Option<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut level = vec![internal.to_string()];
        for _ in 0..32 {
            if level.is_empty() {
                return None;
            }
            let mut hits: Vec<String> = Vec::new();
            let mut next = Vec::new();
            for cn in &level {
                if !seen.insert(cn.clone()) {
                    continue;
                }
                let Some(jc) = self.java_class(bin, cn) else {
                    continue;
                };
                for jm in &jc.methods {
                    if jm.name != name || jm.access & (ACC_BRIDGE | ACC_SYNTHETIC | ACC_STATIC) != 0
                    {
                        continue;
                    }
                    if desc_arity(&jm.desc) == Some(arity) && !hits.contains(&jm.desc) {
                        hits.push(jm.desc.clone());
                    }
                }
                if let Some(s) = &jc.super_name {
                    next.push(s.clone());
                }
                next.extend(jc.interfaces.iter().cloned());
            }
            match hits.len() {
                0 => {}
                1 => return Some(hits.remove(0)),
                _ => return None,
            }
            level = next;
        }
        None
    }

    fn java_class(&mut self, bin: &mut BinaryIndex, internal: &str) -> Option<&JavaClass> {
        if !self.classes.contains_key(internal) {
            let parsed = bin
                .find_class(internal)
                .ok()
                .flatten()
                .and_then(|b| parse_java_classfile(&b).ok());
            self.classes.insert(internal.to_string(), parsed);
        }
        self.classes.get(internal).and_then(|c| c.as_ref())
    }
}

// ---------------------------------------------------------------------------
// Pickled signature -> method shape
// ---------------------------------------------------------------------------

struct ShapeTParam {
    name: String,
    hi: Option<SigType>,
}

struct Param {
    name: String,
    ty: SigType,
    by_name: bool,
}

struct Clause {
    params: Vec<Param>,
    implicit: bool,
}

struct Shape {
    tparams: Vec<ShapeTParam>,
    clauses: Vec<Clause>,
    ret: SigType,
}

impl Shape {
    fn arity(&self) -> usize {
        self.clauses.iter().map(|c| c.params.len()).sum()
    }
}

/// Peel `POLYtpe` / `METHODtpe` layers into type parameters and parameter
/// clauses. nsc writes a parameterless `def` as a `POLYtpe` with no type
/// parameters (`NullaryMethodType`), which becomes an empty clause list.
fn read_shape(t: &SigType) -> Option<Shape> {
    let mut tparams = Vec::new();
    let mut clauses = Vec::new();
    let mut cur = t;
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 16 {
            return None;
        }
        match cur {
            SigType::Poly {
                tparams: tps,
                result,
            } => {
                for tp in tps {
                    tparams.push(ShapeTParam {
                        name: tp.name.clone(),
                        hi: match &tp.bounds {
                            SigType::Bounds { hi, .. } => Some((**hi).clone()),
                            _ => None,
                        },
                    });
                }
                cur = result;
            }
            SigType::Method {
                params,
                implicit,
                result,
            } => {
                clauses.push(Clause {
                    params: params
                        .iter()
                        .map(|p| Param {
                            name: p.name.clone(),
                            ty: p.ty.clone(),
                            by_name: p.by_name,
                        })
                        .collect(),
                    implicit: *implicit,
                });
                cur = result;
            }
            other => {
                return Some(Shape {
                    tparams,
                    clauses,
                    ret: other.clone(),
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SigType -> scala_rs_parser::Type
// ---------------------------------------------------------------------------

/// Map a pickled type onto the typer's. `None` means "cannot express this",
/// and the caller then declines to supply the member.
fn conv(st: &SymbolTable, scope: &HashMap<String, Type>, t: &SigType) -> Option<Type> {
    conv_at(st, scope, t, 0)
}

fn conv_at(
    st: &SymbolTable,
    scope: &HashMap<String, Type>,
    t: &SigType,
    depth: u32,
) -> Option<Type> {
    if depth > 24 {
        return None;
    }
    let d = depth + 1;
    match t {
        SigType::Annotated(inner) => conv_at(st, scope, inner, d),
        SigType::Existential { quantified, result } => {
            // `List[_]`: the quantified variables stand for wildcards.
            let mut inner = scope.clone();
            for q in quantified {
                inner.insert(q.name.clone(), Type::Wildcard);
            }
            conv_at(st, &inner, result, d)
        }
        SigType::Ref { sym, args } => conv_ref(st, scope, sym, args, d),
        // A `val`'s own type is fine, but the remaining forms (`this.type`,
        // singletons, `super`, bare bounds, refinements, literal types) have no
        // faithful counterpart here yet.
        _ => None,
    }
}

fn conv_ref(
    st: &SymbolTable,
    scope: &HashMap<String, Type>,
    sym: &str,
    args: &[SigType],
    d: u32,
) -> Option<Type> {
    if let Some(bound) = scope.get(sym) {
        // A type parameter used as a constructor (`CC[B]`) is higher-kinded;
        // not expressible here.
        return if args.is_empty() {
            Some(bound.clone())
        } else {
            None
        };
    }
    let conv_args = |st: &SymbolTable| -> Option<Vec<Type>> {
        args.iter().map(|a| conv_at(st, scope, a, d)).collect()
    };
    match sym {
        "scala.Unit" => return Some(Type::Unit),
        "scala.Boolean" => return Some(Type::Boolean),
        "scala.Byte" => return Some(Type::Byte),
        "scala.Short" => return Some(Type::Short),
        "scala.Int" => return Some(Type::Int),
        "scala.Long" => return Some(Type::Long),
        "scala.Float" => return Some(Type::Float),
        "scala.Double" => return Some(Type::Double),
        "scala.Char" => return Some(Type::Char),
        "scala.Any" => return Some(Type::Any),
        "scala.AnyRef" | "java.lang.Object" => return Some(Type::AnyRef),
        "scala.AnyVal" => return Some(Type::AnyVal),
        "scala.Nothing" => return Some(Type::Nothing),
        "scala.Null" => return Some(Type::Null),
        "java.lang.String" | "scala.Predef.String" => return Some(Type::String),
        "scala.Array" => {
            let a = conv_args(st)?;
            return a.into_iter().next().map(|e| Type::Array(Box::new(e)));
        }
        "scala.<byname>" => {
            let a = conv_args(st)?;
            return a.into_iter().next().map(|e| Type::ByName(Box::new(e)));
        }
        "scala.<repeated>" => {
            let a = conv_args(st)?;
            return a.into_iter().next().map(|e| Type::Repeated(Box::new(e)));
        }
        _ => {}
    }
    if let Some(n) = sym.strip_prefix("scala.Function") {
        if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() {
            let mut a = conv_args(st)?;
            let ret = a.pop()?;
            return Some(Type::Function {
                params: a,
                ret: Box::new(ret),
            });
        }
    }
    if let Some(n) = sym.strip_prefix("scala.Tuple") {
        if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() {
            return Some(Type::Tuple(conv_args(st)?));
        }
    }
    // Anything else has to already be a class the symbol table knows: making
    // one up here would invent a type the backend cannot name.
    let internal = sym.replace('.', "/");
    let cls = crate::classpath::find_by_jvm(st, &internal)?;
    let a = conv_args(st)?;
    if a.len() != st.get(cls).tparams.len() {
        return None;
    }
    Some(Type::Class { sym: cls, args: a })
}

/// Number of parameters in a JVM method descriptor.
fn desc_arity(desc: &str) -> Option<usize> {
    let b = desc.as_bytes();
    if b.first() != Some(&b'(') {
        return None;
    }
    let mut i = 1;
    let mut n = 0;
    while i < b.len() && b[i] != b')' {
        while i < b.len() && b[i] == b'[' {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'L' {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
        }
        i += 1;
        n += 1;
    }
    if i >= b.len() {
        return None;
    }
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_arity() {
        assert_eq!(desc_arity("()V"), Some(0));
        assert_eq!(
            desc_arity("(Ljava/lang/String;)Ljava/lang/String;"),
            Some(1)
        );
        assert_eq!(
            desc_arity("(Ljava/lang/Object;Lscala/Function2;)Ljava/lang/Object;"),
            Some(2)
        );
        assert_eq!(desc_arity("(I[[Ljava/lang/String;J)V"), Some(3));
        assert_eq!(desc_arity("no"), None);
    }
}
