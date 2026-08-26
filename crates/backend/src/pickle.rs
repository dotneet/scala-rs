//! nsc PickleFormat subset (major 5, minor 2) plus SID-10 ByteCodecs.
//!
//! This is enough for scala-rs to round-trip compiled classes/objects through
//! `ScalaSignature` and for scalac 2.13.16 to typecheck a `val`, a `def` with
//! parameters, `id[T]`, a `case class` (`new Point` + ctor field accessors +
//! companion apply `Point(x, y)` / term `Point` via `MODULESYM` in the class
//! pickle + extractor `unapply` so `x match { case Point(a, b) => … }`), and an
//! `object` method against our classfiles. It is **not** a full nsc pickle (no
//! existentials, annotation args, or the complete Flags long).
//!
//! nsc-facing details in this subset (must match `PickleBuffer` / `UnPickler`):
//! - pickle = major, minor, **nentries**, then `{ tag_Nat, len_Nat, body }`
//! - Nat / LongNat are **big-endian** base-128 (nsc `writeLongNat`)
//! - `NOPREFIXtpe` for primitive `TypeRef` prefixes; class type params use `THIStpe`
//! - `scala` / `java.lang` / `<empty>` `EXTMODCLASSref` (term names)
//! - objects pickle as `CLASSsym` + MODULE (type name), not `MODULEsym`
//! - `POLYtpe` is restpe first, then tparams; empty tparams = NullaryMethodType
//! - methods carry `METHOD`; vals are getters (`METHOD|STABLE|ACCESSOR` + `POLYtpe`)

use scala_rs_parser::{Flags, SymbolId, Type};
use scala_rs_typer::{SymKind, SymbolTable};

pub const MAJOR: u32 = 5;
pub const MINOR: u32 = 2;

pub const TERMNAME: u8 = 1;
pub const TYPENAME: u8 = 2;
pub const NONESYM: u8 = 3;
pub const TYPESYM: u8 = 4;
pub const CLASSSYM: u8 = 6;
pub const MODULESYM: u8 = 7;
pub const VALSYM: u8 = 8;
pub const EXTREF: u8 = 9;
pub const EXTMODCLASSREF: u8 = 10;
pub const NOTPE: u8 = 11;
pub const NOPREFIXTPE: u8 = 12;
pub const THISTPE: u8 = 13;
pub const TYPEREFTPE: u8 = 16;
pub const TYPEBOUNDSTPE: u8 = 17;
pub const CLASSINFOTPE: u8 = 19;
pub const METHODTPE: u8 = 20;
pub const POLYTPE: u8 = 21;

/// Pickled method, constructor, or val recovered by the subset unpickler.
#[derive(Clone, Debug)]
pub struct PickledMethod {
    pub name: String,
    pub param_names: Vec<String>,
    pub param_types: Vec<String>,
    pub ret: String,
    pub tparams: Vec<String>,
    pub is_val: bool,
    pub is_ctor: bool,
}

/// Pickled class or module class.
#[derive(Clone, Debug)]
pub struct PickledClass {
    pub name: String,
    pub is_module: bool,
    pub tparams: Vec<String>,
    pub methods: Vec<PickledMethod>,
}

// ---------------------------------------------------------------------------
// ByteCodecs (SID-10)
// ---------------------------------------------------------------------------

pub fn encode8to7(src: &[u8]) -> Vec<u8> {
    let srclen = src.len();
    let dstlen = (srclen * 8 + 6) / 7;
    let mut dst = vec![0u8; dstlen];
    let mut i = 0;
    let mut j = 0;
    while i + 6 < srclen {
        let mut inp = src[i] as i32;
        dst[j] = (inp & 0x7f) as u8;
        let mut out = inp >> 7;
        inp = src[i + 1] as i32;
        dst[j + 1] = (out | (inp << 1) & 0x7f) as u8;
        out = inp >> 6;
        inp = src[i + 2] as i32;
        dst[j + 2] = (out | (inp << 2) & 0x7f) as u8;
        out = inp >> 5;
        inp = src[i + 3] as i32;
        dst[j + 3] = (out | (inp << 3) & 0x7f) as u8;
        out = inp >> 4;
        inp = src[i + 4] as i32;
        dst[j + 4] = (out | (inp << 4) & 0x7f) as u8;
        out = inp >> 3;
        inp = src[i + 5] as i32;
        dst[j + 5] = (out | (inp << 5) & 0x7f) as u8;
        out = inp >> 2;
        inp = src[i + 6] as i32;
        dst[j + 6] = (out | (inp << 6) & 0x7f) as u8;
        out = inp >> 1;
        dst[j + 7] = out as u8;
        i += 7;
        j += 8;
    }
    if i < srclen {
        let mut inp = src[i] as i32;
        dst[j] = (inp & 0x7f) as u8;
        j += 1;
        let mut out = inp >> 7;
        if i + 1 < srclen {
            inp = src[i + 1] as i32;
            dst[j] = (out | (inp << 1) & 0x7f) as u8;
            j += 1;
            out = inp >> 6;
            if i + 2 < srclen {
                inp = src[i + 2] as i32;
                dst[j] = (out | (inp << 2) & 0x7f) as u8;
                j += 1;
                out = inp >> 5;
                if i + 3 < srclen {
                    inp = src[i + 3] as i32;
                    dst[j] = (out | (inp << 3) & 0x7f) as u8;
                    j += 1;
                    out = inp >> 4;
                    if i + 4 < srclen {
                        inp = src[i + 4] as i32;
                        dst[j] = (out | (inp << 4) & 0x7f) as u8;
                        j += 1;
                        out = inp >> 3;
                        if i + 5 < srclen {
                            inp = src[i + 5] as i32;
                            dst[j] = (out | (inp << 5) & 0x7f) as u8;
                            j += 1;
                            out = inp >> 2;
                        }
                    }
                }
            }
        }
        if j < dstlen {
            dst[j] = out as u8;
        }
    }
    dst
}

/// nsc `ScalaSigBytes.mapToNextModSevenBits`: 0x7f → 0, else +1.
/// Zero bytes are stored as modified UTF-8 `C0 80` in the classfile Utf8,
/// which `ByteCodecs.decode` / [`regenerate_zero`] map back to 0x7f.
pub fn avoid_zero(src: &[u8]) -> Vec<u8> {
    src.iter()
        .map(|&inp| if inp == 0x7f { 0 } else { inp.wrapping_add(1) })
        .collect()
}

pub fn encode_bytes(src: &[u8]) -> Vec<u8> {
    avoid_zero(&encode8to7(src))
}

