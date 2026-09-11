//! The types nsc's pattern-match analysis reasons with, and the operations
//! it needs on them: `checkableType`, `<:<` between checkable types,
//! `enumerateSubtypes` over sealed hierarchies, and `Type.toString` for the
//! constants that end up in a counter-example.
//!
//! Anything this cannot express faithfully becomes `NTy::Unknown`, and the
//! analysis of a match that meets one gives up (reports nothing) rather than
//! reason about a type it has only approximated.

use crate::check::Typer;
use crate::symbol::{SymKind, SymbolTable};
use scala_rs_parser::ast::*;

/// A constant (nsc `Constant`), equal by tag and value; floating-point
/// values compare by bits, as nsc's `Constant.equals` does.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CVal {
    Unit,
    Bool(bool),
    Byte(i8),
    Short(i16),
    Char(char),
    Int(i32),
    Long(i64),
    Float(u32),
    Double(u64),
    Str(String),
    Null,
}

impl CVal {
    pub(crate) fn from_lit(l: &Lit) -> Option<CVal> {
        Some(match l {
            Lit::Unit => CVal::Unit,
            Lit::Boolean(b) => CVal::Bool(*b),
            Lit::Int(n) => CVal::Int(*n),
            Lit::Long(n) => CVal::Long(*n),
            Lit::Float(f) => CVal::Float(f.to_bits()),
            Lit::Double(d) => CVal::Double(d.to_bits()),
            Lit::Char(c) => CVal::Char(*c),
            Lit::String(s) => CVal::Str(s.clone()),
            Lit::Null => CVal::Null,
            Lit::Symbol(_) => return None,
        })
    }

    /// nsc `Constant.tag`.
    fn tag(&self) -> i32 {
        match self {
            CVal::Unit => 1,
            CVal::Bool(_) => 2,
            CVal::Byte(_) => 3,
            CVal::Short(_) => 4,
            CVal::Char(_) => 5,
            CVal::Int(_) => 6,
            CVal::Long(_) => 7,
            CVal::Float(_) => 8,
            CVal::Double(_) => 9,
            CVal::Str(_) => 10,
            CVal::Null => 11,
        }
    }

    /// nsc `Constant.hashCode` (a `MurmurHash3` of tag and value).
    pub(crate) fn scala_hash(&self) -> i32 {
        use crate::scala_coll::{java_string_hash, murmur_finalize, murmur_mix};
        let mut h = 17;
        h = murmur_mix(h, self.tag());
        let value_hash = match self {
            CVal::Null => 0,
            CVal::Unit => 0, // `BoxedUnit.UNIT.hashCode` is 0
            CVal::Bool(b) => {
                if *b {
                    1231
                } else {
                    1237
                }
            }
            CVal::Byte(n) => *n as i32,
            CVal::Short(n) => *n as i32,
            CVal::Char(c) => *c as i32,
            CVal::Int(n) => *n,
            CVal::Long(n) => ((*n as u64) ^ ((*n as u64) >> 32)) as i32,
            CVal::Float(bits) => *bits as i32,
            CVal::Double(bits) => (bits ^ (bits >> 32)) as i32,
            CVal::Str(s) => java_string_hash(s),
        };
        h = murmur_mix(h, value_hash);
        murmur_finalize(h, 2)
    }

    /// nsc `Constant.escapedStringValue` (what a literal tree prints as).
    pub(crate) fn escaped(&self) -> String {
        fn esc_char(c: char, out: &mut String) {
            match c {
                '\u{8}' => out.push_str("\\b"),
                '\t' => out.push_str("\\t"),
                '\n' => out.push_str("\\n"),
                '\u{c}' => out.push_str("\\f"),
                '\r' => out.push_str("\\r"),
                '"' => out.push_str("\\\""),
                '\'' => out.push_str("\\'"),
                '\\' => out.push_str("\\\\"),
                c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
                c => out.push(c),
            }
        }
        match self {
            CVal::Unit => "()".to_string(),
            CVal::Bool(b) => b.to_string(),
            CVal::Byte(n) => n.to_string(),
            CVal::Short(n) => n.to_string(),
            CVal::Int(n) => n.to_string(),
            CVal::Long(n) => format!("{n}L"),
            CVal::Float(bits) => format!("{}f", java_float(f32::from_bits(*bits) as f64, true)),
            CVal::Double(bits) => java_float(f64::from_bits(*bits), false),
            CVal::Char(c) => {
                let mut s = String::from("'");
                esc_char(*c, &mut s);
                s.push('\'');
                s
            }
            CVal::Str(v) => {
                let mut s = String::from("\"");
                for c in v.chars() {
                    esc_char(c, &mut s);
                }
                s.push('"');
                s
            }
            CVal::Null => "null".to_string(),
        }
    }
}

