//! JVM class file writer (major version 52 / Java 8, with StackMapTable).

use rustc_hash::FxHashMap as HashMap;
use std::io::{self, Write};

pub use scala_rs_pickle::names::{decode_method_name, encode_method_name};

pub const ACC_PUBLIC: u16 = 0x0001;
pub const ACC_PRIVATE: u16 = 0x0002;
pub const ACC_PROTECTED: u16 = 0x0004;
pub const ACC_STATIC: u16 = 0x0008;
pub const ACC_FINAL: u16 = 0x0010;
pub const ACC_SUPER: u16 = 0x0020;
pub const ACC_INTERFACE: u16 = 0x0200;
pub const ACC_NATIVE: u16 = 0x0100;
pub const ACC_ABSTRACT: u16 = 0x0400;
pub const ACC_BRIDGE: u16 = 0x0040;
/// Field flag (same bit as [`ACC_BRIDGE`] on methods).
pub const ACC_VOLATILE: u16 = 0x0040;
pub const ACC_TRANSIENT: u16 = 0x0080;
/// The same bit as `ACC_TRANSIENT`, but on a *method*: its last parameter is
/// a Java varargs array (`@scala.annotation.varargs`).
pub const ACC_VARARGS: u16 = 0x0080;
pub const ACC_SYNTHETIC: u16 = 0x1000;

pub struct EmittedClass {
    /// e.g. `"Main"`, `"Main$"`, `"scala/Option"`
    pub internal_name: String,
    pub bytes: Vec<u8>,
    /// Limits of the class file format this class does not fit in, one message
    /// per offending member (`"Method too large: Main.f ()V"`).
    ///
    /// These are not *our* bugs to route around: no encoding of the method
    /// exists, and nsc reports the same thing. `bytes` is filled in anyway --
    /// the driver reports each message and does not write the file, so what is
    /// in it never reaches a class loader.
    pub format_errors: Vec<String>,
}

/// One entry of the JVMS §4.7.6 `InnerClasses` attribute.
#[derive(Clone, Debug)]
pub struct InnerClassEntry {
    /// Internal name of the nested class this entry describes.
    pub inner_class: String,
    /// Internal name of the class it is a *member* of. `None` for a local or
    /// anonymous class (JVMS: `outer_class_info_index` is zero).
    pub outer_class: Option<String>,
    /// Source simple name. `None` for an anonymous class (JVMS:
    /// `inner_name_index` is zero); local classes still carry one.
    pub inner_name: Option<String>,
    /// Source-level modifiers (`public`/`private`/`protected`, `static` for
    /// "has no enclosing instance", `final`) — distinct from the nested
    /// class's own classfile `access_flags`.
    pub access_flags: u16,
}

/// JVMS §4.4.7: a `CONSTANT_Utf8_info` carries a `u2` byte count.
const MAX_UTF8_CONST: usize = 65535;

/// JVMS §4.7.3: `code_length` is a `u4`, but "must be less than 65536".
/// A longer method is not encodable, and a class loader rejects the file
/// while parsing it ("Invalid method Code length").
pub const MAX_CODE_LENGTH: usize = 65535;

/// Modified-UTF-8 width of one char. `\0` is two bytes, not one -- and the
/// SID-10 encoding does produce `\0` (it is what `avoidZero` turns `0x7f`
/// into), so counting chars instead of bytes would still overflow.
fn modified_utf8_width(c: char) -> usize {
    match c as u32 {
        0 => 2,
        u if u < 0x80 => 1,
        u if u < 0x800 => 2,
        _ => 3,
    }
}