/// Encode pickle bytes as the Java String stored in `ScalaSignature.bytes`
/// (latin-1 chars, later written as modified UTF-8 in the classfile).
pub fn encode_to_annotation_string(src: &[u8]) -> String {
    encode_bytes(src).into_iter().map(|b| char::from(b)).collect()
}

pub fn regenerate_zero(src: &mut [u8]) -> usize {
    let srclen = src.len();
    let mut i = 0;
    let mut j = 0;
    while i < srclen {
        let inp = src[i] as u32;
        if inp == 0xc0 && i + 1 < srclen && (src[i + 1] as u32) == 0x80 {
            src[j] = 0x7f;
            i += 2;
        } else if inp == 0 {
            src[j] = 0x7f;
            i += 1;
        } else {
            src[j] = (inp as u8).wrapping_sub(1);
            i += 1;
        }
        j += 1;
    }
    j
}

pub fn decode7to8(src: &mut [u8], srclen: usize) -> usize {
    let mut i = 0;
    let mut j = 0;
    // Inverse of encode8to7's `(srclen * 8 + 6) / 7`. The nsc formula
    // `(srclen * 7 + 7) / 8` rounds up and leaves a padding 0 that would
    // look like another pickle entry.
    let dstlen = (srclen * 7) / 8;
    while i + 7 < srclen {
        let mut out = src[i] as i32;
        let mut inp = src[i + 1] as i32;
        src[j] = (out | (inp & 0x01) << 7) as u8;
        out = inp >> 1;
        inp = src[i + 2] as i32;
        src[j + 1] = (out | (inp & 0x03) << 6) as u8;
        out = inp >> 2;
        inp = src[i + 3] as i32;
        src[j + 2] = (out | (inp & 0x07) << 5) as u8;
        out = inp >> 3;
        inp = src[i + 4] as i32;
        src[j + 3] = (out | (inp & 0x0f) << 4) as u8;
        out = inp >> 4;
        inp = src[i + 5] as i32;
        src[j + 4] = (out | (inp & 0x1f) << 3) as u8;
        out = inp >> 5;
        inp = src[i + 6] as i32;
        src[j + 5] = (out | (inp & 0x3f) << 2) as u8;
        out = inp >> 6;
        inp = src[i + 7] as i32;
        src[j + 6] = (out | inp << 1) as u8;
        i += 8;
        j += 7;
    }
    if i < srclen {
        let mut out = src[i] as i32;
        if i + 1 < srclen {
            let mut inp = src[i + 1] as i32;
            src[j] = (out | (inp & 0x01) << 7) as u8;
            j += 1;
            out = inp >> 1;
            if i + 2 < srclen {
                inp = src[i + 2] as i32;
                src[j] = (out | (inp & 0x03) << 6) as u8;
                j += 1;
                out = inp >> 2;
                if i + 3 < srclen {
                    inp = src[i + 3] as i32;
                    src[j] = (out | (inp & 0x07) << 5) as u8;
                    j += 1;
                    out = inp >> 3;
                    if i + 4 < srclen {
                        inp = src[i + 4] as i32;
                        src[j] = (out | (inp & 0x0f) << 4) as u8;
                        j += 1;
                        out = inp >> 4;
                        if i + 5 < srclen {
                            inp = src[i + 5] as i32;
                            src[j] = (out | (inp & 0x1f) << 3) as u8;
                            j += 1;
                            out = inp >> 5;
                            if i + 6 < srclen {
                                inp = src[i + 6] as i32;
                                src[j] = (out | (inp & 0x3f) << 2) as u8;
                                j += 1;
                            }
                        }
                    }
                }
            }
        }
        if j < dstlen {
            src[j] = out as u8;
        }
    }
    dstlen
}

/// Decode a `ScalaSignature.bytes` Java String (latin-1 chars) to pickle bytes.
pub fn decode_annotation_string(s: &str) -> Vec<u8> {
    let mut buf: Vec<u8> = s.chars().map(|c| c as u8).collect();
    let len = regenerate_zero(&mut buf);
    let n = decode7to8(&mut buf, len);
    buf.truncate(n.min(buf.len()));
    buf
}

// ---------------------------------------------------------------------------
// Pickle buffer
// ---------------------------------------------------------------------------

struct Buf {
    bytes: Vec<u8>,
}

impl Buf {
    fn new() -> Self {
        Buf { bytes: Vec::new() }
    }

    #[allow(dead_code)]
    fn write_byte(&mut self, b: u8) {
        self.bytes.push(b);
    }

    fn write_nat(&mut self, x: u32) {
        write_long_nat_to(&mut self.bytes, x as u64);
    }

    fn write_entry(&mut self, tag: u8, body: &[u8]) {
        // nsc: Entry = type_Nat length_Nat body
        self.write_nat(tag as u32);
        self.write_nat(body.len() as u32);
        self.bytes.extend_from_slice(body);
    }
}

struct Pickler<'a> {
    st: &'a SymbolTable,
    entries: Vec<(u8, Vec<u8>)>,
    term_names: std::collections::HashMap<String, u32>,
    type_names: std::collections::HashMap<String, u32>,
    ext_refs: std::collections::HashMap<String, u32>,
    none: u32,
    notpe: u32,
    noprefix: u32,
    scala_mod: Option<u32>,
    java_lang_mod: Option<u32>,
    empty_pkg: Option<u32>,
    sym_index: std::collections::HashMap<u32, u32>,
    this_tpes: std::collections::HashMap<u32, u32>,
    class_tparams: std::collections::HashMap<u32, Vec<u32>>,
}

impl<'a> Pickler<'a> {
    fn new(st: &'a SymbolTable) -> Self {
        let mut p = Pickler {
            st,
            entries: Vec::new(),
            term_names: std::collections::HashMap::new(),
            type_names: std::collections::HashMap::new(),
            ext_refs: std::collections::HashMap::new(),
            none: 0,
            notpe: 0,
            noprefix: 0,
            scala_mod: None,
            java_lang_mod: None,
            empty_pkg: None,
            sym_index: std::collections::HashMap::new(),
            this_tpes: std::collections::HashMap::new(),
            class_tparams: std::collections::HashMap::new(),
        };
        p.none = p.add(NONESYM, vec![]);
        p.notpe = p.add(NOTPE, vec![]);
        p.noprefix = p.add(NOPREFIXTPE, vec![]);
        p
    }

    fn add(&mut self, tag: u8, body: Vec<u8>) -> u32 {
        let i = self.entries.len() as u32;
        self.entries.push((tag, body));
        i
    }

    fn term_name(&mut self, name: &str) -> u32 {
        if let Some(i) = self.term_names.get(name) {
            return *i;
        }
        let i = self.add(TERMNAME, name.as_bytes().to_vec());
        self.term_names.insert(name.to_string(), i);
        i
    }