/// `Double.toString` / `Float.toString` for the values a pattern literal
/// usually holds.
fn java_float(v: f64, float: bool) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    let s = if float {
        format!("{}", v as f32)
    } else {
        format!("{v}")
    };
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{s}.0")
    }
}

/// A type as the analysis sees it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NTy {
    /// A class type; `args` are `Wild` after `checkable`.
    Class(SymbolId, Vec<NTy>),
    Wild,
    /// `o.type` for an object: the module class.
    Module(SymbolId),
    /// `ConstantType(c)`.
    Const(CVal),
    /// `p.type` for a stable value (not an object).
    Single(SymbolId),
    /// A fresh existential subtype of the bound (`uniqueTpForTree`).
    Fresh(u32, Box<NTy>),
    Any,
    AnyVal,
    AnyRef,
    Null,
    Nothing,
    Unknown,
}

impl NTy {
    pub(crate) fn is_unknown(&self) -> bool {
        match self {
            NTy::Unknown => true,
            NTy::Class(_, args) => args.iter().any(|a| a.is_unknown()),
            NTy::Fresh(_, b) => b.is_unknown(),
            _ => false,
        }
    }

    pub(crate) fn is_singleton(&self) -> bool {
        matches!(self, NTy::Module(_) | NTy::Const(_) | NTy::Single(_))
    }
}

/// The analysis' view of the symbol table.
pub(crate) struct Types<'a> {
    pub(crate) st: &'a SymbolTable,
}

impl<'a> Types<'a> {
    pub(crate) fn tuple_class(&self, n: usize) -> Option<SymbolId> {
        crate::classpath::find_by_jvm(self.st, &format!("scala/Tuple{n}"))
    }

    /// Our `Type` as an `NTy`.
    pub(crate) fn of(&self, ty: &Type) -> NTy {
        let st = self.st;
        match ty {
            Type::Unit => NTy::Class(st.unit_sym, vec![]),
            Type::Boolean => NTy::Class(st.boolean_sym, vec![]),
            Type::Byte => NTy::Class(st.byte_sym, vec![]),
            Type::Short => NTy::Class(st.short_sym, vec![]),
            Type::Int => NTy::Class(st.int_sym, vec![]),
            Type::Long => NTy::Class(st.long_sym, vec![]),
            Type::Float => NTy::Class(st.float_sym, vec![]),
            Type::Double => NTy::Class(st.double_sym, vec![]),
            Type::Char => NTy::Class(st.char_sym, vec![]),
            Type::String => NTy::Class(st.string_sym, vec![]),
            Type::Any => NTy::Any,
            Type::AnyRef | Type::JavaObject => NTy::AnyRef,
            Type::AnyVal => NTy::AnyVal,
            Type::Null => NTy::Null,
            Type::Nothing => NTy::Nothing,
            Type::Wildcard | Type::BoundedWildcard { .. } => NTy::Wild,
            Type::Constant(l) => match CVal::from_lit(l) {
                Some(CVal::Null) => NTy::Null,
                Some(c) => NTy::Const(c),
                None => NTy::Unknown,
            },
            Type::Annotated { tpe, .. } => self.of(tpe),
            Type::Array(e) => NTy::Class(st.array_sym, vec![self.of(e)]),
            Type::Tuple(ts) => match self.tuple_class(ts.len()) {
                Some(c) => NTy::Class(c, ts.iter().map(|t| self.of(t)).collect()),
                None => NTy::Unknown,
            },
            Type::Class { sym, args } => {
                let s = st.get(*sym);
                match s.kind {
                    SymKind::Class => {
                        if st.is_array_class(*sym) {
                            return NTy::Class(st.array_sym, args.iter().map(|a| self.of(a)).collect());
                        }
                        NTy::Class(*sym, args.iter().map(|a| self.of(a)).collect())
                    }
                    SymKind::ModuleClass => NTy::Module(*sym),
                    SymKind::Module => NTy::Module(st.module_class_of(*sym)),
                    _ => NTy::Unknown,
                }
            }
            Type::ModuleRef(m) => {
                let s = st.get(*m);
                match s.kind {
                    SymKind::Module => NTy::Module(st.module_class_of(*m)),
                    SymKind::ModuleClass => NTy::Module(*m),
                    _ => NTy::Unknown,
                }
            }
            Type::SingleType { sym, .. } => {
                let s = st.get(*sym);
                match s.kind {
                    SymKind::Module => NTy::Module(st.module_class_of(*sym)),
                    SymKind::ModuleClass => NTy::Module(*sym),
                    SymKind::Term => NTy::Single(*sym),
                    _ => NTy::Unknown,
                }
            }
            Type::Function { params, ret } => {
                match crate::classpath::find_by_jvm(st, &format!("scala/Function{}", params.len())) {
                    Some(c) => {
                        let mut args: Vec<NTy> = params.iter().map(|p| self.of(p)).collect();
                        args.push(self.of(ret));
                        NTy::Class(c, args)
                    }
                    None => NTy::Unknown,
                }
            }
            // Type parameters, abstract types, refinements, methods, ...:
            // the analysis does not model them.
            _ => NTy::Unknown,
        }
    }

