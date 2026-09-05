//! JVMS §4.7.9 `Signature` attributes: the generic type information erasure
//! throws away.
//!
//! Without them a class file describes only its erased shape, so
//! `Class#getGenericInterfaces`, `Method#toGenericString`, `Field#getGenericType`
//! and every Java client of Scala generics see raw types. nsc computes these
//! in `erasure.javaSig`, `enteringErasure` — that is, from the *un-erased*
//! types. This compiler has a destructive erasure phase
//! (`scala_rs_typer::erasure`) that rewrites symbol types in place, so the
//! strings have to be built before it runs: [`record_generic_signatures`] is
//! called from the driver between the pickler and erasure, exactly where nsc's
//! `enteringErasure` puts the reader.
//!
//! What is recorded is a *candidate*. The writer
//! ([`crate::gen::ClassBuilder::sign_last`]) attaches it to a member only when
//!
//! * the signature differs from the descriptor — an identical one carries no
//!   information and nsc omits it too; and
//! * erasing the signature (JLS: a type variable erases to its leftmost
//!   bound) reproduces that descriptor exactly.
//!
//! The second rule is what makes this safe to switch on for every emitted
//! class. Wherever this compiler's erasure and nsc's disagree — an
//! `Array[T <: AnyRef]` parameter, a refinement-typed one, a constructor that
//! takes an `$outer` — the check fails and the member simply keeps no
//! `Signature`, which is what it had before. A *wrong* signature is worse than
//! none, so nothing is emitted on a guess.

use crate::gen_desc::{class_internal, is_interface_sym, jvm_desc, jvm_desc_val};
use rustc_hash::FxHashMap as HashMap;
use scala_rs_parser::{SymbolId, Type};
use scala_rs_typer::{SymKind, SymbolTable};

/// A member's generic signature, plus everything needed to check it against
/// the descriptor it will be attached to.
#[derive(Clone, Debug, Default)]
pub struct GenericSignature {
    /// The JVMS §4.7.9 string. For a class this is only the formal type
    /// parameter section: the superclass and interfaces come from `parents`,
    /// because only the writer knows which ones it emitted and in what order.
    pub sig: String,
    /// Erasure of every type variable the signature can mention, including the
    /// ones declared by an enclosing class rather than by the signature
    /// itself. `T` -> `Ljava/lang/Object;`.
    pub tvars: Vec<(String, String)>,
    /// For a class: each parent's signature, keyed by the internal name it
    /// erases to.
    pub parents: Vec<(String, String)>,
}

/// Signatures for every method, field-bearing term and class of the run,
/// keyed by symbol.
pub type GenericSignatures = HashMap<SymbolId, GenericSignature>;