    fn type_name(&mut self, name: &str) -> u32 {
        if let Some(i) = self.type_names.get(name) {
            return *i;
        }
        let i = self.add(TYPENAME, name.as_bytes().to_vec());
        self.type_names.insert(name.to_string(), i);
        i
    }

    fn ext_mod(&mut self, name: &str, owner: Option<u32>) -> u32 {
        let key = match owner {
            Some(o) => format!("mod:{name}@{o}"),
            None => format!("mod:{name}"),
        };
        if let Some(i) = self.ext_refs.get(&key) {
            return *i;
        }
        // nsc EXTMODCLASSref uses the module's term name.
        let n = self.term_name(name);
        let mut body = Vec::new();
        write_nat_to(&mut body, n);
        if let Some(o) = owner {
            write_nat_to(&mut body, o);
        }
        let i = self.add(EXTMODCLASSREF, body);
        self.ext_refs.insert(key, i);
        i
    }

    fn ext_ref_owned(&mut self, name: &str, owner: u32) -> u32 {
        let key = format!("ext:{name}@{owner}");
        if let Some(i) = self.ext_refs.get(&key) {
            return *i;
        }
        let n = self.type_name(name);
        let mut body = Vec::new();
        write_nat_to(&mut body, n);
        write_nat_to(&mut body, owner);
        let i = self.add(EXTREF, body);
        self.ext_refs.insert(key, i);
        i
    }

    fn scala_module(&mut self) -> u32 {
        if let Some(i) = self.scala_mod {
            return i;
        }
        let i = self.ext_mod("scala", None);
        self.scala_mod = Some(i);
        i
    }

    fn java_lang_module(&mut self) -> u32 {
        if let Some(i) = self.java_lang_mod {
            return i;
        }
        let java = self.ext_mod("java", None);
        let i = self.ext_mod("lang", Some(java));
        self.java_lang_mod = Some(i);
        i
    }

    fn empty_package(&mut self) -> u32 {
        if let Some(i) = self.empty_pkg {
            return i;
        }
        let i = self.ext_mod("<empty>", None);
        self.empty_pkg = Some(i);
        i
    }

    /// nsc constructor result: `TypeRef(ThisType(<empty>), C, tparams)`.
    fn ctor_result_type(&mut self, class_idx: u32) -> u32 {
        let empty = self.empty_package();
        let mut th = Vec::new();
        write_nat_to(&mut th, empty);
        let pref = self.add(THISTPE, th);
        let tparams = self
            .class_tparams
            .get(&class_idx)
            .cloned()
            .unwrap_or_default();
        let mut targs = Vec::new();
        for tp in tparams {
            let mut tr = Vec::new();
            write_nat_to(&mut tr, self.noprefix);
            write_nat_to(&mut tr, tp);
            targs.push(self.add(TYPEREFTPE, tr));
        }
        let mut body = Vec::new();
        write_nat_to(&mut body, pref);
        write_nat_to(&mut body, class_idx);
        for t in targs {
            write_nat_to(&mut body, t);
        }
        self.add(TYPEREFTPE, body)
    }

    fn type_ref_in(&mut self, owner: u32, name: &str) -> u32 {
        let pref = self.noprefix;
        let sym = self.ext_ref_owned(name, owner);
        let mut body = Vec::new();
        write_nat_to(&mut body, pref);
        write_nat_to(&mut body, sym);
        self.add(TYPEREFTPE, body)
    }

    fn type_ref_local(&mut self, class_idx: u32, args: &[Type]) -> u32 {
        let mut body = Vec::new();
        write_nat_to(&mut body, self.noprefix);
        write_nat_to(&mut body, class_idx);
        for a in args {
            let t = self.pickle_type(a);
            write_nat_to(&mut body, t);
        }
        self.add(TYPEREFTPE, body)
    }

    fn type_ref_in_args(&mut self, owner: u32, name: &str, args: &[Type]) -> u32 {
        let pref = self.noprefix;
        let sym = self.ext_ref_owned(name, owner);
        let mut body = Vec::new();
        write_nat_to(&mut body, pref);
        write_nat_to(&mut body, sym);
        for a in args {
            let t = self.pickle_type(a);
            write_nat_to(&mut body, t);
        }
        self.add(TYPEREFTPE, body)
    }

    /// Default-package user class, as nsc `EXTREF` owned by `<empty>`.
    fn type_ref_user(&mut self, name: &str, args: &[Type]) -> u32 {
        let empty = self.empty_package();
        let pref = self.noprefix;
        let sym = self.ext_ref_owned(name, empty);
        let mut body = Vec::new();
        write_nat_to(&mut body, pref);
        write_nat_to(&mut body, sym);
        for a in args {
            let t = self.pickle_type(a);
            write_nat_to(&mut body, t);
        }
        self.add(TYPEREFTPE, body)
    }

    fn ext_ref(&mut self, name: &str) -> u32 {
        if let Some(i) = self.ext_refs.get(name) {
            return *i;
        }
        let n = self.type_name(name);
        let mut body = Vec::new();
        write_nat_to(&mut body, n);
        let i = self.add(EXTREF, body);
        self.ext_refs.insert(name.to_string(), i);
        i
    }

    fn type_ref_named(&mut self, name: &str) -> u32 {
        match name {
            "Int" | "Long" | "Float" | "Double" | "Boolean" | "Char" | "Unit" | "Any"
            | "AnyRef" | "AnyVal" | "Nothing" | "Null" | "Array" | "Seq" => {
                let sc = self.scala_module();
                self.type_ref_in(sc, name)
            }
            n if n.starts_with("Function") => {
                let sc = self.scala_module();
                self.type_ref_in(sc, n)
            }
            "String" | "Object" => {
                let jl = self.java_lang_module();
                self.type_ref_in(jl, name)
            }
            _ => {
                let pref = self.noprefix;
                let sym = self.ext_ref(name);
                let mut body = Vec::new();
                write_nat_to(&mut body, pref);
                write_nat_to(&mut body, sym);
                self.add(TYPEREFTPE, body)
            }
        }
    }