    /// Back to our `Type`, for the symbol table's own subtyping.
    fn to_type(&self, t: &NTy) -> Option<Type> {
        let st = self.st;
        Some(match t {
            NTy::Class(s, args) => {
                if *s == st.array_sym {
                    return Some(Type::Array(Box::new(self.to_type(args.first()?)?)));
                }
                let prim = [
                    (st.unit_sym, Type::Unit),
                    (st.boolean_sym, Type::Boolean),
                    (st.byte_sym, Type::Byte),
                    (st.short_sym, Type::Short),
                    (st.int_sym, Type::Int),
                    (st.long_sym, Type::Long),
                    (st.float_sym, Type::Float),
                    (st.double_sym, Type::Double),
                    (st.char_sym, Type::Char),
                    (st.string_sym, Type::String),
                ];
                if let Some((_, p)) = prim.iter().find(|(ps, _)| ps == s) {
                    return Some(p.clone());
                }
                let mut targs = Vec::new();
                for a in args {
                    targs.push(self.to_type(a)?);
                }
                Type::Class { sym: *s, args: targs }
            }
            NTy::Wild => Type::Wildcard,
            NTy::Any => Type::Any,
            NTy::AnyVal => Type::AnyVal,
            NTy::AnyRef => Type::AnyRef,
            NTy::Null => Type::Null,
            NTy::Nothing => Type::Nothing,
            _ => return None,
        })
    }

    /// nsc `checkableType`: type arguments become wildcards (except
    /// `Array`'s).
    pub(crate) fn checkable(&self, t: &NTy) -> NTy {
        match t {
            NTy::Class(s, args) if !args.is_empty() && *s != self.st.array_sym => {
                NTy::Class(*s, vec![NTy::Wild; args.len()])
            }
            NTy::Class(s, args) => NTy::Class(*s, args.iter().map(|a| self.checkable(a)).collect()),
            NTy::Fresh(i, b) => NTy::Fresh(*i, Box::new(self.checkable(b))),
            other => other.clone(),
        }
    }

    pub(crate) fn is_primitive_class(&self, s: SymbolId) -> bool {
        self.st.is_primitive_value_class(s)
    }

    /// nsc `isPrimitiveValueType`.
    pub(crate) fn is_primitive_value_type(&self, t: &NTy) -> bool {
        match t {
            NTy::Class(s, _) => self.is_primitive_class(*s),
            NTy::Const(c) => !matches!(c, CVal::Str(_) | CVal::Null),
            _ => false,
        }
    }

