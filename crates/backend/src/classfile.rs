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
/// Method-parameter flag (JVMS §4.7.24).
pub const ACC_MANDATED: u16 = 0x8000;
/// `strictfp` (`@scala.annotation.strictfp`); meaningful up to class file
/// version 60, and this writer emits 52.
pub const ACC_STRICT: u16 = 0x0800;

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
    /// `bootstraps`' attribute index of each entry. A class with thousands
    /// of lambdas looked every new one up by comparing it with all before.
    bootstrap_index: HashMap<(u16, Vec<u16>), u16>,
    /// Method handles of the bodies of this class's *serializable* lambdas,
    /// in first-use order: the argument list of the class's
    /// `$deserializeLambda$` (see `ClassBuilder::finish_inner`).
    pub serializable_lambdas: Vec<u16>,
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
        let key = (handle, args);
        if let Some(&i) = self.bootstrap_index.get(&key) {
            return i;
        }
        let i = self.bootstraps.len() as u16;
        self.bootstrap_index.insert(key.clone(), i);
        self.bootstraps.push(key);
        i
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
    /// Java annotations on the method (`@Deprecated`, `@GET`, …).
    pub java_annots: Vec<Annotation>,
    /// Java annotations on each parameter, in source order; empty when no
    /// parameter has one.
    pub param_annots: Vec<Vec<Annotation>>,
    /// JVMS §4.7.9 `Signature`: the generic shape `desc` erased away. Set
    /// through [`crate::gen::ClassBuilder::sign_last`], which refuses any
    /// string that does not erase back to `desc`.
    pub signature: Option<String>,
    /// JVMS §4.7.24 `MethodParameters`, in descriptor order. `None` keeps a
    /// parameter slot unnamed (for example the synthetic enclosing-instance
    /// argument of a nested class). Reflection libraries such as json4s use
    /// these names when selecting a constructor for a case class.
    pub param_names: Vec<Option<String>>,
    /// JVMS `MethodParameters` access flags, parallel to `param_names`.
    pub param_flags: Vec<u16>,
    /// Parameter names for the `LocalVariableTable` of a method without
    /// `MethodParameters`, in descriptor order.
    pub local_names: Vec<Option<String>>,
}

/// A Java annotation as JVMS §4.7.16 stores it.
#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    /// The annotation interface's descriptor (`Ljakarta/ws/rs/GET;`).
    pub desc: String,
    /// `RuntimeVisibleAnnotations` rather than `RuntimeInvisibleAnnotations`.
    pub visible: bool,
    pub elems: Vec<(String, ElementValue)>,
}

impl Annotation {
    /// A runtime-visible annotation without elements.
    pub fn marker(desc: &str) -> Self {
        Annotation {
            desc: desc.to_string(),
            visible: true,
            elems: Vec::new(),
        }
    }
}