    fn pickle_type(&mut self, ty: &Type) -> u32 {
        match ty {
            Type::Unit | Type::NoType => self.type_ref_named("Unit"),
            Type::Boolean => self.type_ref_named("Boolean"),
            Type::Int => self.type_ref_named("Int"),
            Type::Long => self.type_ref_named("Long"),
            Type::Float => self.type_ref_named("Float"),
            Type::Double => self.type_ref_named("Double"),
            Type::Char => self.type_ref_named("Char"),
            Type::String => self.type_ref_named("String"),
            Type::Any => self.type_ref_named("Any"),
            Type::Wildcard | Type::AnyRef => self.type_ref_named("AnyRef"),
            Type::AnyVal => self.type_ref_named("AnyVal"),
            Type::TypeParam(id) => {
                // nsc pickles class/method tparams as TypeRef(NoPrefix, T), not ThisType.
                // ThisType(C).A is path-dependent and does not asSeenFrom-substitute.
                let pref = self.noprefix;
                let sym = self.pickle_typesym(*id);
                let mut body = Vec::new();
                write_nat_to(&mut body, pref);
                write_nat_to(&mut body, sym);
                self.add(TYPEREFTPE, body)
            }
            Type::Class { sym, args } => {
                if let Some(&idx) = self.sym_index.get(&sym.0) {
                    return self.type_ref_local(idx, args);
                }
                let n = self.st.get(*sym).name.trim_end_matches('$').to_string();
                match n.as_str() {
                    "Int" | "Long" | "Float" | "Double" | "Boolean" | "Char" | "Unit" | "Any"
                    | "AnyRef" | "AnyVal" | "Nothing" | "Null" | "Array" | "Seq" | "String"
                    | "Object" => self.type_ref_named(&n),
                    "Option" | "Some" | "None" => {
                        let sc = self.scala_module();
                        self.type_ref_in_args(sc, n.as_str(), args)
                    }
                    n if n.starts_with("Tuple") => {
                        let sc = self.scala_module();
                        self.type_ref_in_args(sc, n, args)
                    }
                    n if n.starts_with("Function") => self.type_ref_named(n),
                    n => self.type_ref_user(n, args),
                }
            }
            Type::ModuleRef(s) => {
                let n = self.st.get(*s).name.clone();
                let n = n.trim_end_matches('$').to_string();
                self.type_ref_named(&n)
            }
            Type::Function { params, .. } => {
                self.type_ref_named(&format!("Function{}", params.len()))
            }
            Type::Tuple(ts) => {
                let sc = self.scala_module();
                self.type_ref_in_args(sc, &format!("Tuple{}", ts.len()), ts)
            }
            Type::Array(_) => self.type_ref_named("Array"),
            Type::ByName(t) => self.pickle_type(t),
            Type::Repeated(_) => self.type_ref_named("Seq"),
            Type::Method { ret, .. } => self.pickle_type(ret),
            _ => self.type_ref_named("Any"),
        }
    }

    fn symbol_info(&mut self, name_ref: u32, owner_ref: u32, flags: u64, info_ref: u32) -> Vec<u8> {
        let mut body = Vec::new();
        write_nat_to(&mut body, name_ref);
        write_nat_to(&mut body, owner_ref);
        write_long_nat_to(&mut body, flags);
        write_nat_to(&mut body, info_ref);
        body
    }

    fn pickle_class(&mut self, class_id: SymbolId) -> u32 {
        if let Some(i) = self.sym_index.get(&class_id.0) {
            return *i;
        }
        let s = self.st.get(class_id);
        let is_module = matches!(s.kind, SymKind::Module | SymKind::ModuleClass)
            || s.flags.contains(Flags::MODULE)
            || s.name.ends_with('$');
        let is_case = s.flags.contains(Flags::CASE);
        let raw_name = s.name.trim_end_matches('$').to_string();
        // nsc module classes are CLASSsym + MODULE with a type name, not MODULEsym.
        let name_ref = self.type_name(&raw_name);
        let tag = CLASSSYM;
        // Placeholder; fill after children so the class exists as owner.
        let idx = self.add(tag, vec![]);
        self.sym_index.insert(class_id.0, idx);

        let this_tpe = {
            let mut body = Vec::new();
            write_nat_to(&mut body, idx);
            self.add(THISTPE, body)
        };
        self.this_tpes.insert(class_id.0, this_tpe);
        let jl = self.java_lang_module();
        let obj = self.ext_ref_owned("Object", jl);
        let obj_tpe = {
            let mut body = Vec::new();
            write_nat_to(&mut body, self.noprefix);
            write_nat_to(&mut body, obj);
            self.add(TYPEREFTPE, body)
        };
        let mut info_body = Vec::new();
        write_nat_to(&mut info_body, idx);
        write_nat_to(&mut info_body, obj_tpe);
        let info = self.add(CLASSINFOTPE, info_body);

        let members: Vec<SymbolId> = s.members.clone();
        let tparams: Vec<SymbolId> = s.tparams.clone();
        let ctor_fields: Vec<SymbolId> = s.ctor_fields.clone();
        let mut tparam_refs = Vec::new();
        for tp in tparams {
            tparam_refs.push(self.pickle_typesym(tp));
        }
        self.class_tparams.insert(idx, tparam_refs.clone());
        for m in members {
            let kind = self.st.get(m).kind;
            let name = self.st.get(m).name.clone();
            match kind {
                SymKind::TypeParam => {
                    let _ = self.pickle_typesym(m);
                }
                SymKind::Term => {
                    if ctor_fields.contains(&m) || !self.st.get(m).flags.contains(Flags::PARAM) {
                        if is_case && ctor_fields.contains(&m) {
                            // nsc `caseFieldAccessors` pairs CASEACCESSOR getters
                            // with non-method PARAMACCESSOR fields.
                            self.pickle_param_field(m, idx);
                        }
                        self.pickle_val(m, idx, is_case && ctor_fields.contains(&m));
                    }
                }
                SymKind::Method => {
                    if name == "<clinit>" {
                        continue;
                    }
                    if pickle_sig_incomplete(self.st, m) {
                        continue;
                    }
                    self.pickle_method(m, idx, this_tpe);
                }
                _ => {}
            }
        }

        let mut info = info;
        if !tparam_refs.is_empty() {
            let mut body = Vec::new();
            // nsc POLYtpe = restpe, {tparams}
            write_nat_to(&mut body, info);
            for r in tparam_refs {
                write_nat_to(&mut body, r);
            }
            info = self.add(POLYTPE, body);
        }

        let mut flags = 0u64;
        if is_module {
            flags |= raw_to_pickled(1 << 8); // MODULE
        }
        if is_case {
            flags |= raw_to_pickled(1 << 11); // CASE
        }
        let owner = self.empty_package();
        let body = self.symbol_info(name_ref, owner, flags, info);
        self.entries[idx as usize] = (tag, body);
        if is_module {
            // nsc unpickler binds the term `Lib` from MODULEsym; CLASSsym+MODULE is the module class.
            let mut tr = Vec::new();
            write_nat_to(&mut tr, self.noprefix);
            write_nat_to(&mut tr, idx);
            let mtpe = self.add(TYPEREFTPE, tr);
            let mflags = raw_to_pickled(1 << 8);
            let mn = self.term_name(&raw_name);
            let mbody = self.symbol_info(mn, owner, mflags, mtpe);
            self.add(MODULESYM, mbody);
        } else if let Some(mod_id) = self.st.companion_module(class_id) {
            // nsc `enterClassAndModule` completes term `Point` from MODULESYM
            // in `Point.class`, not from `Point$.class`.
            let mc = self.st.module_class_of(mod_id);
            if mc != class_id && !mc.is_none() {
                let _ = self.pickle_class(mc);
            }
        }
        idx
    }

