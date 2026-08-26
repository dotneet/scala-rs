//! JVM class file writer (major version 50 / Java 6, no StackMapTable).

use std::collections::HashMap;
use std::io::{self, Write};

pub const ACC_PUBLIC: u16 = 0x0001;
pub const ACC_PRIVATE: u16 = 0x0002;
pub const ACC_STATIC: u16 = 0x0008;
pub const ACC_FINAL: u16 = 0x0010;
pub const ACC_SUPER: u16 = 0x0020;
pub const ACC_INTERFACE: u16 = 0x0200;
pub const ACC_ABSTRACT: u16 = 0x0400;
pub const ACC_SYNTHETIC: u16 = 0x1000;

/// Scala NameTransformer encoding so operator methods are legal JVM names.
/// `<init>` / `<clinit>` are left alone. `->` becomes `$minus$greater`.
pub fn encode_method_name(name: &str) -> String {
    if name == "<init>" || name == "<clinit>" {
        return name.to_string();
    }
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return name.to_string();
    }
    let mut out = String::new();
    for c in name.chars() {
        match c {
            '~' => out.push_str("$tilde"),
            '=' => out.push_str("$eq"),
            '<' => out.push_str("$less"),
            '>' => out.push_str("$greater"),
            '!' => out.push_str("$bang"),
            '#' => out.push_str("$hash"),
            '%' => out.push_str("$percent"),
            '^' => out.push_str("$up"),
            '&' => out.push_str("$amp"),
            '|' => out.push_str("$bar"),
            '*' => out.push_str("$times"),
            '/' => out.push_str("$div"),
            '+' => out.push_str("$plus"),
            '-' => out.push_str("$minus"),
            ':' => out.push_str("$colon"),
            '?' => out.push_str("$qmark"),
            '@' => out.push_str("$at"),
            _ => out.push(c),
        }
    }
    out
}