/// JVMS §4.7.16.1 `element_value`.
#[derive(Clone, Debug, PartialEq)]
pub enum ElementValue {
    /// `B C I S Z`, with the tag.
    Int(u8, i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Str(String),
    Enum {
        desc: String,
        name: String,
    },
    /// A class literal, by its return descriptor (`Ljava/lang/String;`, `V`).
    Class(String),
    Annot(Annotation),
    Array(Vec<ElementValue>),
}

/// Java annotations of a class and its fields, which live beside the
/// [`Field`] literals rather than in them.
#[derive(Clone, Debug, Default)]
pub struct ClassAnnots {
    pub class: Vec<Annotation>,
    /// Keyed by the field's source name, as `field_signatures` is.
    pub fields: HashMap<String, Vec<Annotation>>,
}

fn encode_annotation(pool: &mut Pool, a: &Annotation, out: &mut Vec<u8>) {
    out.extend_from_slice(&pool.utf8(&a.desc).to_be_bytes());
    out.extend_from_slice(&(a.elems.len() as u16).to_be_bytes());
    for (name, value) in &a.elems {
        out.extend_from_slice(&pool.utf8(name).to_be_bytes());
        encode_element_value(pool, value, out);
    }
}

fn encode_element_value(pool: &mut Pool, v: &ElementValue, out: &mut Vec<u8>) {
    match v {
        ElementValue::Int(tag, n) => {
            out.push(*tag);
            out.extend_from_slice(&pool.integer(*n).to_be_bytes());
        }
        ElementValue::Long(n) => {
            out.push(b'J');
            out.extend_from_slice(&pool.long(*n).to_be_bytes());
        }
        ElementValue::Float(f) => {
            out.push(b'F');
            out.extend_from_slice(&pool.float(*f).to_be_bytes());
        }
        ElementValue::Double(d) => {
            out.push(b'D');
            out.extend_from_slice(&pool.double(*d).to_be_bytes());
        }
        ElementValue::Str(s) => {
            out.push(b's');
            out.extend_from_slice(&pool.utf8(s).to_be_bytes());
        }
        ElementValue::Enum { desc, name } => {
            out.push(b'e');
            out.extend_from_slice(&pool.utf8(desc).to_be_bytes());
            out.extend_from_slice(&pool.utf8(name).to_be_bytes());
        }
        ElementValue::Class(desc) => {
            out.push(b'c');
            out.extend_from_slice(&pool.utf8(desc).to_be_bytes());
        }
        ElementValue::Annot(a) => {
            out.push(b'@');
            encode_annotation(pool, a, out);
        }
        ElementValue::Array(items) => {
            out.push(b'[');
            out.extend_from_slice(&(items.len() as u16).to_be_bytes());
            for item in items {
                encode_element_value(pool, item, out);
            }
        }
    }
}

/// `Runtime{Visible,Invisible}Annotations` for `annots`, as (attribute name,
/// body) pairs; `extra_visible` is appended to the visible attribute (nsc
/// writes a class's `ScalaSignature` after its Java annotations).
fn annotation_attrs(
    pool: &mut Pool,
    annots: &[Annotation],
    extra_visible: Option<(u16, Vec<u8>)>,
) -> Vec<(u16, Vec<u8>)> {
    let mut attrs = Vec::new();
    for visible in [true, false] {
        let mut chosen: Vec<&Annotation> = annots.iter().filter(|a| a.visible == visible).collect();
        chosen.dedup_by(|a, b| a.desc == b.desc);
        let extra = if visible {
            extra_visible.as_ref()
        } else {
            None
        };
        let count = chosen.len() + extra.map_or(0, |(n, _)| *n as usize);
        if count == 0 {
            continue;
        }
        let name = pool.utf8(if visible {
            "RuntimeVisibleAnnotations"
        } else {
            "RuntimeInvisibleAnnotations"
        });
        let mut body = (count as u16).to_be_bytes().to_vec();
        for a in chosen {
            encode_annotation(pool, a, &mut body);
        }
        if let Some((_, bytes)) = extra {
            body.extend_from_slice(bytes);
        }
        attrs.push((name, body));
    }
    attrs
}

/// `Runtime{Visible,Invisible}ParameterAnnotations` (JVMS §4.7.18).
fn param_annotation_attrs(pool: &mut Pool, params: &[Vec<Annotation>]) -> Vec<(u16, Vec<u8>)> {
    let mut attrs = Vec::new();
    for visible in [true, false] {
        if !params.iter().flatten().any(|a| a.visible == visible) {
            continue;
        }
        let name = pool.utf8(if visible {
            "RuntimeVisibleParameterAnnotations"
        } else {
            "RuntimeInvisibleParameterAnnotations"
        });
        let mut body = vec![params.len() as u8];
        for p in params {
            let chosen: Vec<&Annotation> = p.iter().filter(|a| a.visible == visible).collect();
            body.extend_from_slice(&(chosen.len() as u16).to_be_bytes());
            for a in chosen {
                encode_annotation(pool, a, &mut body);
            }
        }
        attrs.push((name, body));
    }
    attrs
}

fn write_attrs(out: &mut Vec<u8>, attrs: &[(u16, Vec<u8>)]) {
    for (name, body) in attrs {
        out.extend_from_slice(&name.to_be_bytes());
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(body);
    }
}

/// The parameter descriptors of method descriptor `desc`.
fn param_descriptors(desc: &str) -> Vec<String> {
    let Some(end) = desc.find(')') else {
        return Vec::new();
    };
    let bytes = desc.as_bytes();
    let mut i = usize::from(bytes.first() == Some(&b'('));
    let mut out = Vec::new();
    while i < end {
        let start = i;
        while i < end && bytes[i] == b'[' {
            i += 1;
        }
        if i < end && bytes[i] == b'L' {
            while i < end && bytes[i] != b';' {
                i += 1;
            }
        }
        i += 1;
        out.push(desc[start..i.min(end)].to_string());
    }
    out
}

fn method_parameter_count(desc: &str) -> usize {
    let Some(end) = desc.find(')') else { return 0 };
    let bytes = desc.as_bytes();
    let mut i = usize::from(bytes.first() == Some(&b'('));
    let mut count = 0;
    while i < end {
        count += 1;
        while i < end && bytes[i] == b'[' {
            i += 1;
        }
        if i < end && bytes[i] == b'L' {
            while i < end && bytes[i] != b';' {
                i += 1;
            }
        }
        i += 1;
    }
    count
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
    /// JVMS §4.7.9 `Signature` on the class itself: formal type parameters,
    /// then the superclass and every interface in `interfaces` order.
    pub signature: Option<String>,
    /// JVMS §4.7.9 `Signature` for a field, keyed by the field's source name
    /// (`fields[i].name`, before `encode_method_name`). A map rather than a
    /// member of [`Field`] because field names are unique within a class file
    /// and twenty-odd call sites build `Field` literals.
    pub field_signatures: std::collections::HashMap<String, String>,
    /// JVMS §4.7.2 `ConstantValue` for a `static final` field, keyed the same
    /// way. Only `long` is produced (`@SerialVersionUID`).
    pub field_constants: std::collections::HashMap<String, i64>,
    pub annots: ClassAnnots,
}

impl ClassEmit {
    pub fn write_with_pool(&self, mut pool: Pool) -> io::Result<Vec<u8>> {
        let this_i = pool.class(&self.this_name);
        let super_i = pool.class(&self.super_name);
        let ifaces: Vec<u16> = self.interfaces.iter().map(|i| pool.class(i)).collect();
        // JVMS §4.7.9 / §4.7.2. Interned up front so the name constants exist
        // whether or not any member turns out to carry one.
        let needs_sig_attr = self.signature.is_some()
            || !self.field_signatures.is_empty()
            || self.methods.iter().any(|m| m.signature.is_some());
        let sig_attr = needs_sig_attr.then(|| pool.utf8("Signature"));
        let const_attr = (!self.field_constants.is_empty()).then(|| pool.utf8("ConstantValue"));
        let class_sig_idx = self.signature.as_deref().map(|s| pool.utf8(s));
        let mut field_idxs = Vec::new();
        for f in &self.fields {
            // A field name is an *unqualified name* (JVMS 4.2.2): `.`, `;`,
            // `[` and `/` are illegal in one. slick's `Library.scala` writes
            // `val / = new SqlOperator("/")`, and emitting the character raw
            // made `slick/ast/Library$` unloadable ("Illegal field name").
            // nsc runs every term name through the same NameTransformer it
            // uses for methods, so `/` is `$div`.
            let sig = self.field_signatures.get(&f.name).map(|s| pool.utf8(s));
            let cst = self.field_constants.get(&f.name).map(|v| pool.long(*v));
            let annots = match self.annots.fields.get(&f.name) {
                Some(a) => annotation_attrs(&mut pool, a, None),
                None => Vec::new(),
            };
            field_idxs.push((
                f.access,
                pool.utf8(&encode_method_name(&f.name)),
                pool.utf8(&f.desc),
                sig,
                cst,
                annots,
            ));
        }
        let code_attr = pool.utf8("Code");
        let stack_map_attr = pool.utf8("StackMapTable");
        let lvt_attr = self
            .methods
            .iter()
            .any(|m| m.code.is_some() && (!m.param_names.is_empty() || !m.local_names.is_empty()))
            .then(|| pool.utf8("LocalVariableTable"));
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
                let name = e
                    .inner_name
                    .as_deref()
                    .map_or(0, |n| pool.utf8(&encode_method_name(n)));
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
            let nt = m
                .as_ref()
                .map_or(0, |(n, d)| pool.name_and_type(&encode_method_name(n), d));
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
        let method_params_attr = if self.methods.iter().any(|m| !m.param_names.is_empty()) {
            Some(pool.utf8("MethodParameters"))
        } else {
            None
        };
        // RuntimeVisibleAnnotations { ScalaSignature { bytes = Utf8 } }, or
        // `ScalaLongSignature { bytes = { Utf8, ... } }` when one constant
        // could not hold the whole pickle, after the class's own Java
        // annotations.
        let scala_sig_annot = match (sig_type, bytes_name) {
            (Some(sig_ty), Some(bn)) => {
                let mut body = Vec::new();
                body.extend_from_slice(&sig_ty.to_be_bytes());
                body.extend_from_slice(&1u16.to_be_bytes());
                body.extend_from_slice(&bn.to_be_bytes());
                if long_sig {
                    body.push(b'[');
                    body.extend_from_slice(&(sig_utf8s.len() as u16).to_be_bytes());
                }
                for su in &sig_utf8s {
                    body.push(b's');
                    body.extend_from_slice(&su.to_be_bytes());
                }
                Some((1u16, body))
            }
            _ => None,
        };
        let class_annots = annotation_attrs(&mut pool, &self.annots.class, scala_sig_annot);
        let mut methods_data = Vec::new();
        for m in &self.methods {
            let n = pool.utf8(&m.name);
            let d = pool.utf8(&m.desc);
            let mut annots = annotation_attrs(&mut pool, &m.java_annots, None);
            annots.extend(param_annotation_attrs(&mut pool, &m.param_annots));
            let sig = m.signature.as_deref().map(|s| pool.utf8(s));
            if m.param_names.len() != m.param_flags.len()
                || (!m.param_names.is_empty()
                    && m.param_names.len() != method_parameter_count(&m.desc))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "MethodParameters count for {}{} does not match its descriptor",
                        m.name, m.desc
                    ),
                ));
            }
            if m.param_flags
                .iter()
                .any(|flags| flags & !(ACC_FINAL | ACC_SYNTHETIC | ACC_MANDATED) != 0)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("illegal MethodParameters flags on {}{}", m.name, m.desc),
                ));
            }
            let param_names: Vec<(Option<u16>, u16)> = m
                .param_names
                .iter()
                .zip(&m.param_flags)
                .map(|(name, flags)| (name.as_deref().map(|n| pool.utf8(n)), *flags))
                .collect();
            if param_names.len() > u8::MAX as usize {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("too many MethodParameters entries on {}{}", m.name, m.desc),
                ));
            }
            // JVMS §4.7.13, for the parameters only, as scalac's default
            // `-g:vars` writes them: libraries that read parameter names off
            // the bytecode (paranamer, which jackson-module-scala uses to pair
            // a case class's properties with its constructor) find none in
            // `MethodParameters`.
            let mut lvt: Vec<(u16, u16, u16)> = Vec::new();
            let names = if m.param_names.is_empty() {
                &m.local_names
            } else {
                &m.param_names
            };
            if m.code.is_some() && !names.is_empty() {
                let mut slot = 0u16;
                if m.access & ACC_STATIC == 0 {
                    let this_desc = format!("L{};", self.this_name);
                    lvt.push((pool.utf8("this"), pool.utf8(&this_desc), 0));
                    slot = 1;
                }
                for (desc, name) in param_descriptors(&m.desc).iter().zip(names) {
                    // Encoded, as nsc writes it: the JVM checks a table
                    // entry's name as it checks a field's, so a backquoted
                    // `https://…` parameter made the class unloadable.
                    if let Some(name) = name {
                        lvt.push((pool.utf8(&encode_method_name(name)), pool.utf8(desc), slot));
                    }
                    slot += if desc == "J" || desc == "D" { 2 } else { 1 };
                }
            }
            methods_data.push((
                m.access,
                n,
                d,
                m.code.clone(),
                annots,
                sig,
                param_names,
                lvt,
            ));
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
        for (acc, n, d, sig, cst, annots) in field_idxs {
            out.extend_from_slice(&acc.to_be_bytes());
            out.extend_from_slice(&n.to_be_bytes());
            out.extend_from_slice(&d.to_be_bytes());
            let n_attrs = u16::from(sig.is_some()) + u16::from(cst.is_some()) + annots.len() as u16;
            out.extend_from_slice(&n_attrs.to_be_bytes());
            if let (Some(a), Some(s)) = (const_attr, cst) {
                out.extend_from_slice(&a.to_be_bytes());
                out.extend_from_slice(&2u32.to_be_bytes());
                out.extend_from_slice(&s.to_be_bytes());
            }
            if let (Some(a), Some(s)) = (sig_attr, sig) {
                out.extend_from_slice(&a.to_be_bytes());
                out.extend_from_slice(&2u32.to_be_bytes());
                out.extend_from_slice(&s.to_be_bytes());
            }
            write_attrs(&mut out, &annots);
        }
        out.extend_from_slice(&(methods_data.len() as u16).to_be_bytes());
        for (acc, n, d, code, annots, sig, param_names, lvt) in methods_data {
            out.extend_from_slice(&acc.to_be_bytes());
            out.extend_from_slice(&n.to_be_bytes());
            out.extend_from_slice(&d.to_be_bytes());
            let n_attrs = u16::from(code.is_some())
                + annots.len() as u16
                + u16::from(sig.is_some())
                + u16::from(!param_names.is_empty());
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
                let lvt_attr = lvt_attr.filter(|_| !lvt.is_empty());
                let n_code_attrs = u16::from(c.stack_map.is_some()) + u16::from(lvt_attr.is_some());
                body.extend_from_slice(&n_code_attrs.to_be_bytes());
                if let Some(sm) = &c.stack_map {
                    body.extend_from_slice(&stack_map_attr.to_be_bytes());
                    body.extend_from_slice(&(sm.len() as u32).to_be_bytes());
                    body.extend_from_slice(sm);
                }
                if let Some(a) = lvt_attr {
                    body.extend_from_slice(&a.to_be_bytes());
                    body.extend_from_slice(&((2 + lvt.len() * 10) as u32).to_be_bytes());
                    body.extend_from_slice(&(lvt.len() as u16).to_be_bytes());
                    let len = c.bytes.len() as u16;
                    for (name, desc, slot) in &lvt {
                        body.extend_from_slice(&0u16.to_be_bytes());
                        body.extend_from_slice(&len.to_be_bytes());
                        body.extend_from_slice(&name.to_be_bytes());
                        body.extend_from_slice(&desc.to_be_bytes());
                        body.extend_from_slice(&slot.to_be_bytes());
                    }
                }
                out.extend_from_slice(&(body.len() as u32).to_be_bytes());
                out.extend_from_slice(&body);
            }
            write_attrs(&mut out, &annots);
            if let (Some(a), Some(s)) = (sig_attr, sig) {
                out.extend_from_slice(&a.to_be_bytes());
                out.extend_from_slice(&2u32.to_be_bytes());
                out.extend_from_slice(&s.to_be_bytes());
            }
            if !param_names.is_empty() {
                let a = method_params_attr.expect("MethodParameters utf8");
                out.extend_from_slice(&a.to_be_bytes());
                out.extend_from_slice(&((1 + param_names.len() * 4) as u32).to_be_bytes());
                out.push(param_names.len() as u8);
                for (name, flags) in param_names {
                    out.extend_from_slice(&name.unwrap_or(0).to_be_bytes());
                    out.extend_from_slice(&flags.to_be_bytes());
                }
            }
        }
        let n_class_attrs = 1u16
            + if class_sig_idx.is_some() { 1 } else { 0 }
            + class_annots.len() as u16
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
        if let (Some(a), Some(s)) = (sig_attr, class_sig_idx) {
            out.extend_from_slice(&a.to_be_bytes());
            out.extend_from_slice(&2u32.to_be_bytes());
            out.extend_from_slice(&s.to_be_bytes());
        }
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
        write_attrs(&mut out, &class_annots);
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