    fn pickle_method(&mut self, method_id: SymbolId, owner_ref: u32, _this_tpe: u32) -> u32 {
        if let Some(i) = self.sym_index.get(&method_id.0) {
            return *i;
        }
        let s = self.st.get(method_id);
        let name_ref = self.term_name(&s.name);
        let (paramss, ret) = match &s.ty {
            Type::Method { paramss, ret } => (paramss.clone(), (**ret).clone()),
            _ => (vec![], Type::Unit),
        };
        let params: Vec<(String, Type, Flags)> = if !s.params.is_empty() {
            s.params
                .iter()
                .map(|p| {
                    let ps = self.st.get(*p);
                    (ps.name.clone(), ps.ty.clone(), ps.flags)
                })
                .collect()
        } else {
            paramss
                .iter()
                .flatten()
                .enumerate()
                .map(|(i, t)| (format!("x${i}"), t.clone(), Flags::PARAM))
                .collect()
        };

        let meth_idx = self.add(VALSYM, vec![]);
        self.sym_index.insert(method_id.0, meth_idx);

        let mut param_refs = Vec::new();
        for (pname, pty, pflags) in &params {
            let pn = self.term_name(pname);
            let pty_ref = self.pickle_type(pty);
            let mut flags = raw_to_pickled(1u64 << 13); // PARAM
            if pflags.contains(Flags::DEFAULTPARAM) {
                flags |= 1 << 25; // DEFAULTPARAM (not remapped)
            }
            if pflags.contains(Flags::IMPLICIT) {
                flags |= raw_to_pickled(1 << 9); // IMPLICIT
            }
            let body = self.symbol_info(pn, meth_idx, flags, pty_ref);
            param_refs.push(self.add(VALSYM, body));
        }
        let ret_ref = if s.name == "<init>" {
            self.ctor_result_type(owner_ref)
        } else {
            self.pickle_type(&ret)
        };
        let mut info = if params.is_empty() && s.name != "<init>" {
            // nsc NullaryMethodType = POLYtpe(restpe) with no tparams.
            let mut pt = Vec::new();
            write_nat_to(&mut pt, ret_ref);
            self.add(POLYTPE, pt)
        } else {
            let mut mt = Vec::new();
            write_nat_to(&mut mt, ret_ref);
            for p in param_refs {
                write_nat_to(&mut mt, p);
            }
            self.add(METHODTPE, mt)
        };
        let tparams: Vec<SymbolId> = s.tparams.clone();
        if !tparams.is_empty() {
            let mut tpref = Vec::new();
            // nsc POLYtpe = restpe, {tparams}
            write_nat_to(&mut tpref, info);
            for tp in tparams {
                write_nat_to(&mut tpref, self.pickle_typesym(tp));
            }
            info = self.add(POLYTPE, tpref);
        }
        let mut flags = raw_to_pickled(1u64 << 6); // METHOD
        if s.flags.contains(Flags::SYNTHETIC) || s.name.contains("$default$") {
            flags |= 1 << 21; // SYNTHETIC (not remapped)
        }
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[meth_idx as usize] = (VALSYM, body);
        meth_idx
    }

    fn pickle_typesym(&mut self, id: SymbolId) -> u32 {
        if let Some(i) = self.sym_index.get(&id.0) {
            return *i;
        }
        let s = self.st.get(id);
        let name_ref = self.type_name(&s.name);
        let idx = self.add(TYPESYM, vec![]);
        self.sym_index.insert(id.0, idx);
        let owner_ref = self
            .sym_index
            .get(&s.owner.0)
            .copied()
            .unwrap_or(self.none);
        let lo = self.type_ref_named("Nothing");
        let hi = self.type_ref_named("Any");
        let mut b = Vec::new();
        write_nat_to(&mut b, lo);
        write_nat_to(&mut b, hi);
        let bounds = self.add(TYPEBOUNDSTPE, b);
        let flags = raw_to_pickled((1u64 << 13) | (1u64 << 4)); // PARAM | DEFERRED
        let body = self.symbol_info(name_ref, owner_ref, flags, bounds);
        self.entries[idx as usize] = (TYPESYM, body);
        idx
    }

    /// nsc case-class ctor field (not the getter): PARAMACCESSOR, not METHOD.
    fn pickle_param_field(&mut self, val_id: SymbolId, owner_ref: u32) {
        let s = self.st.get(val_id);
        let name_ref = self.term_name(&s.name);
        let ty_ref = self.pickle_type(&s.ty);
        // PRIVATE | LOCAL stay outside bits 0–11; PARAMACCESSOR is not remapped.
        let mut flags = raw_to_pickled(1u64 << 2); // PRIVATE
        flags |= 1 << 19; // LOCAL
        flags |= 1 << 29; // PARAMACCESSOR
        let body = self.symbol_info(name_ref, owner_ref, flags, ty_ref);
        let _ = self.add(VALSYM, body);
    }

    fn pickle_val(&mut self, val_id: SymbolId, owner_ref: u32, case_accessor: bool) -> u32 {
        if let Some(i) = self.sym_index.get(&val_id.0) {
            return *i;
        }
        let s = self.st.get(val_id);
        let name_ref = self.term_name(&s.name);
        let idx = self.add(VALSYM, vec![]);
        self.sym_index.insert(val_id.0, idx);
        let ret_ref = self.pickle_type(&s.ty);
        // nsc NullaryMethodType is POLYtpe(restpe) with no tparams.
        let mut pt = Vec::new();
        write_nat_to(&mut pt, ret_ref);
        let info = self.add(POLYTPE, pt);
        // METHOD | STABLE | ACCESSOR, then nsc raw→pickled remap
        let mut flags = raw_to_pickled((1u64 << 6) | (1u64 << 22) | (1u64 << 27));
        if case_accessor {
            flags |= 1 << 24; // CASEACCESSOR (not remapped)
            flags |= 1 << 29; // PARAMACCESSOR (not remapped)
        }
        let body = self.symbol_info(name_ref, owner_ref, flags, info);
        self.entries[idx as usize] = (VALSYM, body);
        idx
    }

    fn finish(self) -> Vec<u8> {
        let mut buf = Buf::new();
        buf.write_nat(MAJOR);
        buf.write_nat(MINOR);
        buf.write_nat(self.entries.len() as u32);
        for (tag, body) in self.entries {
            buf.write_entry(tag, &body);
        }
        buf.bytes
    }
}

