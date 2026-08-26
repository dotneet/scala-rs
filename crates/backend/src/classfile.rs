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
        self.bytes.extend_from_slice(&(b.len() as u16).to_be_bytes());
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
        out.extend_from_slice(&1u16.to_be_bytes());
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
