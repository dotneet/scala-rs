//! nsc PickleFormat subset (major 5, minor 2) plus SID-10 ByteCodecs.
//!
//! This is enough for scala-rs to round-trip compiled classes/objects through
//! `ScalaSignature` and for `javap -v` to show the annotation. It is **not**
//! a full nsc pickle (no POLY types, existentials, annotation args, or the
//! complete Flags long).

use scala_rs_parser::{Flags, SymbolId, Type};
use scala_rs_typer::{SymKind, SymbolTable};

pub const MAJOR: u32 = 5;
pub const MINOR: u32 = 2;

pub const TERMNAME: u8 = 1;
pub const TYPENAME: u8 = 2;
pub const NONESYM: u8 = 3;
pub const CLASSSYM: u8 = 6;
pub const MODULESYM: u8 = 7;
pub const VALSYM: u8 = 8;
pub const EXTREF: u8 = 9;
pub const EXTMODCLASSREF: u8 = 10;
pub const NOTPE: u8 = 11;
pub const THISTPE: u8 = 13;
pub const TYPEREFTPE: u8 = 16;
pub const CLASSINFOTPE: u8 = 19;
pub const METHODTPE: u8 = 20;

/// Pickled method recovered by the subset unpickler.
#[derive(Clone, Debug)]
pub struct PickledMethod {
    pub name: String,
    pub param_names: Vec<String>,
    pub param_types: Vec<String>,
    pub ret: String,
}

/// Pickled class or module class.
#[derive(Clone, Debug)]
pub struct PickledClass {
    pub name: String,
    pub is_module: bool,
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

pub fn avoid_zero(src: &[u8]) -> Vec<u8> {
    let extra = src.iter().filter(|&&b| b == 0x7f).count();
    let mut dst = Vec::with_capacity(src.len() + extra);
    for &inp in src {
        if inp == 0x7f {
            dst.push(0xc0);
            dst.push(0x80);
        } else {
            dst.push(inp.wrapping_add(1));
        }
    }
    dst
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
    let dstlen = (srclen * 7 + 7) / 8;
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

    fn write_byte(&mut self, b: u8) {
        self.bytes.push(b);
    }

    fn write_nat(&mut self, mut x: u32) {
        while (x & !0x7f) != 0 {
            self.write_byte(((x & 0x7f) | 0x80) as u8);
            x >>= 7;
        }
        self.write_byte((x & 0x7f) as u8);
    }

    #[allow(dead_code)]
    fn write_long_nat(&mut self, mut x: u64) {
        while (x & !0x7f) != 0 {
            self.write_byte(((x & 0x7f) | 0x80) as u8);
            x >>= 7;
        }
        self.write_byte((x & 0x7f) as u8);
    }

    fn write_entry(&mut self, tag: u8, body: &[u8]) {
        self.write_nat((body.len() + 1) as u32);
        self.write_byte(tag);
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
    sym_index: std::collections::HashMap<u32, u32>,
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
            sym_index: std::collections::HashMap::new(),
        };
        p.none = p.add(NONESYM, vec![]);
        p.notpe = p.add(NOTPE, vec![]);
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
        let pref = self.notpe;
        let sym = self.ext_ref(name);
        let mut body = Vec::new();
        write_nat_to(&mut body, pref);
        write_nat_to(&mut body, sym);
        self.add(TYPEREFTPE, body)
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
            Type::Any | Type::TypeParam(_) | Type::Wildcard | Type::AnyRef | Type::AnyVal => {
                self.type_ref_named("Object")
            }
            Type::Class { sym, .. } => {
                let n = self.st.get(*sym).name.clone();
                let n = n.trim_end_matches('$').to_string();
                self.type_ref_named(&n)
            }
            Type::ModuleRef(s) => {
                let n = self.st.get(*s).name.clone();
                let n = n.trim_end_matches('$').to_string();
                self.type_ref_named(&n)
            }
            Type::Function { params, .. } => {
                self.type_ref_named(&format!("Function{}", params.len()))
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
        let raw_name = s.name.trim_end_matches('$').to_string();
        let name_ref = if is_module {
            self.term_name(&raw_name)
        } else {
            self.type_name(&raw_name)
        };
        let tag = if is_module { MODULESYM } else { CLASSSYM };
        // Placeholder; fill after children so the class exists as owner.
        let idx = self.add(tag, vec![]);
        self.sym_index.insert(class_id.0, idx);

        let this_tpe = {
            let mut body = Vec::new();
            write_nat_to(&mut body, idx);
            self.add(THISTPE, body)
        };
        let obj = self.ext_ref("Object");
        let obj_tpe = {
            let mut body = Vec::new();
            write_nat_to(&mut body, self.notpe);
            write_nat_to(&mut body, obj);
            self.add(TYPEREFTPE, body)
        };
        let mut info_body = Vec::new();
        write_nat_to(&mut info_body, idx);
        write_nat_to(&mut info_body, obj_tpe);
        let info = self.add(CLASSINFOTPE, info_body);

        let members: Vec<SymbolId> = s.members.clone();
        for m in members {
            let ms = self.st.get(m);
            if ms.kind != SymKind::Method {
                continue;
            }
            if ms.name == "<init>" || ms.name == "<clinit>" {
                continue;
            }
            self.pickle_method(m, idx, this_tpe);
        }

        let body = self.symbol_info(name_ref, self.none, 0, info);
        self.entries[idx as usize] = (tag, body);
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
            let mut flags = 0u64;
            if pflags.contains(Flags::PARAM) {
                flags |= 1 << 13; // nsc PARAM-ish; subset only
            }
            if pflags.contains(Flags::DEFAULTPARAM) {
                flags |= 1 << 21;
            }
            if pflags.contains(Flags::IMPLICIT) {
                flags |= 1 << 5;
            }
            let body = self.symbol_info(pn, meth_idx, flags, pty_ref);
            param_refs.push(self.add(VALSYM, body));
        }
        let ret_ref = self.pickle_type(&ret);
        let mut mt = Vec::new();
        write_nat_to(&mut mt, ret_ref);
        for p in param_refs {
            write_nat_to(&mut mt, p);
        }
        let info = self.add(METHODTPE, mt);
        let body = self.symbol_info(name_ref, owner_ref, 0, info);
        self.entries[meth_idx as usize] = (VALSYM, body);
        meth_idx
    }