fn write_nat_to(out: &mut Vec<u8>, x: u32) {
    write_long_nat_to(out, x as u64);
}

/// nsc `PickleBuffer.writeLongNat`: big-endian base-128.
fn write_long_nat_to(out: &mut Vec<u8>, x: u64) {
    fn prefix(out: &mut Vec<u8>, x: u64) {
        let y = x >> 7;
        if y != 0 {
            prefix(out, y);
        }
        out.push(((x & 0x7f) | 0x80) as u8);
    }
    let y = x >> 7;
    if y != 0 {
        prefix(out, y);
    }
    out.push((x & 0x7f) as u8);
}

fn pickle_sig_incomplete(st: &SymbolTable, id: SymbolId) -> bool {
    fn ty_incomplete(t: &Type) -> bool {
        match t {
            Type::NoType | Type::Error => true,
            Type::Method { paramss, ret } => {
                ty_incomplete(ret) || paramss.iter().flatten().any(ty_incomplete)
            }
            Type::Function { params, ret } => {
                ty_incomplete(ret) || params.iter().any(ty_incomplete)
            }
            Type::Class { args, .. } => args.iter().any(ty_incomplete),
            Type::Tuple(ts) => ts.iter().any(ty_incomplete),
            Type::Array(t) | Type::ByName(t) | Type::Repeated(t) => ty_incomplete(t),
            _ => false,
        }
    }
    ty_incomplete(&st.get(id).ty)
}

/// nsc `Flags.rawToPickledFlags`: bits 0–11 differ between raw and pickled form.
fn raw_to_pickled(flags: u64) -> u64 {
    const PAIRS: [(u64, u64); 12] = [
        (1 << 6, 1 << 9),   // METHOD
        (1 << 2, 1 << 2),   // PRIVATE
        (1 << 5, 1 << 1),   // FINAL
        (1 << 0, 1 << 3),   // PROTECTED
        (1 << 11, 1 << 6),  // CASE
        (1 << 4, 1 << 8),   // DEFERRED
        (1 << 8, 1 << 10),  // MODULE
        (1 << 1, 1 << 5),   // OVERRIDE
        (1 << 7, 1 << 11),  // INTERFACE
        (1 << 9, 1 << 0),   // IMPLICIT
        (1 << 10, 1 << 4),  // SEALED
        (1 << 3, 1 << 7),   // ABSTRACT
    ];
    let from_set = PAIRS.iter().fold(0u64, |a, (from, _)| a | from);
    let mut result = flags & !from_set;
    let mut tobe = flags & from_set;
    for (from, to) in PAIRS {
        if tobe & from != 0 {
            result |= to;
            tobe &= !from;
        }
    }
    result
}

/// Pickle a class or module-class symbol into nsc-subset bytes.
pub fn pickle_class(st: &SymbolTable, class_id: SymbolId) -> Vec<u8> {
    if class_id.is_none() {
        return Vec::new();
    }
    let mut p = Pickler::new(st);
    p.pickle_class(class_id);
    p.finish()
}