    /// The class of a constant's type.
    pub(crate) fn const_class(&self, c: &CVal) -> SymbolId {
        let st = self.st;
        match c {
            CVal::Unit => st.unit_sym,
            CVal::Bool(_) => st.boolean_sym,
            CVal::Byte(_) => st.byte_sym,
            CVal::Short(_) => st.short_sym,
            CVal::Char(_) => st.char_sym,
            CVal::Int(_) => st.int_sym,
            CVal::Long(_) => st.long_sym,
            CVal::Float(_) => st.float_sym,
            CVal::Double(_) => st.double_sym,
            CVal::Str(_) => st.string_sym,
            CVal::Null => SymbolId::NONE,
        }
    }

    /// nsc `tp.widen`.
    pub(crate) fn widen(&self, t: &NTy) -> NTy {
        match t {
            NTy::Const(c) => NTy::Class(self.const_class(c), vec![]),
            NTy::Single(s) => self.of(&self.value_type(*s)),
            other => other.clone(),
        }
    }

    /// The (result) type of a value symbol.
    pub(crate) fn value_type(&self, s: SymbolId) -> Type {
        match &self.st.get(s).ty {
            Type::Method { paramss, ret } if paramss.iter().all(|c| c.is_empty()) => (**ret).clone(),
            t => t.clone(),
        }
    }

    /// nsc `typeSymbol`, `None` when there is none we know.
    pub(crate) fn type_symbol(&self, t: &NTy) -> Option<SymbolId> {
        match t {
            NTy::Class(s, _) => Some(*s),
            NTy::Module(m) => Some(*m),
            NTy::Const(c) if *c != CVal::Null => Some(self.const_class(c)),
            NTy::Single(_) => self.type_symbol(&self.widen(t)),
            NTy::Fresh(_, b) => self.type_symbol(b),
            NTy::Any => Some(self.st.any_sym),
            NTy::AnyRef => Some(self.st.anyref_sym),
            NTy::AnyVal => Some(self.st.anyval_sym),
            _ => None,
        }
    }

    fn is_reference(&self, t: &NTy) -> bool {
        match t {
            NTy::Class(s, _) => !self.is_primitive_class(*s) && *s != self.st.anyval_sym,
            NTy::Module(_) | NTy::AnyRef | NTy::Null => true,
            NTy::Const(c) => matches!(c, CVal::Str(_)),
            NTy::Single(_) => self.is_reference(&self.widen(t)),
            NTy::Fresh(_, b) => self.is_reference(b),
            _ => false,
        }
    }

    /// `a <:< b`, `None` when this cannot tell.
    pub(crate) fn sub(&self, a: &NTy, b: &NTy) -> Option<bool> {
        if a == b {
            return Some(true);
        }
        if a.is_unknown() || b.is_unknown() {
            return None;
        }
        match (a, b) {
            (_, NTy::Any) | (NTy::Nothing, _) => Some(true),
            (_, NTy::Wild) | (NTy::Wild, _) => Some(true),
            (NTy::Null, _) => Some(self.is_reference(b) || matches!(b, NTy::Null)),
            (_, NTy::Nothing) | (_, NTy::Null) => Some(false),
            (_, NTy::AnyRef) => Some(self.is_reference(a)),
            (_, NTy::AnyVal) => Some(!self.is_reference(a) && !matches!(a, NTy::Any)),
            (NTy::Any | NTy::AnyRef | NTy::AnyVal, _) => Some(false),
            (NTy::Const(c), _) => {
                if matches!(b, NTy::Const(_) | NTy::Single(_) | NTy::Module(_) | NTy::Fresh(..)) {
                    return Some(false);
                }
                self.sub(&NTy::Class(self.const_class(c), vec![]), b)
            }
            (NTy::Single(_), _) => {
                if matches!(b, NTy::Single(_) | NTy::Const(_) | NTy::Module(_) | NTy::Fresh(..)) {
                    return Some(false);
                }
                let w = self.widen(a);
                if w.is_unknown() {
                    return None;
                }
                self.sub(&w, b)
            }
            (NTy::Fresh(_, bound), _) => {
                if matches!(b, NTy::Single(_) | NTy::Const(_) | NTy::Module(_) | NTy::Fresh(..)) {
                    return Some(false);
                }
                self.sub(bound, b)
            }
            (NTy::Module(m), NTy::Class(..)) => {
                // An object is an instance of its module class's parents.
                let parents = self.st.get(*m).parents.clone();
                let mut unsure = false;
                for p in &parents {
                    match self.sub(&self.of(p), b) {
                        Some(true) => return Some(true),
                        None => unsure = true,
                        Some(false) => {}
                    }
                }
                if unsure {
                    None
                } else {
                    Some(false)
                }
            }
            (NTy::Module(_), _) => Some(false),
            (NTy::Class(..), NTy::Module(_) | NTy::Const(_) | NTy::Single(_) | NTy::Fresh(..)) => {
                Some(false)
            }
            (NTy::Class(s, sargs), NTy::Class(t, targs)) => self.class_sub(*s, sargs, *t, targs),
            _ => None,
        }
    }