/// Split `s` at char boundaries into pieces that each fit one constant.
///
/// The reader concatenates the pieces back into one string before decoding,
/// so where the split falls does not matter as long as no char is cut in
/// half.
fn utf8_chunks(s: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut len = 0usize;
    for ch in s.chars() {
        let w = modified_utf8_width(ch);
        if len + w > max && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            len = 0;
        }
        cur.push(ch);
        len += w;
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

/// JVMS §4.4.7 modified UTF-8 (U+0000 as `C0 80`).
fn modified_utf8_bytes(s: &str) -> Vec<u8> {
    let mut b = Vec::with_capacity(s.len());
    for c in s.chars() {
        let u = c as u32;
        if u == 0 {
            b.push(0xc0);
            b.push(0x80);
        } else if u < 0x80 {
            b.push(u as u8);
        } else if u < 0x800 {
            b.push((0xc0 | (u >> 6)) as u8);
            b.push((0x80 | (u & 0x3f)) as u8);
        } else {
            b.push((0xe0 | (u >> 12)) as u8);
            b.push((0x80 | ((u >> 6) & 0x3f)) as u8);
            b.push((0x80 | (u & 0x3f)) as u8);
        }
    }
    b
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
    /// `CONSTANT_MethodType_info` (JVMS §4.4.9), keyed by descriptor.
    method_types: HashMap<String, u16>,
    /// `CONSTANT_MethodHandle_info` (JVMS §4.4.8), keyed by
    /// `(reference_kind, reference_index)`.
    method_handles: HashMap<(u8, u16), u16>,
    /// `CONSTANT_InvokeDynamic_info` (JVMS §4.4.10), keyed by
    /// `(bootstrap_method_attr_index, name_and_type_index)`.
    invoke_dynamics: HashMap<(u16, u16), u16>,
    /// `BootstrapMethods` (JVMS §4.7.23) entries, in attribute order:
    /// `(method handle index, static argument indices)`.
    bootstraps: Vec<(u16, Vec<u16>)>,
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
        let encoded = modified_utf8_bytes(s);
        // `encoded.len() as u16` used to wrap silently, and the class file
        // that came out had a constant pool no reader could walk ("unexpected
        // tag at #104"). Only the `ScalaSignature` ever got near the limit,
        // and that one is split across `ScalaLongSignature` before it reaches
        // here; anything else arriving oversized is a bug worth stopping for
        // rather than writing an unloadable class file.
        assert!(
            encoded.len() <= MAX_UTF8_CONST,
            "CONSTANT_Utf8 of {} bytes exceeds the JVMS limit of {MAX_UTF8_CONST}",
            encoded.len()
        );
        let i = self.count;
        self.count += 1;
        self.bytes.push(1); // CONSTANT_Utf8
        self.bytes
            .extend_from_slice(&(encoded.len() as u16).to_be_bytes());
        self.bytes.extend_from_slice(&encoded);
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

    /// Internal names of every `CONSTANT_Class` already interned in this pool
    /// (from actual bytecode: `new`/`checkcast`/`instanceof`, method and
    /// field descriptors, the superclass and interface list, …). Used to
    /// compute the `InnerClasses` attribute: JVMS §4.7.6 requires an entry
    /// for every member class that appears anywhere in the constant pool.
    pub fn interned_class_names(&self) -> Vec<String> {
        self.class.keys().cloned().collect()
    }

    /// `CONSTANT_MethodType_info` (JVMS §4.4.9) for a method descriptor.
    pub fn method_type(&mut self, desc: &str) -> u16 {
        if let Some(i) = self.method_types.get(desc) {
            return *i;
        }
        let d = self.utf8(desc);
        let i = self.count;
        self.count += 1;
        self.bytes.push(16);
        self.bytes.extend_from_slice(&d.to_be_bytes());
        self.method_types.insert(desc.to_string(), i);
        i
    }

    /// `CONSTANT_MethodHandle_info` (JVMS §4.4.8) for a static method:
    /// `reference_kind` 6 (`REF_invokeStatic`), pointing at a `Methodref` or
    /// (for a static method declared in an interface) an `InterfaceMethodref`.
    pub fn method_handle_static(
        &mut self,
        owner: &str,
        name: &str,
        desc: &str,
        iface: bool,
    ) -> u16 {
        let r = if iface {
            self.iface_ref(owner, name, desc)
        } else {
            self.methodref(owner, name, desc)
        };
        self.method_handle(6, r)
    }

    /// `CONSTANT_MethodHandle_info` (JVMS §4.4.8) with an explicit kind.
    pub fn method_handle(&mut self, kind: u8, reference: u16) -> u16 {
        if let Some(i) = self.method_handles.get(&(kind, reference)) {
            return *i;
        }
        let i = self.count;
        self.count += 1;
        self.bytes.push(15);
        self.bytes.push(kind);
        self.bytes.extend_from_slice(&reference.to_be_bytes());
        self.method_handles.insert((kind, reference), i);
        i
    }

    /// Append (or reuse) a `BootstrapMethods` entry and return its
    /// *attribute* index — the number that a `CONSTANT_InvokeDynamic_info`
    /// stores in `bootstrap_method_attr_index`.
    pub fn bootstrap(&mut self, handle: u16, args: Vec<u16>) -> u16 {
        if let Some(i) = self
            .bootstraps
            .iter()
            .position(|(h, a)| *h == handle && *a == args)
        {
            return i as u16;
        }
        self.bootstraps.push((handle, args));
        (self.bootstraps.len() - 1) as u16
    }

    /// `CONSTANT_InvokeDynamic_info` (JVMS §4.4.10).
    pub fn invoke_dynamic(&mut self, bootstrap: u16, name: &str, desc: &str) -> u16 {
        let nt = self.nat(name, desc);
        if let Some(i) = self.invoke_dynamics.get(&(bootstrap, nt)) {
            return *i;
        }
        let i = self.count;
        self.count += 1;
        self.bytes.push(18);
        self.bytes.extend_from_slice(&bootstrap.to_be_bytes());
        self.bytes.extend_from_slice(&nt.to_be_bytes());
        self.invoke_dynamics.insert((bootstrap, nt), i);
        i
    }

    fn has_bootstraps(&self) -> bool {
        !self.bootstraps.is_empty()
    }

    /// The `BootstrapMethods` attribute body (JVMS §4.7.23), without the
    /// attribute name/length header.
    fn bootstrap_body(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&(self.bootstraps.len() as u16).to_be_bytes());
        for (handle, args) in &self.bootstraps {
            b.extend_from_slice(&handle.to_be_bytes());
            b.extend_from_slice(&(args.len() as u16).to_be_bytes());
            for a in args {
                b.extend_from_slice(&a.to_be_bytes());
            }
        }
        b
    }

    /// Public wrapper for [`Pool::nat`], needed to build an `EnclosingMethod`
    /// attribute's `method_index` from outside this module.
    pub fn name_and_type(&mut self, name: &str, desc: &str) -> u16 {
        self.nat(name, desc)
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
    /// RuntimeVisible Java annotations (`Ljava/lang/Deprecated;`, …).
    pub java_annots: Vec<String>,
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
    pub stack_map: Option<Vec<u8>>,
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
    /// nsc `Scala` attribute: pickle lives on the companion / mirror class.
    pub scala_raw: bool,
    /// JVMS §4.7.6 `InnerClasses` entries. Empty means the attribute is
    /// omitted entirely (a class that neither is, nor references, any
    /// nested class).
    pub inner_classes: Vec<InnerClassEntry>,
    /// JVMS §4.7.7 `EnclosingMethod`, for a local or anonymous class: the
    /// binary name of the innermost enclosing class, and — if it is
    /// enclosed by a method/constructor rather than by a field initializer —
    /// that method's name and descriptor.
    pub enclosing_method: Option<(String, Option<(String, String)>)>,
}

impl ClassEmit {
    pub fn write_with_pool(&self, mut pool: Pool) -> io::Result<Vec<u8>> {
        let this_i = pool.class(&self.this_name);
        let super_i = pool.class(&self.super_name);
        let ifaces: Vec<u16> = self.interfaces.iter().map(|i| pool.class(i)).collect();
        let mut field_idxs = Vec::new();
        for f in &self.fields {
            // A field name is an *unqualified name* (JVMS 4.2.2): `.`, `;`,
            // `[` and `/` are illegal in one. slick's `Library.scala` writes
            // `val / = new SqlOperator("/")`, and emitting the character raw
            // made `slick/ast/Library$` unloadable ("Illegal field name").
            // nsc runs every term name through the same NameTransformer it
            // uses for methods, so `/` is `$div`.
            field_idxs.push((
                f.access,
                pool.utf8(&encode_method_name(&f.name)),
                pool.utf8(&f.desc),
            ));
        }
        let code_attr = pool.utf8("Code");
        let stack_map_attr = pool.utf8("StackMapTable");
        let src_attr = pool.utf8("SourceFile");
        let src_name = pool.utf8(&self.source);
        let scala_raw_attr = if self.scala_raw {
            Some(pool.utf8("Scala"))
        } else {
            None
        };
        let scala_sig_attr = if self.scala_signature.is_some() && !self.scala_raw {
            Some(pool.utf8("ScalaSig"))
        } else {
            None
        };
        let rva_attr = if self.scala_signature.is_some() {
            Some(pool.utf8("RuntimeVisibleAnnotations"))
        } else {
            None
        };
        // A `CONSTANT_Utf8` holds at most 65535 bytes (JVMS §4.4.7) and the
        // length field is a `u2`, so an oversized one used to wrap and leave
        // an unreadable constant pool behind -- `slick/util/TupleMethods`
        // came out as "unexpected tag at #104" once its nested classes went
        // into its signature. nsc's answer is SID-10's `ScalaLongSignature`:
        // the same encoded string, split into an array of pieces that each
        // fit, concatenated again by the reader.
        let sig_chunks: Vec<String> = self
            .scala_signature
            .as_deref()
            .map(|s| utf8_chunks(s, MAX_UTF8_CONST))
            .unwrap_or_default();
        let long_sig = sig_chunks.len() > 1;
        let sig_type = self.scala_signature.is_some().then(|| {
            pool.utf8(if long_sig {
                "Lscala/reflect/ScalaLongSignature;"
            } else {
                "Lscala/reflect/ScalaSignature;"
            })
        });
        let bytes_name = if self.scala_signature.is_some() {
            Some(pool.utf8("bytes"))
        } else {
            None
        };
        let sig_utf8s: Vec<u16> = sig_chunks.iter().map(|c| pool.utf8(c)).collect();
        let inner_classes_attr = if self.inner_classes.is_empty() {
            None
        } else {
            Some(pool.utf8("InnerClasses"))
        };
        let inner_classes_idxs: Vec<(u16, u16, u16, u16)> = self
            .inner_classes
            .iter()
            .map(|e| {
                let inner = pool.class(&e.inner_class);
                let outer = e.outer_class.as_deref().map_or(0, |o| pool.class(o));
                let name = e.inner_name.as_deref().map_or(0, |n| pool.utf8(n));
                (inner, outer, name, e.access_flags)
            })
            .collect();
        let enclosing_method_attr = if self.enclosing_method.is_some() {
            Some(pool.utf8("EnclosingMethod"))
        } else {
            None
        };
        let enclosing_method_idxs = self.enclosing_method.as_ref().map(|(cls, m)| {
            let c = pool.class(cls);
            let nt = m.as_ref().map_or(0, |(n, d)| pool.name_and_type(n, d));
            (c, nt)
        });
        // JVMS §4.7.23: a class whose constant pool holds a
        // `CONSTANT_InvokeDynamic_info` must carry exactly one
        // `BootstrapMethods` attribute. Intern its name before the pool is
        // written out; the entries themselves are already pool indices.
        let bootstrap_attr = if pool.has_bootstraps() {
            Some(pool.utf8("BootstrapMethods"))
        } else {
            None
        };
        let method_rva = if self.methods.iter().any(|m| !m.java_annots.is_empty()) {
            Some(pool.utf8("RuntimeVisibleAnnotations"))
        } else {
            None
        };
        let mut methods_data = Vec::new();
        for m in &self.methods {
            let n = pool.utf8(&m.name);
            let d = pool.utf8(&m.desc);
            let annots: Vec<u16> = m.java_annots.iter().map(|a| pool.utf8(a)).collect();
            methods_data.push((m.access, n, d, m.code.clone(), annots));
        }
        let mut out = Vec::new();
        out.extend_from_slice(&0xCAFEBABEu32.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&52u16.to_be_bytes());
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
        for (acc, n, d, code, annots) in methods_data {
            out.extend_from_slice(&acc.to_be_bytes());
            out.extend_from_slice(&n.to_be_bytes());
            out.extend_from_slice(&d.to_be_bytes());
            let n_attrs = u16::from(code.is_some()) + u16::from(!annots.is_empty());
            out.extend_from_slice(&n_attrs.to_be_bytes());
            if let Some(c) = code {
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
                let n_code_attrs = if c.stack_map.is_some() { 1u16 } else { 0 };
                body.extend_from_slice(&n_code_attrs.to_be_bytes());
                if let Some(sm) = &c.stack_map {
                    body.extend_from_slice(&stack_map_attr.to_be_bytes());
                    body.extend_from_slice(&(sm.len() as u32).to_be_bytes());
                    body.extend_from_slice(sm);
                }
                out.extend_from_slice(&(body.len() as u32).to_be_bytes());
                out.extend_from_slice(&body);
            }
            if !annots.is_empty() {
                let rva = method_rva
                    .or(rva_attr)
                    .expect("RuntimeVisibleAnnotations utf8");
                let mut body = Vec::new();
                body.extend_from_slice(&(annots.len() as u16).to_be_bytes());
                for ty in annots {
                    body.extend_from_slice(&ty.to_be_bytes());
                    body.extend_from_slice(&0u16.to_be_bytes());
                }
                out.extend_from_slice(&rva.to_be_bytes());
                out.extend_from_slice(&(body.len() as u32).to_be_bytes());
                out.extend_from_slice(&body);
            }
        }
        let n_class_attrs = 1u16
            + if rva_attr.is_some() { 1 } else { 0 }
            + if scala_sig_attr.is_some() { 1 } else { 0 }
            + if scala_raw_attr.is_some() { 1 } else { 0 }
            + if inner_classes_attr.is_some() { 1 } else { 0 }
            + if bootstrap_attr.is_some() { 1 } else { 0 }
            + if enclosing_method_attr.is_some() {
                1
            } else {
                0
            };
        out.extend_from_slice(&n_class_attrs.to_be_bytes());
        if let Some(ic) = inner_classes_attr {
            out.extend_from_slice(&ic.to_be_bytes());
            out.extend_from_slice(&((2 + inner_classes_idxs.len() * 8) as u32).to_be_bytes());
            out.extend_from_slice(&(inner_classes_idxs.len() as u16).to_be_bytes());
            for (inner, outer, name, flags) in &inner_classes_idxs {
                out.extend_from_slice(&inner.to_be_bytes());
                out.extend_from_slice(&outer.to_be_bytes());
                out.extend_from_slice(&name.to_be_bytes());
                out.extend_from_slice(&flags.to_be_bytes());
            }
        }
        if let Some(bm) = bootstrap_attr {
            let body = pool.bootstrap_body();
            out.extend_from_slice(&bm.to_be_bytes());
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(&body);
        }
        if let (Some(em), Some((c, nt))) = (enclosing_method_attr, enclosing_method_idxs) {
            out.extend_from_slice(&em.to_be_bytes());
            out.extend_from_slice(&4u32.to_be_bytes());
            out.extend_from_slice(&c.to_be_bytes());
            out.extend_from_slice(&nt.to_be_bytes());
        }
        if let Some(raw) = scala_raw_attr {
            // nsc pickleMarkerForeign: pickle is on the companion / mirror.
            out.extend_from_slice(&raw.to_be_bytes());
            out.extend_from_slice(&0u32.to_be_bytes());
        }
        if let Some(ss) = scala_sig_attr {
            // nsc pickleMarkerLocal: `ScalaSig` attribute with version pickle
            // (major, minor, nentries=0). Tells nsc this class carries a pickle.
            let marker = [5u8, 2, 0];
            out.extend_from_slice(&ss.to_be_bytes());
            out.extend_from_slice(&(marker.len() as u32).to_be_bytes());
            out.extend_from_slice(&marker);
        }
        if let (Some(rva), Some(sig_ty), Some(bn)) = (rva_attr, sig_type, bytes_name) {
            // RuntimeVisibleAnnotations { num=1, ScalaSignature { bytes = Utf8 } },
            // or `ScalaLongSignature { bytes = { Utf8, ... } }` when one
            // constant could not hold the whole pickle.
            let mut body = Vec::new();
            body.extend_from_slice(&1u16.to_be_bytes());
            body.extend_from_slice(&sig_ty.to_be_bytes());
            body.extend_from_slice(&1u16.to_be_bytes());
            body.extend_from_slice(&bn.to_be_bytes());
            if long_sig {
                body.push(b'[');
                body.extend_from_slice(&(sig_utf8s.len() as u16).to_be_bytes());
                for su in &sig_utf8s {
                    body.push(b's');
                    body.extend_from_slice(&su.to_be_bytes());
                }
            } else {
                for su in &sig_utf8s {
                    body.push(b's');
                    body.extend_from_slice(&su.to_be_bytes());
                }
            }
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