/// Snapshot pickles before erasure. nsc pickles pre-erasure signatures so
/// `id[T]` / `Box[A]#get` stay type parameters, not `Object`.
pub fn pickle_all(st: &SymbolTable) -> std::collections::HashMap<u32, Vec<u8>> {
    let mut out = std::collections::HashMap::new();
    for s in &st.symbols {
        if matches!(
            s.kind,
            SymKind::Class | SymKind::ModuleClass
        ) && !s.id.is_none()
        {
            let raw = pickle_class(st, s.id);
            if !raw.is_empty() {
                out.insert(s.id.0, raw);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Unpickler
// ---------------------------------------------------------------------------

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    fn remaining(&self) -> bool {
        self.pos < self.bytes.len()
    }

    #[allow(dead_code)]
    fn skip(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n).min(self.bytes.len());
    }

    fn read_byte(&mut self) -> Option<u8> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        let b = self.bytes[self.pos];
        self.pos += 1;
        Some(b)
    }

    fn read_nat(&mut self) -> Option<u32> {
        Some(self.read_long_nat()? as u32)
    }

    fn read_long_nat(&mut self) -> Option<u64> {
        let mut x = 0u64;
        loop {
            let b = self.read_byte()? as u64;
            x = (x << 7) + (b & 0x7f);
            if (b & 0x80) == 0 {
                return Some(x);
            }
        }
    }
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
enum Entry {
    TermName(String),
    TypeName(String),
    NoneSym,
    TypeSym {
        name: u32,
        owner: u32,
        info: u32,
    },
    ClassSym {
        name: u32,
        owner: u32,
        info: u32,
        flags: u64,
    },
    ModuleSym {
        name: u32,
        owner: u32,
        info: u32,
    },
    ValSym {
        name: u32,
        owner: u32,
        info: u32,
        flags: u64,
    },
    ExtRef(u32),
    NoTpe,
    ThisTpe(u32),
    TypeRef {
        prefix: u32,
        sym: u32,
    },
    ClassInfo(u32),
    MethodTpe {
        ret: u32,
        params: Vec<u32>,
    },
    PolyTpe {
        tparams: Vec<u32>,
        rest: u32,
    },
    Other,
}

fn read_symbol_info(r: &mut Reader, end: usize) -> Option<(u32, u32, u64, u32)> {
    let name = r.read_nat()?;
    let owner = r.read_nat()?;
    let flags = r.read_long_nat()?;
    let info = r.read_nat()?;
    // Ignore a trailing privateWithin if a writer ever emits one.
    while r.pos < end {
        let _ = r.read_nat()?;
    }
    Some((name, owner, flags, info))
}

/// Unpickle our subset. Returns the first class/module plus its methods.
pub fn unpickle(bytes: &[u8]) -> Option<PickledClass> {
    if bytes.is_empty() {
        return None;
    }
    let mut r = Reader::new(bytes);
    let major = r.read_nat()?;
    let _minor = r.read_nat()?;
    if major != MAJOR {
        return None;
    }
    let nentries = r.read_nat()? as usize;
    if nentries > 100_000 {
        return None;
    }
    let mut entries: Vec<Entry> = Vec::with_capacity(nentries);
    for _ in 0..nentries {
        if !r.remaining() {
            return None;
        }
        let tag = r.read_byte()?;
        let len = r.read_nat()? as usize;
        let end = r.pos.saturating_add(len).min(r.bytes.len());
        let e = match tag {
            TERMNAME => {
                let s = String::from_utf8_lossy(&r.bytes[r.pos..end]).into_owned();
                r.pos = end;
                Entry::TermName(s)
            }
            TYPENAME => {
                let s = String::from_utf8_lossy(&r.bytes[r.pos..end]).into_owned();
                r.pos = end;
                Entry::TypeName(s)
            }
            NONESYM => {
                r.pos = end;
                Entry::NoneSym
            }
            TYPESYM => {
                let (name, owner, _flags, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::TypeSym { name, owner, info }
            }
            CLASSSYM => {
                let (name, owner, flags, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::ClassSym {
                    name,
                    owner,
                    info,
                    flags,
                }
            }
            MODULESYM => {
                let (name, owner, _flags, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::ModuleSym { name, owner, info }
            }
            VALSYM => {
                let (name, owner, flags, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::ValSym {
                    name,
                    owner,
                    info,
                    flags,
                }
            }
            EXTREF | EXTMODCLASSREF => {
                let n = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::ExtRef(n)
            }
            NOTPE | NOPREFIXTPE => {
                r.pos = end;
                Entry::NoTpe
            }
            THISTPE => {
                let s = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::ThisTpe(s)
            }
            TYPEREFTPE => {
                let prefix = r.read_nat().unwrap_or(0);
                let sym = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::TypeRef { prefix, sym }
            }
            CLASSINFOTPE => {
                let c = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::ClassInfo(c)
            }
            METHODTPE => {
                let ret = r.read_nat().unwrap_or(0);
                let mut params = Vec::new();
                while r.pos < end {
                    if let Some(p) = r.read_nat() {
                        params.push(p);
                    } else {
                        break;
                    }
                }
                r.pos = end;
                Entry::MethodTpe { ret, params }
            }
            POLYTPE => {
                let mut refs = Vec::new();
                while r.pos < end {
                    if let Some(p) = r.read_nat() {
                        refs.push(p);
                    } else {
                        break;
                    }
                }
                r.pos = end;
                // nsc: restpe first, then tparams
                let rest = refs.first().copied().unwrap_or(0);
                let tparams = if refs.len() > 1 {
                    refs[1..].to_vec()
                } else {
                    Vec::new()
                };
                Entry::PolyTpe { tparams, rest }
            }
            _ => {
                r.pos = end;
                Entry::Other
            }
        };
        entries.push(e);
    }

    fn name_of(entries: &[Entry], i: u32) -> String {
        match entries.get(i as usize) {
            Some(Entry::TermName(s) | Entry::TypeName(s)) => s.clone(),
            Some(Entry::ExtRef(n)) => name_of(entries, *n),
            Some(Entry::TypeSym { name, .. }) => name_of(entries, *name),
            _ => String::new(),
        }
    }

    fn type_name_of(entries: &[Entry], i: u32) -> String {
        match entries.get(i as usize) {
            Some(Entry::TypeRef { sym, .. }) => {
                let n = name_of(entries, *sym);
                if n.is_empty() {
                    type_name_of(entries, *sym)
                } else {
                    n
                }
            }
            Some(Entry::ExtRef(n)) => name_of(entries, *n),
            Some(Entry::TermName(s) | Entry::TypeName(s)) => s.clone(),
            Some(Entry::NoTpe) => "Any".into(),
            _ => "Any".into(),
        }
    }

    let mut class_idx = None;
    let mut is_module = false;
    const MODULE_PKL: u64 = 1 << 10;
    for (i, e) in entries.iter().enumerate() {
        match e {
            Entry::ClassSym { flags, .. } => {
                let mod_flag = (*flags & MODULE_PKL) != 0;
                if class_idx.is_none() {
                    class_idx = Some(i);
                    is_module = mod_flag;
                }
            }
            Entry::ModuleSym { .. } if class_idx.is_none() => {
                class_idx = Some(i);
                is_module = true;
            }
            _ => {}
        }
    }
    let ci = class_idx?;
    let class_name = match &entries[ci] {
        Entry::ModuleSym { name, .. } | Entry::ClassSym { name, .. } => name_of(&entries, *name),
        _ => return None,
    };
    if class_name.is_empty() {
        return None;
    }

    fn peel_info(entries: &[Entry], info: u32) -> (Vec<String>, u32) {
        match entries.get(info as usize) {
            Some(Entry::PolyTpe { tparams, rest }) => {
                let names = tparams.iter().map(|t| name_of(entries, *t)).collect();
                (names, *rest)
            }
            _ => (Vec::new(), info),
        }
    }

    let class_info = match &entries[ci] {
        Entry::ModuleSym { info, .. } | Entry::ClassSym { info, .. } => *info,
        _ => 0,
    };
    let (class_tparams, _) = peel_info(&entries, class_info);

    let mut methods = Vec::new();
    for e in &entries {
        let Entry::ValSym {
            name,
            owner,
            info,
            flags,
        } = e
        else {
            continue;
        };
        if *owner != ci as u32 {
            continue;
        }
        // Case-class ctor fields are PARAMACCESSOR without METHOD; skip them.
        const METHOD_PKL: u64 = 1 << 9;
        const PARAMACCESSOR: u64 = 1 << 29;
        if (*flags & METHOD_PKL) == 0 && (*flags & PARAMACCESSOR) != 0 {
            continue;
        }
        let mname = name_of(&entries, *name);
        if mname.is_empty() {
            continue;
        }
        let (tparams, rest) = peel_info(&entries, *info);
        if let Some(Entry::MethodTpe { ret, params }) = entries.get(rest as usize) {
            let mut param_names = Vec::new();
            let mut param_types = Vec::new();
            for p in params {
                if let Some(Entry::ValSym {
                    name: pn, info: pt, ..
                }) = entries.get(*p as usize)
                {
                    param_names.push(name_of(&entries, *pn));
                    param_types.push(type_name_of(&entries, *pt));
                } else {
                    param_types.push(type_name_of(&entries, *p));
                    param_names.push(format!("x${}", param_names.len()));
                }
            }
            let is_accessor = (*flags & (1u64 << 27)) != 0; // ACCESSOR
            methods.push(PickledMethod {
                    name: mname.clone(),
                    param_names,
                    param_types,
                    ret: type_name_of(&entries, *ret),
                    tparams,
                    is_val: is_accessor,
                    is_ctor: mname == "<init>",
                });
        } else {
            // NullaryMethodType (POLYtpe with no tparams) or a plain type.
            let is_accessor = (*flags & (1u64 << 27)) != 0;
            methods.push(PickledMethod {
                name: mname,
                param_names: Vec::new(),
                param_types: Vec::new(),
                ret: type_name_of(&entries, rest),
                tparams,
                is_val: is_accessor,
                is_ctor: false,
            });
        }
    }

    Some(PickledClass {
        name: class_name,
        is_module,
        tparams: class_tparams,
        methods,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pickle_tags(bytes: &[u8]) -> Vec<u8> {
        let mut r = Reader::new(bytes);
        let _ = r.read_nat();
        let _ = r.read_nat();
        let n = r.read_nat().unwrap_or(0) as usize;
        let mut tags = Vec::new();
        for _ in 0..n {
            let Some(tag) = r.read_byte() else {
                break;
            };
            let len = r.read_nat().unwrap_or(0) as usize;
            r.skip(len);
            tags.push(tag);
        }
        tags
    }

    #[test]
    fn bytecodecs_roundtrip() {
        let src: Vec<u8> = (0u8..=255).collect();
        let enc = encode_bytes(&src);
        let s: String = enc.iter().copied().map(char::from).collect();
        let dec = decode_annotation_string(&s);
        assert_eq!(dec, src, "SID-10 roundtrip");
    }

    #[test]
    fn pickle_unpickle_names() {
        let mut buf = Buf::new();
        buf.write_nat(5);
        buf.write_nat(2);
        buf.write_entry(NONESYM, &[]);
        buf.write_entry(TERMNAME, b"Lib");
        buf.write_entry(TYPENAME, b"String");
        let raw = buf.bytes;
        let enc = encode_to_annotation_string(&raw);
        let dec = decode_annotation_string(&enc);
        assert_eq!(dec, raw);
    }

    #[test]
    fn pickle_class_roundtrip_default_getter() {
        let src = r#"
object Lib {
  def greet(name: String, punct: String = "!"): String = name + punct
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let cls = st.module_class_of(lib);
        let raw = pickle_class(&st, cls);
        assert!(!raw.is_empty(), "expected pickle bytes");
        let enc = encode_to_annotation_string(&raw);
        let dec = decode_annotation_string(&enc);
        let p = unpickle(&dec).expect("unpickle subset ScalaSignature");
        assert!(p.is_module, "expected module class");
        let names: Vec<&str> = p.methods.iter().map(|m| m.name.as_str()).collect();
        assert!(
            names.iter().any(|n| *n == "greet"),
            "expected greet in pickle, got {names:?}"
        );
        assert!(
            names.iter().any(|n| *n == "greet$default$2"),
            "expected greet$default$2 in pickle, got {names:?}"
        );
    }

    #[test]
    fn pickle_vals_params_and_tparams() {
        let src = r#"
object Lib {
  val magic: Int = 7
  def greet(name: String, punct: String = "!"): String = name + punct
  def id[T](x: T): T = x
}
class Box[A](val value: A) {
  def get: A = value
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let cls = st.module_class_of(lib);
        let p = unpickle(&pickle_class(&st, cls)).expect("unpickle Lib");
        assert!(p.is_module);
        let magic = p
            .methods
            .iter()
            .find(|m| m.name == "magic")
            .expect("val magic");
        assert!(magic.is_val, "magic should be a val, got {magic:?}");
        assert_eq!(magic.ret, "Int");
        let greet = p
            .methods
            .iter()
            .find(|m| m.name == "greet")
            .expect("greet");
        assert_eq!(greet.param_names.len(), 2);
        assert_eq!(greet.param_types, vec!["String".to_string(), "String".to_string()]);
        let id = p.methods.iter().find(|m| m.name == "id").expect("id");
        assert_eq!(id.tparams, vec!["T".to_string()]);
        assert_eq!(id.param_types, vec!["T".to_string()]);
        assert_eq!(id.ret, "T");

        let box_id = st
            .symbols
            .iter()
            .find(|s| s.name == "Box" && s.kind == scala_rs_typer::SymKind::Class)
            .map(|s| s.id)
            .expect("Box class");
        let b = unpickle(&pickle_class(&st, box_id)).expect("unpickle Box");
        assert!(!b.is_module);
        assert_eq!(b.tparams, vec!["A".to_string()]);
        let value = b
            .methods
            .iter()
            .find(|m| m.name == "value")
            .expect("val value");
        assert!(value.is_val);
        assert_eq!(value.ret, "A");
        let get = b.methods.iter().find(|m| m.name == "get").expect("get");
        assert_eq!(get.ret, "A");
        let init = b
            .methods
            .iter()
            .find(|m| m.is_ctor)
            .expect("<init>");
        assert_eq!(init.param_types, vec!["A".to_string()]);
    }

    #[test]
    fn pickle_case_class_and_object_def() {
        let src = r#"
case class Point(x: Int, y: Int)
object Lib {
  def add(p: Point): Int = p.x + p.y
}
"#;
        let (_t, st, diags) = scala_rs_typer::typecheck_str(src);
        assert!(
            !scala_rs_typer::has_errors(&diags),
            "type errors: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        let point = st
            .symbols
            .iter()
            .find(|s| s.name == "Point" && s.kind == scala_rs_typer::SymKind::Class)
            .map(|s| s.id)
            .expect("Point class");
        let point_raw = pickle_class(&st, point);
        let pc = unpickle(&point_raw).expect("unpickle Point");
        assert!(
            !pc.is_module,
            "Point.class pickle must stay a class, not the companion module"
        );
        assert!(
            pickle_tags(&point_raw).contains(&MODULESYM),
            "Point.class pickle must include MODULESYM so nsc can bind term Point"
        );
        let x = pc.methods.iter().find(|m| m.name == "x").expect("val x");
        assert!(x.is_val);
        assert_eq!(x.ret, "Int");
        let init = pc
            .methods
            .iter()
            .find(|m| m.is_ctor)
            .expect("<init>");
        assert_eq!(init.param_types, vec!["Int".to_string(), "Int".to_string()]);

        let lib = st
            .symbols
            .iter()
            .find(|s| s.name == "Lib" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Lib module");
        let l = unpickle(&pickle_class(&st, st.module_class_of(lib))).expect("unpickle Lib");
        let add = l.methods.iter().find(|m| m.name == "add").expect("add");
        assert_eq!(add.param_types, vec!["Point".to_string()]);
        assert_eq!(add.ret, "Int");

        let pmod = st
            .symbols
            .iter()
            .find(|s| s.name == "Point" && s.kind == scala_rs_typer::SymKind::Module)
            .map(|s| s.id)
            .expect("Point module");
        let pm = unpickle(&pickle_class(&st, st.module_class_of(pmod))).expect("unpickle Point$");
        assert!(pm.is_module);
        let apply = pm.methods.iter().find(|m| m.name == "apply").expect("apply");
        assert_eq!(apply.param_types, vec!["Int".to_string(), "Int".to_string()]);
        assert_eq!(apply.ret, "Point");
        let unapply = pm
            .methods
            .iter()
            .find(|m| m.name == "unapply")
            .expect("unapply");
        assert_eq!(unapply.param_types, vec!["Point".to_string()]);
        assert_eq!(unapply.ret, "Option");
    }
}