    /// `s[sargs] <:< t[targs]` for class types.
    fn class_sub(&self, s: SymbolId, sargs: &[NTy], t: SymbolId, targs: &[NTy]) -> Option<bool> {
        let st = self.st;
        if s == t {
            return self.args_conform(t, sargs, targs);
        }
        // Primitive and String classes relate to nothing but themselves.
        if self.is_primitive_class(s) || self.is_primitive_class(t) {
            return Some(false);
        }
        if t == st.object_sym || t == st.anyref_sym {
            return Some(true);
        }
        let sty = match self.to_type(&NTy::Class(s, sargs.to_vec())) {
            Some(t) => t,
            None => return None,
        };
        for bt in st.base_type_seq(&sty) {
            if let Type::Class { sym, args } = &bt {
                if *sym == t {
                    let bargs: Vec<NTy> = args.iter().map(|a| self.of(a)).collect();
                    return self.args_conform(t, &bargs, targs);
                }
            }
        }
        if crate::pickle_supply::inherits_from(st, s, t) {
            // A parent our base-type walk did not reach: we know the classes
            // relate but not how their arguments do.
            if targs.iter().all(|a| *a == NTy::Wild) {
                return Some(true);
            }
            return None;
        }
        Some(false)
    }

    fn args_conform(&self, t: SymbolId, sargs: &[NTy], targs: &[NTy]) -> Option<bool> {
        if targs.is_empty() || sargs.is_empty() {
            return Some(true);
        }
        if sargs.len() != targs.len() {
            return None;
        }
        let tparams = self.st.get(t).tparams.clone();
        for (i, (sa, ta)) in sargs.iter().zip(targs).enumerate() {
            if matches!(sa, NTy::Wild) || matches!(ta, NTy::Wild) || sa == ta {
                continue;
            }
            if sa.is_unknown() || ta.is_unknown() {
                // A type parameter of the subclass stands in for "anything".
                return None;
            }
            let flags = tparams.get(i).map(|p| self.st.get(*p).flags).unwrap_or_default();
            let ok = if flags.contains(Flags::COVARIANT) {
                self.sub(sa, ta)?
            } else if flags.contains(Flags::CONTRAVARIANT) {
                self.sub(ta, sa)?
            } else {
                self.sub(sa, ta)? && self.sub(ta, sa)?
            };
            if !ok {
                return Some(false);
            }
        }
        Some(true)
    }

    /// nsc `instanceOfTpImplies`.
    pub(crate) fn instance_of_implies(&self, tp: &NTy, implied: &NTy) -> Option<bool> {
        let value = self.is_primitive_value_type(tp);
        let normalized = if (value && *implied == NTy::AnyVal) || (!value && *implied == NTy::AnyRef) {
            NTy::Any
        } else {
            implied.clone()
        };
        self.sub(tp, &normalized)
    }