/// Inverse of [`encode_method_name`] for names recovered from classfiles.
pub fn decode_method_name(name: &str) -> String {
    if !name.contains('$') {
        return name.to_string();
    }
    let mut out = String::new();
    let mut rest = name;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("$tilde") {
            out.push('~');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$eq") {
            out.push('=');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$less") {
            out.push('<');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$greater") {
            out.push('>');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$bang") {
            out.push('!');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$hash") {
            out.push('#');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$percent") {
            out.push('%');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$up") {
            out.push('^');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$amp") {
            out.push('&');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$bar") {
            out.push('|');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$times") {
            out.push('*');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$div") {
            out.push('/');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$plus") {
            out.push('+');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$minus") {
            out.push('-');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$colon") {
            out.push(':');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$qmark") {
            out.push('?');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$at") {
            out.push('@');
            rest = r;
        } else {
            let ch = rest.chars().next().unwrap();
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

pub struct EmittedClass {
    /// e.g. `"Main"`, `"Main$"`, `"scala/Option"`
    pub internal_name: String,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
pub struct Pool {
    bytes: Vec<u8>,
    count: u16, // number of entries + 1 (next index)
    utf8: HashMap<String, u16>,
    class: HashMap<String, u16>,
    string: HashMap<String, u16>,
    nat: HashMap<(u16, u16), u16>,
    refs: HashMap<(u8, u16, u16), u16>,
    ints: HashMap<i32, u16>,
    floats: HashMap<u32, u16>,
    longs: HashMap<i64, u16>,
    doubles: HashMap<u64, u16>,
}

impl Pool {
    pub fn new() -> Self {
        Pool {
            count: 1,
            ..Default::default()
        }
    }

    pub fn utf8(&mut self, s: &str) -> u16 {
        if let Some(i) = self.utf8.get(s) {
            return *i;
        }
        let i = self.count;
        self.count += 1;
        self.bytes.push(1); // CONSTANT_Utf8
        let b = s.as_bytes();
        self.bytes
            .extend_from_slice(&(b.len() as u16).to_be_bytes());
        self.bytes.extend_from_slice(b);
        self.utf8.insert(s.to_string(), i);
        i
    }

    pub fn class(&mut self, internal: &str) -> u16 {
        if let Some(i) = self.class.get(internal) {
            return *i;
        }
        let u = self.utf8(internal);
        let i = self.count;
        self.count += 1;
        self.bytes.push(7);
        self.bytes.extend_from_slice(&u.to_be_bytes());
        self.class.insert(internal.to_string(), i);
        i
    }

    pub fn string(&mut self, s: &str) -> u16 {
        if let Some(i) = self.string.get(s) {
            return *i;
        }
        let u = self.utf8(s);
        let i = self.count;
        self.count += 1;
        self.bytes.push(8);
        self.bytes.extend_from_slice(&u.to_be_bytes());
        self.string.insert(s.to_string(), i);
        i
    }

    pub fn integer(&mut self, v: i32) -> u16 {
        if let Some(i) = self.ints.get(&v) {
            return *i;
        }
        let i = self.count;
        self.count += 1;
        self.bytes.push(3);
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self.ints.insert(v, i);
        i
    }

    /// CONSTANT_Float (tag 4) occupies one pool slot.
    pub fn float(&mut self, v: f32) -> u16 {
        let bits = v.to_bits();
        if let Some(i) = self.floats.get(&bits) {
            return *i;
        }
        let i = self.count;
        self.count += 1;
        self.bytes.push(4);
        self.bytes.extend_from_slice(&bits.to_be_bytes());
        self.floats.insert(bits, i);
        i
    }

    /// CONSTANT_Long occupies two pool slots (JVMS §4.4.5).
    pub fn long(&mut self, v: i64) -> u16 {
        if let Some(i) = self.longs.get(&v) {
            return *i;
        }
        let i = self.count;
        self.count = self.count.saturating_add(2);
        self.bytes.push(5);
        self.bytes.extend_from_slice(&v.to_be_bytes());
        self.longs.insert(v, i);
        i
    }

    /// CONSTANT_Double occupies two pool slots (JVMS §4.4.5).
    pub fn double(&mut self, v: f64) -> u16 {
        let bits = v.to_bits();
        if let Some(i) = self.doubles.get(&bits) {
            return *i;
        }
        let i = self.count;
        self.count = self.count.saturating_add(2);
        self.bytes.push(6);
        self.bytes.extend_from_slice(&bits.to_be_bytes());
        self.doubles.insert(bits, i);
        i
    }

    fn nat(&mut self, name: &str, desc: &str) -> u16 {
        let n = self.utf8(name);
        let d = self.utf8(desc);
        if let Some(i) = self.nat.get(&(n, d)) {
            return *i;
        }
        let i = self.count;
        self.count += 1;
        self.bytes.push(12);
        self.bytes.extend_from_slice(&n.to_be_bytes());
        self.bytes.extend_from_slice(&d.to_be_bytes());
        self.nat.insert((n, d), i);
        i
    }

    pub fn fieldref(&mut self, owner: &str, name: &str, desc: &str) -> u16 {
        self.member_ref(9, owner, name, desc)
    }

    pub fn methodref(&mut self, owner: &str, name: &str, desc: &str) -> u16 {
        self.member_ref(10, owner, name, desc)
    }

    pub fn iface_ref(&mut self, owner: &str, name: &str, desc: &str) -> u16 {
        self.member_ref(11, owner, name, desc)
    }

    fn member_ref(&mut self, tag: u8, owner: &str, name: &str, desc: &str) -> u16 {
        let c = self.class(owner);
        let n = self.nat(name, desc);
        if let Some(i) = self.refs.get(&(tag, c, n)) {
            return *i;
        }
        let i = self.count;
        self.count += 1;
        self.bytes.push(tag);
        self.bytes.extend_from_slice(&c.to_be_bytes());
        self.bytes.extend_from_slice(&n.to_be_bytes());
        self.refs.insert((tag, c, n), i);
        i
    }

    pub fn write_header(&self, w: &mut Vec<u8>) {
        w.extend_from_slice(&self.count.to_be_bytes());
        w.extend_from_slice(&self.bytes);
    }
}

pub struct Field {
    pub access: u16,
    pub name: String,
    pub desc: String,
}

pub struct Method {
    pub access: u16,
    pub name: String,
    pub desc: String,
    pub code: Option<Code>,
}

#[derive(Clone, Debug)]
pub struct ExceptionEntry {
    pub start_pc: u16,
    pub end_pc: u16,
    pub handler_pc: u16,
    /// Constant-pool Class index, or `0` for catch-all / finally.
    pub catch_type: u16,
}

#[derive(Clone, Debug)]
pub struct Code {
    pub max_stack: u16,
    pub max_locals: u16,
    pub bytes: Vec<u8>,
    pub exceptions: Vec<ExceptionEntry>,
}

pub struct ClassEmit {
    pub access: u16,
    pub this_name: String,
    pub super_name: String,
    pub interfaces: Vec<String>,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    pub source: String,
    /// `ScalaSignature.bytes` as a Java String (latin-1 chars), if any.
    pub scala_signature: Option<String>,
}

impl ClassEmit {
    pub fn write_with_pool(&self, mut pool: Pool) -> io::Result<Vec<u8>> {
        let this_i = pool.class(&self.this_name);
        let super_i = pool.class(&self.super_name);
        let ifaces: Vec<u16> = self.interfaces.iter().map(|i| pool.class(i)).collect();
        let mut field_idxs = Vec::new();
        for f in &self.fields {
            field_idxs.push((f.access, pool.utf8(&f.name), pool.utf8(&f.desc)));
        }
        let code_attr = pool.utf8("Code");
        let src_attr = pool.utf8("SourceFile");
        let src_name = pool.utf8(&self.source);
        let rva_attr = if self.scala_signature.is_some() {
            Some(pool.utf8("RuntimeVisibleAnnotations"))
        } else {
            None
        };
        let sig_type = if self.scala_signature.is_some() {
            Some(pool.utf8("Lscala/reflect/ScalaSignature;"))
        } else {
            None
        };
        let bytes_name = if self.scala_signature.is_some() {
            Some(pool.utf8("bytes"))
        } else {
            None
        };
        let sig_utf8 = self
            .scala_signature
            .as_deref()
            .map(|s| pool.utf8(s));
        let mut methods_data = Vec::new();
        for m in &self.methods {
            let n = pool.utf8(&m.name);
            let d = pool.utf8(&m.desc);
            methods_data.push((m.access, n, d, m.code.clone()));
        }
        let mut out = Vec::new();
        out.extend_from_slice(&0xCAFEBABEu32.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&50u16.to_be_bytes());
        pool.write_header(&mut out);
        out.extend_from_slice(&self.access.to_be_bytes());
        out.extend_from_slice(&this_i.to_be_bytes());
        out.extend_from_slice(&super_i.to_be_bytes());
        out.extend_from_slice(&(ifaces.len() as u16).to_be_bytes());
        for i in ifaces {
            out.extend_from_slice(&i.to_be_bytes());
        }
        out.extend_from_slice(&(field_idxs.len() as u16).to_be_bytes());
        for (acc, n, d) in field_idxs {
            out.extend_from_slice(&acc.to_be_bytes());
            out.extend_from_slice(&n.to_be_bytes());
            out.extend_from_slice(&d.to_be_bytes());
            out.extend_from_slice(&0u16.to_be_bytes());
        }
        out.extend_from_slice(&(methods_data.len() as u16).to_be_bytes());
        for (acc, n, d, code) in methods_data {
            out.extend_from_slice(&acc.to_be_bytes());
            out.extend_from_slice(&n.to_be_bytes());
            out.extend_from_slice(&d.to_be_bytes());
            if let Some(c) = code {
                out.extend_from_slice(&1u16.to_be_bytes());
                out.extend_from_slice(&code_attr.to_be_bytes());
                let mut body = Vec::new();
                body.extend_from_slice(&c.max_stack.to_be_bytes());
                body.extend_from_slice(&c.max_locals.to_be_bytes());
                body.extend_from_slice(&(c.bytes.len() as u32).to_be_bytes());
                body.extend_from_slice(&c.bytes);
                body.extend_from_slice(&(c.exceptions.len() as u16).to_be_bytes());
                for e in &c.exceptions {
                    body.extend_from_slice(&e.start_pc.to_be_bytes());
                    body.extend_from_slice(&e.end_pc.to_be_bytes());
                    body.extend_from_slice(&e.handler_pc.to_be_bytes());
                    body.extend_from_slice(&e.catch_type.to_be_bytes());
                }
                body.extend_from_slice(&0u16.to_be_bytes());
                out.extend_from_slice(&(body.len() as u32).to_be_bytes());
                out.extend_from_slice(&body);
            } else {
                out.extend_from_slice(&0u16.to_be_bytes());
            }
        }
        let n_class_attrs = 1u16 + if rva_attr.is_some() { 1 } else { 0 };
        out.extend_from_slice(&n_class_attrs.to_be_bytes());
        if let (Some(rva), Some(sig_ty), Some(bn), Some(su)) =
            (rva_attr, sig_type, bytes_name, sig_utf8)
        {
            // RuntimeVisibleAnnotations { num=1, ScalaSignature { bytes = Utf8 } }
            let mut body = Vec::new();
            body.extend_from_slice(&1u16.to_be_bytes());
            body.extend_from_slice(&sig_ty.to_be_bytes());
            body.extend_from_slice(&1u16.to_be_bytes());
            body.extend_from_slice(&bn.to_be_bytes());
            body.push(b's');
            body.extend_from_slice(&su.to_be_bytes());
            out.extend_from_slice(&rva.to_be_bytes());
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(&body);
        }
        out.extend_from_slice(&src_attr.to_be_bytes());
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&src_name.to_be_bytes());
        Ok(out)
    }
}

pub fn write_class_file(path: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)
}