/// Build the run's generic signatures. Must be called *before*
/// [`scala_rs_typer::erasure::erase`], which destroys the type arguments and
/// type-parameter references this reads.
pub fn record_generic_signatures(st: &SymbolTable) -> GenericSignatures {
    let mut out: GenericSignatures = HashMap::default();
    for i in 1..st.symbols.len() {
        let id = SymbolId(i as u32);
        let s = st.get(id);
        match s.kind {
            SymKind::Method => {
                if let Some(g) = method_signature(st, id) {
                    out.insert(id, g);
                }
            }
            // Only a term a class owns can become a field or an accessor;
            // recording a signature for every local and parameter of the run
            // would be so much waste.
            SymKind::Term
                if matches!(st.get(s.owner).kind, SymKind::Class | SymKind::ModuleClass) =>
            {
                if let Some(g) = value_signature(st, id) {
                    out.insert(id, g);
                }
            }
            SymKind::Class | SymKind::ModuleClass => {
                if let Some(g) = class_signature(st, id) {
                    out.insert(id, g);
                }
            }
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// scopes
// ---------------------------------------------------------------------------

/// The type parameters a member's signature may spell as `T…;`: its own, plus
/// those of every enclosing class. Returns `None` when two of them share a
/// name, since a signature cannot tell those apart.
fn tvars_in_scope(st: &SymbolTable, sym: SymbolId) -> Option<Vec<SymbolId>> {
    let mut ids: Vec<SymbolId> = st.get(sym).tparams.clone();
    let mut owner = st.get(sym).owner;
    let mut guard = 0;
    while !owner.is_none() && guard < 64 {
        guard += 1;
        let o = st.get(owner);
        if matches!(o.kind, SymKind::Class | SymKind::ModuleClass) {
            ids.extend(o.tparams.iter().copied());
        }
        if matches!(o.kind, SymKind::Package | SymKind::NoSymbol) {
            break;
        }
        owner = o.owner;
    }
    ids.retain(|t| st.get(*t).kind == SymKind::TypeParam);
    let mut names: Vec<&str> = ids.iter().map(|t| st.get(*t).name.as_str()).collect();
    names.sort_unstable();
    let n = names.len();
    names.dedup();
    if names.len() != n {
        return None;
    }
    Some(ids)
}

struct Sig<'a> {
    st: &'a SymbolTable,
    /// Type parameters spelled as type variables, and the descriptor each
    /// erases to under the JLS rule (leftmost bound).
    tvars: HashMap<SymbolId, (String, String)>,
}

impl<'a> Sig<'a> {
    fn new(st: &'a SymbolTable, in_scope: &[SymbolId]) -> Sig<'a> {
        let mut tvars = HashMap::default();
        for t in in_scope {
            // A higher-kinded parameter (`F[_]`) is not a JVM type variable:
            // nsc's `isTypeParameterInSig` excludes it and falls back to the
            // erasure, so leaving it out of the map is what puts it there.
            if !st.get(*t).tparams.is_empty() {
                continue;
            }
            let name = st.get(*t).name.clone();
            tvars.insert(*t, (name, String::new()));
        }
        let mut me = Sig { st, tvars };
        // Fill in the erasures once the names are known: a bound may itself
        // mention another parameter of the same class (`B <: Seq[A]`).
        let ids: Vec<SymbolId> = me.tvars.keys().copied().collect();
        for id in ids {
            let e = me.tvar_erasure(id);
            if let Some(slot) = me.tvars.get_mut(&id) {
                slot.1 = e;
            }
        }
        me
    }

    fn tvar_list(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self.tvars.values().cloned().collect();
        v.sort();
        v
    }

    /// The bounds a formal type parameter declares, split the way JVMS §4.7.9
    /// wants them: at most one class bound, then any number of interface
    /// bounds.
    fn bounds_of(&self, id: SymbolId) -> (Option<Type>, Vec<Type>) {
        let hi = match &self.st.get(id).bound_hi {
            Some(h) => h.clone(),
            None => return (None, Vec::new()),
        };
        let parts: Vec<Type> = match &hi {
            Type::Refined { parents, .. } => parents.clone(),
            _ => vec![hi.clone()],
        };
        let mut cls = None;
        let mut ifaces = Vec::new();
        for p in parts {
            if self.is_interface(&p) {
                ifaces.push(p);
            } else if cls.is_none() {
                cls = Some(p);
            } else {
                // Two class bounds cannot both be written; the extra one is
                // dropped, as nsc's `boundsSig` does.
            }
        }
        (cls, ifaces)
    }

    fn is_interface(&self, ty: &Type) -> bool {
        match ty {
            Type::Class { sym, .. } | Type::ThisType(sym) => is_interface_sym(self.st, *sym),
            _ => false,
        }
    }

    /// JLS 4.6: a type variable erases to its leftmost bound.
    fn tvar_erasure(&self, id: SymbolId) -> String {
        let (cls, ifaces) = self.bounds_of(id);
        let leftmost = cls.or_else(|| ifaces.first().cloned());
        match leftmost {
            Some(t) => {
                // Only a class-shaped bound is expressible; anything else
                // (another parameter, a refinement) leaves `Object`, which is
                // also what an unbounded parameter erases to.
                match &t {
                    Type::Class { .. } | Type::ThisType(_) | Type::String | Type::Array(_) => {
                        jvm_desc_val(self.st, &t)
                    }
                    _ => "Ljava/lang/Object;".to_string(),
                }
            }
            None => "Ljava/lang/Object;".to_string(),
        }
    }

    /// The `<A:…B::…>` section. Every declared parameter appears, including a
    /// higher-kinded one: only *references* to it erase away (nsc's
    /// `isTypeParameterInSig`), and dropping its formal made a subclass's
    /// `LTCC<LSeq;>;` name one type argument more than `TCC` declared --
    /// `MalformedParameterizedTypeException` the moment anything reflected on
    /// it. Real scalac writes `<C:Ljava/lang/Object;>` for `class TCC[C[_]]`.
    fn formals(&self, tparams: &[SymbolId]) -> String {
        let mut s = String::new();
        for t in tparams {
            if self.st.get(*t).kind != SymKind::TypeParam {
                continue;
            }
            s.push_str(&self.st.get(*t).name);
            let (cls, ifaces) = self.bounds_of(*t);
            match &cls {
                Some(c) => {
                    s.push(':');
                    s.push_str(&self.boxed(c));
                }
                None => s.push(':'),
            }
            for i in &ifaces {
                s.push(':');
                s.push_str(&self.boxed(i));
            }
            if cls.is_none() && ifaces.is_empty() {
                s.push_str("Ljava/lang/Object;");
            }
        }
        if s.is_empty() {
            String::new()
        } else {
            format!("<{s}>")
        }
    }

    /// nsc's `boxedSig`: a type in a position where a primitive cannot stand
    /// (a type argument, a parent, a bound).
    fn boxed(&self, ty: &Type) -> String {
        self.jsig(ty, false, 0)
    }

    /// nsc's `argSig`: a type argument, which may be a wildcard.
    fn arg(&self, ty: &Type, depth: u32) -> String {
        match ty {
            Type::Wildcard => "*".to_string(),
            Type::BoundedWildcard { lo, hi } => match (lo, hi) {
                (_, Some(h)) if !is_top(h) => format!("+{}", self.jsig(h, false, depth + 1)),
                (Some(l), _) if !is_bottom(l) => format!("-{}", self.jsig(l, false, depth + 1)),
                _ => "*".to_string(),
            },
            _ => self.jsig(ty, false, depth + 1),
        }
    }

    fn args(&self, args: &[Type], depth: u32) -> String {
        if args.is_empty() {
            return String::new();
        }
        let mut s = String::from("<");
        for a in args {
            s.push_str(&self.arg(a, depth));
        }
        s.push('>');
        s
    }

    /// The signature of a type in a value position. Falls back to the erased
    /// descriptor wherever no faithful spelling exists — the writer's
    /// "signature equals descriptor" test then drops the whole thing, so a
    /// fallback never turns into a claim.
    fn jsig(&self, ty: &Type, primitive_ok: bool, depth: u32) -> String {
        if depth > 24 {
            return jvm_desc_val(self.st, ty);
        }
        match ty {
            Type::Boolean
            | Type::Byte
            | Type::Short
            | Type::Int
            | Type::Long
            | Type::Float
            | Type::Double
            | Type::Char => {
                if primitive_ok {
                    jvm_desc_val(self.st, ty)
                } else {
                    "Ljava/lang/Object;".to_string()
                }
            }
            Type::Constant(lit) => self.jsig(&Type::lit_underlying(lit), primitive_ok, depth),
            Type::Annotated { tpe, .. } => self.jsig(tpe, primitive_ok, depth),
            Type::Array(elem) => {
                // `Array[T]` for an unbounded `T` erases to `Object`, not to
                // an array type; take the erasure's word for which of the two
                // this is.
                let d = jvm_desc_val(self.st, ty);
                if !d.starts_with('[') {
                    return d;
                }
                let e = match elem.widen_constant() {
                    Type::Nothing => "Ljava/lang/Object;".to_string(),
                    _ => self.jsig(elem, true, depth + 1),
                };
                format!("[{e}")
            }
            Type::TypeParam(id) => match self.tvars.get(id) {
                Some((name, _)) => format!("T{name};"),
                None => jvm_desc_val(self.st, ty),
            },
            Type::Class { sym, args } => {
                // `Array[T]` read back from a class file signature arrives as
                // a class application (`SymbolTable::array_class_form`), and
                // its "internal name" is `[Ljava/lang/Object`, which is not a
                // class name at all: `L[Ljava/lang/Object;` is what slick's
                // `TypedCollectionTypeConstructor` came out with, and the JVM
                // rejected the signature outright.
                if let Some(a) = self.st.array_class_form(ty) {
                    return self.jsig(&a, primitive_ok, depth + 1);
                }
                // Unapplied, `Array` is a type *constructor* -- slick's
                // `TypedCollectionTypeConstructor[Array]`. nsc spells it
                // `Lscala/Array;`, a name with no class file behind it but a
                // well-formed signature; `[Ljava/lang/Object` is neither.
                if *sym == self.st.array_sym {
                    return format!("Lscala/Array{};", self.args(args, depth));
                }
                if self.st.is_value_class(*sym) {
                    if let Some(u) = self.st.value_class_underlying(*sym) {
                        if !(is_primitive(&u) && !primitive_ok) {
                            return self.jsig(&u, primitive_ok, depth + 1);
                        }
                    }
                }
                format!(
                    "L{}{};",
                    class_internal(self.st, *sym),
                    self.args(args, depth)
                )
            }
            Type::Function { params, ret } => {
                let mut a: Vec<Type> = params.clone();
                a.push((**ret).clone());
                format!("Lscala/Function{}{};", params.len(), self.args(&a, depth))
            }
            Type::Tuple(ts) => format!("Lscala/Tuple{}{};", ts.len(), self.args(ts, depth)),
            // uncurry rewrites a by-name parameter to `Function0[T]` and a
            // repeated one to `Seq[T]`; both keep their Scala spelling in the
            // symbol's type, and nsc's signature is of the rewritten shape.
            Type::ByName(t) => format!(
                "Lscala/Function0{};",
                self.args(std::slice::from_ref(t), depth)
            ),
            Type::Repeated(t) => format!(
                "Lscala/collection/immutable/Seq{};",
                self.args(std::slice::from_ref(t), depth)
            ),
            // nsc's `intersectionDominator`: a refinement is written as the
            // parent it erases to. `Long { type Tag = Nothing }` is a `long`,
            // not an `Object`, and dropping to the erasure fallback here cost
            // `run/t8756` its one remaining line.
            Type::Refined { parents, .. } if !parents.is_empty() => {
                let dom = parents
                    .iter()
                    .find(|p| !self.is_interface(p))
                    .unwrap_or(&parents[0]);
                self.jsig(dom, primitive_ok, depth + 1)
            }
            Type::SingleType { prefix, sym } => {
                let inner = self.st.get(*sym).ty.clone();
                if inner.is_no_type() {
                    self.jsig(prefix, primitive_ok, depth + 1)
                } else {
                    self.jsig(&inner, primitive_ok, depth + 1)
                }
            }
            // Everything else — `Any`, `AnyRef`, `Nothing`, `Null`, a
            // refinement, an abstract type member, a higher-kinded
            // application, an unresolved name — has no spelling beyond its
            // erasure, which is what nsc writes for it too.
            _ => jvm_desc_val(self.st, ty),
        }
    }

    /// The result position, where `Unit` really is `V`.
    fn result(&self, ty: &Type, depth: u32) -> String {
        match ty.widen_constant() {
            Type::Unit | Type::NoType => "V".to_string(),
            _ => self.jsig(ty, true, depth),
        }
    }
}

fn is_primitive(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Boolean
            | Type::Byte
            | Type::Short
            | Type::Int
            | Type::Long
            | Type::Float
            | Type::Double
            | Type::Char
            | Type::Unit
    )
}

fn is_top(ty: &Type) -> bool {
    matches!(ty, Type::Any | Type::AnyRef | Type::AnyVal)
}

fn is_bottom(ty: &Type) -> bool {
    matches!(ty, Type::Nothing | Type::Null)
}

// ---------------------------------------------------------------------------
// members
// ---------------------------------------------------------------------------

fn method_signature(st: &SymbolTable, id: SymbolId) -> Option<GenericSignature> {
    let s = st.get(id);
    // A method whose `jvm_name` is a literal descriptor was supplied from a
    // pickle or built by hand; its Scala type is not what the JVM sees.
    if s.jvm_name.starts_with('(') {
        return None;
    }
    let in_scope = tvars_in_scope(st, id)?;
    let sig = Sig::new(st, &in_scope);
    let (paramss, ret) = match &s.ty {
        Type::Method { paramss, ret } => (paramss.iter().flatten().cloned().collect(), ret.clone()),
        Type::Function { params, ret } => (params.clone(), ret.clone()),
        t => (Vec::new(), Box::new(t.clone())),
    };
    let params: Vec<Type> = if paramss.iter().any(|p| p.is_no_type() || p.is_error()) {
        s.params.iter().map(|p| st.get(*p).ty.clone()).collect()
    } else {
        paramss
    };
    let mut out = sig.formals(&s.tparams);
    out.push('(');
    for p in &params {
        out.push_str(&sig.jsig(p, true, 0));
    }
    out.push(')');
    // A constructor's result is `V`, whatever its symbol's type says.
    if s.name == "<init>" {
        out.push('V');
    } else {
        out.push_str(&sig.result(&ret, 0));
    }
    informative(out, sig.tvar_list())
}

/// The type signature of a `val` / `var`, used for the field and for its
/// accessors.
fn value_signature(st: &SymbolTable, id: SymbolId) -> Option<GenericSignature> {
    let s = st.get(id);
    if s.ty.is_no_type() || s.ty.is_error() {
        return None;
    }
    if matches!(s.ty, Type::Method { .. } | Type::Overload(_)) {
        return None;
    }
    let in_scope = tvars_in_scope(st, id)?;
    let sig = Sig::new(st, &in_scope);
    informative(sig.jsig(&s.ty, true, 0), sig.tvar_list())
}

/// A signature that erases to itself says nothing the descriptor does not, and
/// nsc does not emit one either.
fn informative(sig: String, tvars: Vec<(String, String)>) -> Option<GenericSignature> {
    if erase_signature(&sig, &tvars).as_deref() == Some(sig.as_str()) {
        return None;
    }
    Some(GenericSignature {
        sig,
        tvars,
        parents: Vec::new(),
    })
}

/// JVMS §4.7.9 class signature. Only the formal type parameter section is
/// built here: the superclass and interfaces have to appear in exactly the
/// order the class file lists them, and only the writer knows that order, so
/// each parent's signature is recorded against the internal name it erases to
/// and assembled in [`crate::gen::ClassBuilder::sign_class`].
fn class_signature(st: &SymbolTable, id: SymbolId) -> Option<GenericSignature> {
    let s = st.get(id);
    if s.tparams.is_empty() && !s.parents.iter().any(has_type_args) {
        return None;
    }
    let in_scope = tvars_in_scope(st, id)?;
    let sig = Sig::new(st, &in_scope);
    let parents = s
        .parents
        .iter()
        .filter_map(|p| {
            let d = jvm_desc(st, p);
            let internal = d.strip_prefix('L')?.strip_suffix(';')?.to_string();
            Some((internal, sig.boxed(p)))
        })
        .collect();
    Some(GenericSignature {
        sig: sig.formals(&s.tparams),
        tvars: sig.tvar_list(),
        parents,
    })
}

fn has_type_args(ty: &Type) -> bool {
    match ty {
        Type::Class { args, .. } => !args.is_empty(),
        Type::Named { args, .. } => !args.is_empty(),
        Type::Function { .. } | Type::Tuple(_) | Type::Applied { .. } => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// erasing a signature back to a descriptor
// ---------------------------------------------------------------------------

/// Erase a JVMS §4.7.9 signature to the descriptor it must agree with. JLS
/// 4.6: type arguments are dropped and a type variable becomes its leftmost
/// bound. Returns `None` for anything this does not recognise, which is a
/// refusal to vouch for the string rather than a claim that it is bad.
pub fn erase_signature(sig: &str, tvars: &[(String, String)]) -> Option<String> {
    let b = sig.as_bytes();
    let mut i = 0usize;
    // Skip the formal type parameter section, if any.
    if b.first() == Some(&b'<') {
        i = skip_formals(b, 0)?;
    }
    let mut out = String::new();
    if b.get(i) == Some(&b'(') {
        out.push('(');
        i += 1;
        while i < b.len() && b[i] != b')' {
            let (d, ni) = erase_one(b, i, tvars)?;
            out.push_str(&d);
            i = ni;
        }
        if b.get(i) != Some(&b')') {
            return None;
        }
        out.push(')');
        i += 1;
    }
    if b.get(i) == Some(&b'V') {
        out.push('V');
        i += 1;
    } else {
        let (d, ni) = erase_one(b, i, tvars)?;
        out.push_str(&d);
        i = ni;
    }
    if i != b.len() {
        return None;
    }
    Some(out)
}

/// Erase a class signature to the list of internal names it names, in order:
/// the superclass first, then every interface. The writer compares that list
/// against the `super_class` / `interfaces` it actually wrote, since
/// `getGenericSuperclass` and `getGenericInterfaces` match them positionally.
pub fn erase_class_signature(sig: &str, tvars: &[(String, String)]) -> Option<Vec<String>> {
    let b = sig.as_bytes();
    let mut i = 0usize;
    if b.first() == Some(&b'<') {
        i = skip_formals(b, 0)?;
    }
    let mut out = Vec::new();
    while i < b.len() {
        if b[i] != b'L' {
            return None;
        }
        let (d, ni) = erase_one(b, i, tvars)?;
        out.push(d.strip_prefix('L')?.strip_suffix(';')?.to_string());
        i = ni;
    }
    Some(out)
}

fn skip_formals(b: &[u8], mut i: usize) -> Option<usize> {
    // `<` Ident `:` FieldTypeSig? (`:` FieldTypeSig)* ... `>`
    i += 1;
    loop {
        if i >= b.len() {
            return None;
        }
        if b[i] == b'>' {
            return Some(i + 1);
        }
        // identifier
        while i < b.len() && b[i] != b':' {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        while b.get(i) == Some(&b':') {
            i += 1;
            if matches!(b.get(i), Some(b'L') | Some(b'T') | Some(b'[')) {
                i = skip_type(b, i)?;
            }
        }
    }
}

/// Walk past one type signature without erasing it. Separate from
/// [`erase_one`] because the places that only need to *skip* — a formal
/// parameter's bounds, a type argument — may mention type variables that are
/// none of the caller's business, and resolving them there is what made an
/// otherwise valid `(Lscala/collection/immutable/List<TU;>;)I` unparseable.
fn skip_sig(b: &[u8], mut i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' | b'V' => Some(i + 1),
        b'[' => skip_sig(b, i + 1),
        b'T' => {
            while i < b.len() && b[i] != b';' {
                i += 1;
            }
            (i < b.len()).then_some(i + 1)
        }
        b'L' => {
            i += 1;
            loop {
                match *b.get(i)? {
                    b';' => return Some(i + 1),
                    b'<' => i = skip_args(b, i)?,
                    // Same well-formedness net as `erase_one`: a type argument
                    // is skipped rather than erased, so this is the only place
                    // a malformed class name inside one is ever looked at.
                    b'[' | b'+' | b'*' | b'>' | b'(' | b')' => return None,
                    _ => i += 1,
                }
            }
        }
        _ => None,
    }
}

fn skip_type(b: &[u8], i: usize) -> Option<usize> {
    skip_sig(b, i)
}

/// Erase one type signature starting at `i`; returns the descriptor and the
/// index just past it.
fn erase_one(b: &[u8], mut i: usize, tvars: &[(String, String)]) -> Option<(String, usize)> {
    match *b.get(i)? {
        c @ (b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z') => {
            Some(((c as char).to_string(), i + 1))
        }
        b'[' => {
            let (e, ni) = erase_one(b, i + 1, tvars)?;
            Some((format!("[{e}"), ni))
        }
        b'T' => {
            let start = i + 1;
            let mut j = start;
            while j < b.len() && b[j] != b';' {
                j += 1;
            }
            if j >= b.len() {
                return None;
            }
            let name = std::str::from_utf8(&b[start..j]).ok()?;
            let d = tvars
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, d)| d.clone())?;
            Some((d, j + 1))
        }
        b'L' => {
            let mut name = String::new();
            i += 1;
            loop {
                match *b.get(i)? {
                    // An empty name is not a class either: `L;` would come
                    // from a symbol with no JVM name, and there is nothing to
                    // vouch for there.
                    b';' if name.is_empty() => return None,
                    b';' => return Some((format!("L{name};"), i + 1)),
                    b'<' => {
                        i = skip_args(b, i)?;
                    }
                    b'.' => {
                        // A nested class written `LOuter<…>.Inner;` is
                        // `Outer$Inner` after erasure.
                        name.push('$');
                        i += 1;
                    }
                    // Not a character an internal name can hold. This is the
                    // net under the writer: an `L[Ljava/lang/Object;` would
                    // parse happily as a class called `[Ljava` otherwise, and
                    // the JVM would reject the attribute at run time.
                    b'[' | b'+' | b'-' | b'*' | b'>' | b'(' | b')' => return None,
                    c => {
                        name.push(c as char);
                        i += 1;
                    }
                }
            }
        }
        _ => None,
    }
}

fn skip_args(b: &[u8], mut i: usize) -> Option<usize> {
    // at `<`
    i += 1;
    loop {
        match *b.get(i)? {
            b'>' => return Some(i + 1),
            b'*' => i += 1,
            b'+' | b'-' => i += 1,
            _ => i = skip_sig(b, i)?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tv() -> Vec<(String, String)> {
        vec![
            ("A".to_string(), "Ljava/lang/Object;".to_string()),
            (
                "B".to_string(),
                "Lscala/collection/immutable/Seq;".to_string(),
            ),
        ]
    }

    #[test]
    fn erases_a_method_signature() {
        assert_eq!(
            erase_signature("(LWrapper<[I>;)I", &[]).as_deref(),
            Some("(LWrapper;)I")
        );
        assert_eq!(
            erase_signature("<A:Ljava/lang/Object;>(TA;I)LBox<TA;>;", &tv()).as_deref(),
            Some("(Ljava/lang/Object;I)LBox;")
        );
        assert_eq!(
            erase_signature("(TB;)V", &tv()).as_deref(),
            Some("(Lscala/collection/immutable/Seq;)V")
        );
        assert_eq!(
            erase_signature("()Lscala/Option<*>;", &[]).as_deref(),
            Some("()Lscala/Option;")
        );
        assert_eq!(
            erase_signature(
                "()Lscala/collection/immutable/Map<*Ljava/lang/Object;>;",
                &[]
            )
            .as_deref(),
            Some("()Lscala/collection/immutable/Map;")
        );
    }

    #[test]
    fn erases_a_field_signature() {
        assert_eq!(
            erase_signature("TA;", &tv()).as_deref(),
            Some("Ljava/lang/Object;")
        );
        assert_eq!(
            erase_signature("Lscala/Option<Ljava/lang/String;>;", &[]).as_deref(),
            Some("Lscala/Option;")
        );
        assert_eq!(
            erase_signature("[TB;", &tv()).as_deref(),
            Some("[Lscala/collection/immutable/Seq;")
        );
    }

    #[test]
    fn an_unknown_variable_is_a_refusal() {
        assert_eq!(erase_signature("(TZZ;)V", &tv()), None);
        assert_eq!(erase_signature("(", &[]), None);
        assert_eq!(erase_signature("()Lfoo;junk", &[]), None);
    }

    #[test]
    fn erases_a_nested_class_signature() {
        assert_eq!(
            erase_signature("LOuter<Ljava/lang/String;>.Inner;", &[]).as_deref(),
            Some("LOuter$Inner;")
        );
    }
}