    /// nsc `Type.toString` for the types a counter-example or a constant
    /// prints.
    pub(crate) fn show(&self, t: &NTy) -> String {
        let st = self.st;
        match t {
            NTy::Class(s, args) => {
                let tuple_arity = st.get(*s).name.strip_prefix("Tuple").and_then(|n| n.parse::<usize>().ok());
                if tuple_arity.is_some_and(|n| n == args.len() && n > 1) && is_scala_owned(st, *s)
                    && args.iter().all(|a| *a != NTy::Wild)
                {
                    return format!(
                        "({})",
                        args.iter().map(|a| self.show(a)).collect::<Vec<_>>().join(", ")
                    );
                }
                let mut s = self.class_name(*s);
                if !args.is_empty() {
                    s.push('[');
                    s.push_str(&args.iter().map(|a| self.show(a)).collect::<Vec<_>>().join(","));
                    s.push(']');
                }
                s
            }
            NTy::Wild => "?".into(),
            NTy::Module(m) => format!("{}.type", self.class_name(*m).trim_end_matches('$')),
            NTy::Const(c) => {
                let cls = self.class_name(self.const_class(c));
                format!("{cls}({})", c.escaped())
            }
            NTy::Single(s) => format!("{}.type", st.get(*s).name),
            NTy::Fresh(_, b) => format!("?{}", self.show(b)),
            NTy::Any => "Any".into(),
            NTy::AnyVal => "AnyVal".into(),
            NTy::AnyRef => "AnyRef".into(),
            NTy::Null => "Null".into(),
            NTy::Nothing => "Nothing".into(),
            NTy::Unknown => "<unknown>".into(),
        }
    }

    /// A class's name as nsc prints it: the prefix is omitted for the
    /// `scala` and `java.lang` packages and `Predef`; a class nested in an
    /// object prints `Outer.Name`.
    pub(crate) fn class_name(&self, s: SymbolId) -> String {
        let st = self.st;
        let sym = st.get(s);
        let name = sym.name.trim_end_matches('$').to_string();
        let jvm = &sym.jvm_name;
        let pkg = jvm.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        let simple = jvm.rsplit('/').next().unwrap_or("");
        let omit = matches!(pkg, "" | "scala" | "java/lang");
        let owner = sym.owner;
        if !owner.is_none() && matches!(st.get(owner).kind, SymKind::ModuleClass | SymKind::Module) {
            let o = st.get(owner);
            if o.name != "Predef" && o.name != "package" && simple.contains('$') {
                return format!("{}.{name}", self.class_name(owner));
            }
        }
        if omit {
            name
        } else {
            format!("{}.{name}", pkg.replace('/', "."))
        }
    }
}

fn is_scala_owned(st: &SymbolTable, s: SymbolId) -> bool {
    st.get(s).jvm_name.starts_with("scala/")
}

/// A library class's pickled flags (`pflags`): the prelude's hand-written
/// `Option` and `List` do not carry `sealed` / `abstract`, the pickles do.
pub(crate) fn pickled_flags(t: &mut Typer, cls: SymbolId) -> Option<u64> {
    if !t.library_abi || cls.is_none() || !t.st.get(cls).jvm_name.starts_with("scala/") {
        return None;
    }
    t.pickle.class_sig_of(&t.st, &mut t.binary, cls).map(|s| s.flags)
}

/// The sealed children of a class: the source's own record, or the pickle's
/// `CHILDREN` for a library class. `None` when a library class's children
/// cannot be read (the analysis then gives up on the match).
pub(crate) fn sealed_children(t: &mut Typer, cls: SymbolId) -> Option<Vec<SymbolId>> {
    let s = t.st.get(cls);
    if !s.children.is_empty() {
        return Some(s.children.clone());
    }
    if !t.library_abi || !s.jvm_name.starts_with("scala/") {
        // A source sealed class with no subclass at all, or one this run
        // cannot read the pickle of.
        return if s.jvm_name.starts_with("scala/") { None } else { Some(Vec::new()) };
    }
    let sig = t.pickle.class_sig_of(&t.st, &mut t.binary, cls)?;
    let mut out = Vec::new();
    for (full, module) in &sig.children {
        let internal = full.replace('.', "/");
        let id = t.pickle.ensure_class(&mut t.st, &mut t.binary, full, *module)?;
        let id = if *module {
            let s = t.st.get(id);
            if s.kind == SymKind::Module {
                t.st.module_class_of(id)
            } else {
                id
            }
        } else {
            id
        };
        let _ = internal;
        out.push(id);
    }
    Some(out)
}