    fn finish(self) -> Vec<u8> {
        let mut buf = Buf::new();
        buf.write_nat(MAJOR);
        buf.write_nat(MINOR);
        for (tag, body) in self.entries {
            buf.write_entry(tag, &body);
        }
        buf.bytes
    }
}

fn write_nat_to(out: &mut Vec<u8>, mut x: u32) {
    while (x & !0x7f) != 0 {
        out.push(((x & 0x7f) | 0x80) as u8);
        x >>= 7;
    }
    out.push((x & 0x7f) as u8);
}

fn write_long_nat_to(out: &mut Vec<u8>, mut x: u64) {
    while (x & !0x7f) != 0 {
        out.push(((x & 0x7f) | 0x80) as u8);
        x >>= 7;
    }
    out.push((x & 0x7f) as u8);
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

    fn read_byte(&mut self) -> Option<u8> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        let b = self.bytes[self.pos];
        self.pos += 1;
        Some(b)
    }

    fn read_nat(&mut self) -> Option<u32> {
        let mut x = 0u32;
        let mut shift = 0;
        loop {
            let b = self.read_byte()? as u32;
            x |= (b & 0x7f) << shift;
            if (b & 0x80) == 0 {
                return Some(x);
            }
            shift += 7;
            if shift > 28 {
                return None;
            }
        }
    }

    fn read_long_nat(&mut self) -> Option<u64> {
        let mut x = 0u64;
        let mut shift = 0;
        loop {
            let b = self.read_byte()? as u64;
            x |= (b & 0x7f) << shift;
            if (b & 0x80) == 0 {
                return Some(x);
            }
            shift += 7;
            if shift > 63 {
                return None;
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
    ClassSym {
        name: u32,
        owner: u32,
        info: u32,
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
    Other,
}

fn read_symbol_info(r: &mut Reader, end: usize) -> Option<(u32, u32, u32)> {
    let name = r.read_nat()?;
    let owner = r.read_nat()?;
    let _flags = r.read_long_nat()?;
    let info = r.read_nat()?;
    // Ignore a trailing privateWithin if a writer ever emits one.
    while r.pos < end {
        let _ = r.read_nat()?;
    }
    Some((name, owner, info))
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
    let mut entries: Vec<Entry> = Vec::new();
    while r.remaining() {
        let len = r.read_nat()? as usize;
        let start = r.pos;
        let end = start.saturating_add(len).min(r.bytes.len());
        let tag = r.read_byte()?;
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
            CLASSSYM => {
                let (name, owner, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::ClassSym { name, owner, info }
            }
            MODULESYM => {
                let (name, owner, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::ModuleSym { name, owner, info }
            }
            VALSYM => {
                let (name, owner, info) = read_symbol_info(&mut r, end)?;
                r.pos = end;
                Entry::ValSym { name, owner, info }
            }
            EXTREF | EXTMODCLASSREF => {
                let n = r.read_nat().unwrap_or(0);
                r.pos = end;
                Entry::ExtRef(n)
            }
            NOTPE => {
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
    for (i, e) in entries.iter().enumerate() {
        match e {
            Entry::ModuleSym { .. } => {
                class_idx = Some(i);
                is_module = true;
                break;
            }
            Entry::ClassSym { .. } if class_idx.is_none() => {
                class_idx = Some(i);
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

    let mut methods = Vec::new();
    for e in &entries {
        let Entry::ValSym { name, owner, info } = e else {
            continue;
        };
        if *owner != ci as u32 {
            continue;
        }
        let mname = name_of(&entries, *name);
        if mname.is_empty() {
            continue;
        }
        let Some(Entry::MethodTpe { ret, params }) = entries.get(*info as usize) else {
            continue;
        };
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
        methods.push(PickledMethod {
            name: mname,
            param_names,
            param_types,
            ret: type_name_of(&entries, *ret),
        });
    }

    Some(PickledClass {
        name: class_name,
        is_module,
        methods,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
            .lookup("Lib")
            .into_iter()
            .find(|&s| st.get(s).kind == scala_rs_typer::SymKind::Module)
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
}
