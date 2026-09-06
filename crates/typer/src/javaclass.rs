//! On-demand Java classfile completion from `-cp`, jars, jmods, and the JDK.

// `class_cache` is asked for a class name on every completion, and hashing
// those names with SipHash was measurable. Iteration order is not observable
// here (the map is only ever looked up by key).
use rustc_hash::FxHashMap as HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PROTECTED: u16 = 0x0004;
const ACC_STATIC: u16 = 0x0008;
const ACC_VARARGS: u16 = 0x0080;
const ACC_INTERFACE: u16 = 0x0200;
const ACC_ABSTRACT: u16 = 0x0400;
const ACC_BRIDGE: u16 = 0x0040;
const ACC_SYNTHETIC: u16 = 0x1000;
const ACC_ENUM: u16 = 0x4000;
const ACC_MODULE: u16 = 0x8000;

#[derive(Clone, Debug)]
pub struct JavaMethod {
    pub name: String,
    pub desc: String,
    pub access: u16,
    pub signature: Option<String>,
}

#[derive(Clone, Debug)]
pub struct JavaField {
    pub name: String,
    pub desc: String,
    pub access: u16,
}

#[derive(Clone, Debug)]
pub struct JavaInnerClass {
    pub inner_jvm: String,
    pub access: u16,
}

#[derive(Clone, Debug)]
pub struct JavaClass {
    pub internal_name: String,
    pub access: u16,
    pub super_name: Option<String>,
    pub interfaces: Vec<String>,
    pub methods: Vec<JavaMethod>,
    pub fields: Vec<JavaField>,
    /// Descriptor of Scala's synthetic enclosing-instance field, when present.
    /// This can differ from the lexical `InnerClasses` owner for classes
    /// nested in component traits.
    pub outer_desc: Option<String>,
    pub signature: Option<String>,
    /// Nested type is `static` (`InnerClasses` / `$` in the name).
    pub nested_static: bool,
    /// Scala classfile (`ScalaSig` / `Scala` / `ScalaSignature`).
    pub is_scala: bool,
    /// Has a `MODULE$` static field (Scala module class).
    pub has_module_field: bool,
    pub inner_classes: Vec<JavaInnerClass>,
    /// The one non-static field, when the class has exactly one -- `private`
    /// ones included, which `fields` deliberately drops.
    ///
    /// This is the shape of a value class, and the field *is* its
    /// representation: `class Meters(val n: Int) extends AnyVal` compiles to
    /// `private final int n`, and outside the run that declared it there is
    /// nowhere else to read the underlying type from. See
    /// `pickle_supply::ensure_value_class_field`.
    pub sole_instance_field: Option<JavaField>,
}

/// A jar/jmod's bytes, shared so the `ZipArchive` that parses them can be kept
/// alive alongside the `BinaryIndex` instead of being rebuilt per lookup.
/// `off` skips the four-byte `JM\x01\0` header a `.jmod` puts before the zip.
#[derive(Clone)]
struct SharedBytes {
    data: std::sync::Arc<Vec<u8>>,
    off: usize,
}

impl AsRef<[u8]> for SharedBytes {
    fn as_ref(&self) -> &[u8] {
        &self.data[self.off..]
    }
}

/// One archive, parsed once. `ZipArchive::new` reads the whole central
/// directory (scala-library has ~10k entries); doing that per class lookup, for
/// each of the ~15 jars on a classpath, was the single largest cost in the
/// typer.
struct ZipIndex {
    archive: ZipArchive<Cursor<SharedBytes>>,
    /// Entry names, sorted, so a package-prefix query is a binary search rather
    /// than a linear walk over every entry.
    sorted_names: Vec<String>,
}

/// What a classpath entry turned out to be, decided once. `is_dir()`/`is_file()`
/// are syscalls, and the old code paid two of them per entry per lookup.
enum PathKind {
    Dir,
    Zip,
    /// Neither at startup: re-checked on use, so an output directory created
    /// during the run is still found.
    Unknown,
}

pub struct BinaryIndex {
    paths: Vec<Entry>,
    /// Every answer `find_class` has given, misses included. A miss costs a
    /// probe of every jar and jmod on the classpath, and the typer asks the
    /// same questions over and over (`has_pickle`, `class_file_of`,
    /// `java_class` and `ensure_class` all ask about the same name). Sound
    /// because nothing on `binary_path` is written during a run: the output
    /// directory is not on it, and the macro engine's classes go to a cache
    /// directory of their own.
    class_cache: HashMap<String, Option<Vec<u8>>>,
}

/// One classpath entry, with its archive once something has been looked up in it.
struct Entry {
    path: PathBuf,
    kind: PathKind,
    zip: Option<ZipIndex>,
}

impl BinaryIndex {
    pub fn from_user_paths(user: Vec<PathBuf>) -> Self {
        let mut raw = user;
        raw.extend(discover_jdk_jmods());
        let paths = raw
            .into_iter()
            .map(|p| {
                let kind = if p.is_dir() {
                    PathKind::Dir
                } else if is_zip_like(&p) {
                    PathKind::Zip
                } else {
                    PathKind::Unknown
                };
                Entry {
                    path: p,
                    kind,
                    zip: None,
                }
            })
            .collect();
        BinaryIndex {
            paths,
            class_cache: HashMap::default(),
        }
    }

    pub fn find_class(&mut self, internal: &str) -> Result<Option<Vec<u8>>, String> {
        if let Some(hit) = self.class_cache.get(internal) {
            return Ok(hit.clone());
        }
        let found = self.find_class_uncached(internal)?;
        self.class_cache.insert(internal.to_string(), found.clone());
        Ok(found)
    }

    fn find_class_uncached(&mut self, internal: &str) -> Result<Option<Vec<u8>>, String> {
        let rel = format!("{internal}.class");
        let alt = format!("classes/{rel}");
        for i in 0..self.paths.len() {
            match self.paths[i].kind {
                PathKind::Zip => {
                    let e = load_zip(&mut self.paths[i])?;
                    let path = &e.path;
                    let z = e.zip.as_mut().expect("zip loaded");
                    for n in [&rel, &alt] {
                        match z.archive.by_name(n) {
                            Ok(mut f) => {
                                let mut buf = Vec::with_capacity(f.size() as usize);
                                f.read_to_end(&mut buf).map_err(|err| {
                                    format!(
                                        "unsupported classfile archive {}: {err}",
                                        path.display()
                                    )
                                })?;
                                return Ok(Some(buf));
                            }
                            Err(zip::result::ZipError::FileNotFound) => {}
                            Err(err) => {
                                return Err(format!(
                                    "unsupported classfile archive {}: {err}",
                                    path.display()
                                ))
                            }
                        }
                    }
                }
                // `Unknown` too: a path that was neither a directory nor an
                // archive at startup may have become a directory since.
                PathKind::Dir | PathKind::Unknown => {
                    let f = self.paths[i].path.join(&rel);
                    if f.is_file() {
                        return std::fs::read(&f)
                            .map(Some)
                            .map_err(|e| format!("cannot read {}: {e}", f.display()));
                    }
                }
            }
        }
        Ok(None)
    }

    pub fn has_package_prefix(&mut self, prefix: &str) -> bool {
        let dir_rel = prefix.trim_end_matches('/');
        let alt = format!("classes/{prefix}");
        for i in 0..self.paths.len() {
            match self.paths[i].kind {
                PathKind::Zip => {
                    let Ok(e) = load_zip(&mut self.paths[i]) else {
                        continue;
                    };
                    let names = &e.zip.as_ref().expect("zip loaded").sorted_names;
                    if has_sorted_prefix(names, prefix) || has_sorted_prefix(names, &alt) {
                        return true;
                    }
                }
                PathKind::Dir | PathKind::Unknown => {
                    if self.paths[i].path.join(dir_rel).is_dir() {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Parse `e`'s archive if it has not been parsed yet, and hand `e` back.
fn load_zip(e: &mut Entry) -> Result<&mut Entry, String> {
    if e.zip.is_none() {
        let data = std::fs::read(&e.path)
            .map_err(|err| format!("cannot read {}: {err}", e.path.display()))?;
        // A `.jmod` puts a four-byte `JM\x01\0` header before the zip.
        let off = usize::from(data.len() >= 4 && data[0] == b'J' && data[1] == b'M') * 4;
        let shared = SharedBytes {
            data: std::sync::Arc::new(data),
            off,
        };
        let archive = ZipArchive::new(Cursor::new(shared))
            .map_err(|err| format!("unsupported classfile archive {}: {err}", e.path.display()))?;
        let mut sorted_names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        sorted_names.sort_unstable();
        e.zip = Some(ZipIndex {
            archive,
            sorted_names,
        });
    }
    Ok(e)
}

/// `names` is sorted, so every entry starting with `prefix` is contiguous and
/// the first of them is at the insertion point of `prefix` itself.
fn has_sorted_prefix(names: &[String], prefix: &str) -> bool {
    let at = names.partition_point(|n| n.as_str() < prefix);
    names.get(at).is_some_and(|n| n.starts_with(prefix))
}

fn is_zip_like(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|s| s.to_str()),
        Some("jar" | "zip" | "jmod")
    ) && p.is_file()
}

fn discover_jdk_jmods() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut homes = Vec::new();
    if let Ok(h) = std::env::var("JAVA_HOME") {
        homes.push(PathBuf::from(h));
    }
    if let Ok(java) = std::fs::read_link("/usr/bin/java")
        .or_else(|_| std::fs::read_link("/bin/java"))
        .or_else(|_| which_java())
    {
        // …/bin/java → home
        if let Some(home) = java.parent().and_then(|b| b.parent()) {
            homes.push(home.to_path_buf());
        }
    }
    // macOS: `/usr/bin/java` is a stub, not a symlink into a JDK. Ask java_home.
    if let Ok(o) = std::process::Command::new("/usr/libexec/java_home").output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() {
                homes.push(PathBuf::from(s));
            }
        }
    }
    homes.push(PathBuf::from("/usr/lib/jvm/default-java"));
    homes.push(PathBuf::from("/usr/lib/jvm/java-21-openjdk-amd64"));
    let mut seen = std::collections::HashSet::new();
    for home in homes {
        let jmods = home.join("jmods");
        let base = jmods.join("java.base.jmod");
        if base.is_file() && seen.insert(base.clone()) {
            out.push(base);
            if let Ok(rd) = std::fs::read_dir(&jmods) {
                for ent in rd.flatten() {
                    let p = ent.path();
                    if p.extension().and_then(|s| s.to_str()) == Some("jmod")
                        && seen.insert(p.clone())
                    {
                        out.push(p);
                    }
                }
            }
        }
        let rt = home.join("lib/rt.jar");
        if rt.is_file() && seen.insert(rt.clone()) {
            out.push(rt);
        }
    }
    out
}

fn which_java() -> std::io::Result<PathBuf> {
    let p = std::process::Command::new("which").arg("java").output()?;
    if !p.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "which java",
        ));
    }
    let s = String::from_utf8_lossy(&p.stdout).trim().to_string();
    let path = PathBuf::from(s);
    std::fs::read_link(&path).or(Ok(path))
}

pub fn parse_java_classfile(bytes: &[u8]) -> Result<JavaClass, String> {
    let mut c = CursorJ::new(bytes);
    if c.u4().ok_or("truncated classfile")? != 0xCAFEBABE {
        return Err("not a classfile (bad magic)".into());
    }
    let _minor = c.u2().ok_or("truncated classfile")?;
    let _major = c.u2().ok_or("truncated classfile")?;
    let cp_count = c.u2().ok_or("truncated classfile")? as usize;
    let mut tags = vec![0u8; cp_count];
    let mut data = vec![Vec::new(); cp_count];
    let mut i = 1usize;
    while i < cp_count {
        let tag = c.u1().ok_or("truncated constant pool")?;
        tags[i] = tag;
        match tag {
            1 => {
                let n = c.u2().ok_or("truncated Utf8")? as usize;
                data[i] = c.bytes(n).ok_or("truncated Utf8")?.to_vec();
            }
            7 | 8 | 16 | 19 | 20 => {
                data[i] = c.u2().ok_or("truncated cp")?.to_be_bytes().to_vec();
            }
            3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => {
                let mut v = c.u2().ok_or("truncated cp")?.to_be_bytes().to_vec();
                v.extend_from_slice(&c.u2().ok_or("truncated cp")?.to_be_bytes());
                data[i] = v;
            }
            5 | 6 => {
                data[i] = c.bytes(8).ok_or("truncated long/double")?.to_vec();
                i += 1;
            }
            15 => {
                let mut v = vec![c.u1().ok_or("truncated MethodHandle")?];
                v.extend_from_slice(&c.u2().ok_or("truncated MethodHandle")?.to_be_bytes());
                data[i] = v;
            }
            other => return Err(format!("unsupported classfile constant pool tag {other}")),
        }
        i += 1;
    }
    let cp = Cp { tags, data };
    let access = c.u2().ok_or("truncated classfile")?;
    if access & ACC_MODULE != 0 {
        return Err("unsupported classfile: ACC_MODULE".into());
    }
    let this_i = c.u2().ok_or("truncated classfile")?;
    let internal_name = cp
        .class_name(this_i)
        .ok_or_else(|| "unsupported classfile: this_class".to_string())?;
    let super_i = c.u2().ok_or("truncated classfile")?;
    let super_name = if super_i == 0 {
        None
    } else {
        cp.class_name(super_i)
    };
    let niface = c.u2().ok_or("truncated classfile")? as usize;
    let mut interfaces = Vec::new();
    for _ in 0..niface {
        let i = c.u2().ok_or("truncated classfile")?;
        if let Some(n) = cp.class_name(i) {
            interfaces.push(n);
        }
    }
    let nfields = c.u2().ok_or("truncated classfile")? as usize;
    let mut fields = Vec::new();
    let mut outer_desc = None;
    let mut instance_fields: Vec<JavaField> = Vec::new();
    for _ in 0..nfields {
        let acc = c.u2().ok_or("truncated field")?;
        let name_i = c.u2().ok_or("truncated field")?;
        let desc_i = c.u2().ok_or("truncated field")?;
        let attrs = read_attrs(&mut c, &cp).ok_or("truncated field attributes")?;
        let _ = attrs;
        let name = cp.utf8(name_i).ok_or("unsupported classfile field name")?;
        let desc = cp.utf8(desc_i).ok_or("unsupported classfile field desc")?;
        if name == "$outer" && acc & ACC_STATIC == 0 {
            outer_desc = Some(desc.clone());
        }
        if acc & ACC_STATIC == 0 && instance_fields.len() < 2 {
            instance_fields.push(JavaField {
                name: name.clone(),
                desc: desc.clone(),
                access: acc,
            });
        }
        if acc & ACC_SYNTHETIC == 0 && java_member_visible(acc) {
            fields.push(JavaField {
                name,
                desc,
                access: acc,
            });
        }
    }
    let sole_instance_field = if instance_fields.len() == 1 {
        instance_fields.pop()
    } else {
        None
    };
    let nmethods = c.u2().ok_or("truncated classfile")? as usize;
    let mut methods = Vec::new();
    for _ in 0..nmethods {
        let acc = c.u2().ok_or("truncated method")?;
        let name_i = c.u2().ok_or("truncated method")?;
        let desc_i = c.u2().ok_or("truncated method")?;
        let attrs = read_attrs(&mut c, &cp).ok_or("truncated method attributes")?;
        let name = cp.utf8(name_i).ok_or("unsupported classfile method name")?;
        let desc = cp.utf8(desc_i).ok_or("unsupported classfile method desc")?;
        if name == "<clinit>" {
            continue;
        }
        if acc & ACC_SYNTHETIC != 0 || acc & ACC_BRIDGE != 0 {
            continue;
        }
        if !java_member_visible(acc) {
            continue;
        }
        methods.push(JavaMethod {
            name,
            desc,
            access: acc,
            signature: attr_utf8(&attrs, "Signature", &cp),
        });
    }
    let class_attrs = read_attrs(&mut c, &cp).unwrap_or_default();
    let signature = attr_utf8(&class_attrs, "Signature", &cp);
    let inner_classes = parse_inner_classes(&class_attrs, &cp);
    let nested_static = nested_is_static(&internal_name, &inner_classes);
    let is_scala = class_attrs.iter().any(|(n, _)| {
        n == "ScalaSig" || n == "Scala" || n == "ScalaSignature" || n == "ScalaLongSignature"
    });
    let has_module_field = fields.iter().any(|f| f.name == "MODULE$");
    Ok(JavaClass {
        internal_name,
        access,
        super_name,
        interfaces,
        methods,
        fields,
        outer_desc,
        signature,
        nested_static,
        is_scala,
        has_module_field,
        inner_classes,
        sole_instance_field,
    })
}

pub fn is_java_interface(access: u16) -> bool {
    access & ACC_INTERFACE != 0
}

pub fn is_java_static(access: u16) -> bool {
    access & ACC_STATIC != 0
}

/// A bridge is the compiler's own copy of a signature the class inherits, so
/// it never introduces an alternative the source did not write.
pub fn is_java_bridge(access: u16) -> bool {
    access & ACC_BRIDGE != 0
}

pub fn is_java_abstract(access: u16) -> bool {
    access & ACC_ABSTRACT != 0
}

pub fn is_java_varargs(access: u16) -> bool {
    access & ACC_VARARGS != 0
}

pub fn is_java_protected(access: u16) -> bool {
    access & ACC_PROTECTED != 0
}

pub fn is_java_enum(access: u16) -> bool {
    access & ACC_ENUM != 0
}

fn java_member_visible(access: u16) -> bool {
    access & ACC_PUBLIC != 0 || access & ACC_PROTECTED != 0
}

struct Cp {
    tags: Vec<u8>,
    data: Vec<Vec<u8>>,
}

impl Cp {
    fn utf8(&self, i: u16) -> Option<String> {
        let i = i as usize;
        if i == 0 || i >= self.tags.len() || self.tags[i] != 1 {
            return None;
        }
        modified_utf8_to_string(&self.data[i])
    }

    fn class_name(&self, i: u16) -> Option<String> {
        let i = i as usize;
        if i == 0 || i >= self.tags.len() || self.tags[i] != 7 {
            return None;
        }
        if self.data[i].len() < 2 {
            return None;
        }
        let u = u16::from_be_bytes([self.data[i][0], self.data[i][1]]);
        self.utf8(u)
    }
}

fn modified_utf8_to_string(b: &[u8]) -> Option<String> {
    let mut s = String::new();
    let mut i = 0;
    while i < b.len() {
        let x = b[i];
        if x == 0 {
            return None;
        } else if x < 0x80 {
            s.push(x as char);
            i += 1;
        } else if x & 0xe0 == 0xc0 {
            if i + 1 >= b.len() {
                return None;
            }
            let y = b[i + 1];
            let c = (((x as u32) & 0x1f) << 6) | ((y as u32) & 0x3f);
            s.push(char::from_u32(c)?);
            i += 2;
        } else if x & 0xf0 == 0xe0 {
            if i + 2 >= b.len() {
                return None;
            }
            let y = b[i + 1];
            let z = b[i + 2];
            let c = (((x as u32) & 0x0f) << 12) | (((y as u32) & 0x3f) << 6) | ((z as u32) & 0x3f);
            s.push(char::from_u32(c)?);
            i += 3;
        } else {
            return None;
        }
    }
    Some(s)
}

struct CursorJ<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> CursorJ<'a> {
    fn new(b: &'a [u8]) -> Self {
        CursorJ { b, i: 0 }
    }
    fn u1(&mut self) -> Option<u8> {
        if self.i >= self.b.len() {
            return None;
        }
        let v = self.b[self.i];
        self.i += 1;
        Some(v)
    }
    fn u2(&mut self) -> Option<u16> {
        let hi = self.u1()? as u16;
        let lo = self.u1()? as u16;
        Some((hi << 8) | lo)
    }
    fn u4(&mut self) -> Option<u32> {
        let a = self.u2()? as u32;
        let b = self.u2()? as u32;
        Some((a << 16) | b)
    }
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.i + n > self.b.len() {
            return None;
        }
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        Some(s)
    }
}

fn read_attrs(c: &mut CursorJ, cp: &Cp) -> Option<Vec<(String, Vec<u8>)>> {
    let n = c.u2()? as usize;
    let mut out = Vec::new();
    for _ in 0..n {
        let name_i = c.u2()?;
        let len = c.u4()? as usize;
        let body = c.bytes(len)?.to_vec();
        if let Some(name) = cp.utf8(name_i) {
            out.push((name, body));
        }
    }
    Some(out)
}

fn attr_utf8(attrs: &[(String, Vec<u8>)], name: &str, cp: &Cp) -> Option<String> {
    let body = attrs.iter().find(|(n, _)| n == name)?.1.as_slice();
    if body.len() < 2 {
        return None;
    }
    let i = u16::from_be_bytes([body[0], body[1]]);
    cp.utf8(i)
}

fn parse_inner_classes(attrs: &[(String, Vec<u8>)], cp: &Cp) -> Vec<JavaInnerClass> {
    let Some((_, body)) = attrs.iter().find(|(n, _)| n == "InnerClasses") else {
        return Vec::new();
    };
    if body.len() < 2 {
        return Vec::new();
    }
    let n = u16::from_be_bytes([body[0], body[1]]) as usize;
    let mut i = 2usize;
    let mut out = Vec::new();
    for _ in 0..n {
        if i + 8 > body.len() {
            break;
        }
        let inner_i = u16::from_be_bytes([body[i], body[i + 1]]);
        let flags = u16::from_be_bytes([body[i + 6], body[i + 7]]);
        i += 8;
        let Some(inner_jvm) = cp.class_name(inner_i) else {
            continue;
        };
        out.push(JavaInnerClass {
            inner_jvm,
            access: flags,
        });
    }
    out
}

fn nested_is_static(this: &str, inners: &[JavaInnerClass]) -> bool {
    if let Some(ic) = inners.iter().find(|ic| ic.inner_jvm == this) {
        return ic.access & ACC_STATIC != 0;
    }
    this.contains('$')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jdk_math_if_present() {
        let mut idx = BinaryIndex::from_user_paths(Vec::new());
        let Some(bytes) = idx.find_class("java/lang/Math").unwrap() else {
            panic!("JDK java.lang.Math.class must be readable from jmods/rt");
        };
        let c = parse_java_classfile(&bytes).expect("parse Math");
        assert!(
            c.methods
                .iter()
                .any(|m| m.name == "abs" && m.desc == "(I)I"),
            "Math.abs(int) missing: {:?}",
            c.methods
                .iter()
                .filter(|m| m.name == "abs")
                .collect::<Vec<_>>()
        );
        assert!(is_java_static(
            c.methods
                .iter()
                .find(|m| m.name == "abs" && m.desc == "(I)I")
                .unwrap()
                .access
        ));
    }

    #[test]
    fn parses_jdk_arraylist_if_present() {
        let mut idx = BinaryIndex::from_user_paths(Vec::new());
        let Some(bytes) = idx.find_class("java/util/ArrayList").unwrap() else {
            panic!("JDK java.util.ArrayList.class must be readable from jmods/rt");
        };
        let c = parse_java_classfile(&bytes).expect("parse ArrayList");
        assert!(
            c.signature.as_deref().is_some_and(|s| s.starts_with("<E:")),
            "ArrayList class Signature missing: {:?}",
            c.signature
        );
        let get = c
            .methods
            .iter()
            .find(|m| m.name == "get" && m.desc == "(I)Ljava/lang/Object;")
            .expect("ArrayList.get(int)");
        assert_eq!(get.signature.as_deref(), Some("(I)TE;"));
        assert!(
            c.methods
                .iter()
                .any(|m| m.name == "add" && m.desc == "(Ljava/lang/Object;)Z"),
            "ArrayList.add(Object) missing"
        );
        assert!(
            c.methods
                .iter()
                .any(|m| m.name == "size" && m.desc == "()I"),
            "ArrayList.size() missing"
        );
    }

    #[test]
    fn parses_jdk_map_entry_inner_if_present() {
        let mut idx = BinaryIndex::from_user_paths(Vec::new());
        let Some(bytes) = idx.find_class("java/util/Map$Entry").unwrap() else {
            panic!("JDK java.util.Map$Entry.class must be readable from jmods/rt");
        };
        let c = parse_java_classfile(&bytes).expect("parse Map$Entry");
        assert!(is_java_interface(c.access));
        assert!(
            c.nested_static,
            "Map.Entry should be a static nested type: {:?}",
            c
        );
        assert!(
            c.signature
                .as_deref()
                .is_some_and(|s| s.starts_with("<K:") && s.contains("V:")),
            "Map.Entry class Signature missing: {:?}",
            c.signature
        );
        let get_key = c
            .methods
            .iter()
            .find(|m| m.name == "getKey" && m.desc == "()Ljava/lang/Object;")
            .expect("Map.Entry.getKey");
        assert_eq!(get_key.signature.as_deref(), Some("()TK;"));
    }

    #[test]
    fn parses_jdk_string_format_varargs_if_present() {
        let mut idx = BinaryIndex::from_user_paths(Vec::new());
        let Some(bytes) = idx.find_class("java/lang/String").unwrap() else {
            panic!("JDK java.lang.String.class must be readable from jmods/rt");
        };
        let c = parse_java_classfile(&bytes).expect("parse String");
        let fmt = c
            .methods
            .iter()
            .find(|m| {
                m.name == "format"
                    && m.desc == "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;"
            })
            .expect("String.format(String, Object...)");
        assert!(
            is_java_varargs(fmt.access),
            "String.format missing ACC_VARARGS: {:#x}",
            fmt.access
        );
        assert!(is_java_static(fmt.access));
    }

    #[test]
    fn parses_jdk_class_forname_wildcard_if_present() {
        let mut idx = BinaryIndex::from_user_paths(Vec::new());
        let Some(bytes) = idx.find_class("java/lang/Class").unwrap() else {
            panic!("JDK java.lang.Class.class must be readable from jmods/rt");
        };
        let c = parse_java_classfile(&bytes).expect("parse Class");
        let fnm = c
            .methods
            .iter()
            .find(|m| m.name == "forName" && m.desc == "(Ljava/lang/String;)Ljava/lang/Class;")
            .expect("Class.forName(String)");
        assert_eq!(
            fnm.signature.as_deref(),
            Some("(Ljava/lang/String;)Ljava/lang/Class<*>;")
        );
    }

    #[test]
    fn parses_jdk_collections_max_bounds_if_present() {
        let mut idx = BinaryIndex::from_user_paths(Vec::new());
        let Some(bytes) = idx.find_class("java/util/Collections").unwrap() else {
            panic!("JDK java.util.Collections.class must be readable from jmods/rt");
        };
        let c = parse_java_classfile(&bytes).expect("parse Collections");
        let max = c
            .methods
            .iter()
            .find(|m| m.name == "max" && m.desc == "(Ljava/util/Collection;)Ljava/lang/Object;")
            .expect("Collections.max(Collection)");
        assert_eq!(
            max.signature.as_deref(),
            Some("<T:Ljava/lang/Object;:Ljava/lang/Comparable<-TT;>;>(Ljava/util/Collection<+TT;>;)TT;")
        );
    }

    #[test]
    fn parses_jdk_thread_sleep_if_present() {
        let mut idx = BinaryIndex::from_user_paths(Vec::new());
        let Some(bytes) = idx.find_class("java/lang/Thread").unwrap() else {
            panic!("JDK java.lang.Thread.class must be readable from jmods/rt");
        };
        let c = parse_java_classfile(&bytes).expect("parse Thread");
        assert!(
            c.methods
                .iter()
                .any(|m| m.name == "sleep" && m.desc == "(J)V" && is_java_static(m.access)),
            "Thread.sleep(long) missing"
        );
    }

    #[test]
    fn parses_jdk_thread_state_enum_if_present() {
        let mut idx = BinaryIndex::from_user_paths(Vec::new());
        let Some(bytes) = idx.find_class("java/lang/Thread$State").unwrap() else {
            panic!("JDK java.lang.Thread$State.class must be readable from jmods/rt");
        };
        let c = parse_java_classfile(&bytes).expect("parse Thread.State");
        assert!(
            is_java_enum(c.access),
            "Thread.State must have ACC_ENUM: access={:#x}",
            c.access
        );
        assert_eq!(c.super_name.as_deref(), Some("java/lang/Enum"));
        assert!(
            c.fields
                .iter()
                .any(|f| f.name == "NEW" && is_java_enum(f.access) && is_java_static(f.access)),
            "enum constant NEW missing: {:?}",
            c.fields
        );
        assert!(
            c.methods.iter().any(|m| m.name == "values"
                && m.desc == "()[Ljava/lang/Thread$State;"
                && is_java_static(m.access)),
            "values() missing: {:?}",
            c.methods
                .iter()
                .filter(|m| m.name == "values")
                .collect::<Vec<_>>()
        );
        assert!(
            c.methods.iter().any(|m| m.name == "valueOf"
                && m.desc == "(Ljava/lang/String;)Ljava/lang/Thread$State;"
                && is_java_static(m.access)),
            "valueOf(String) missing"
        );
    }

    #[test]
    fn unknown_cp_tag_is_unsupported() {
        let mut b = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 52, 0, 2, 99];
        let err = parse_java_classfile(&b).unwrap_err();
        assert!(
            err.contains("unsupported classfile constant pool tag 99"),
            "{err}"
        );
        b[0] = 0x00;
        let err = parse_java_classfile(&b).unwrap_err();
        assert!(err.contains("not a classfile"), "{err}");
    }

    #[test]
    fn parses_scala_library_ordering_enum_inners_if_present() {
        let jar = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
        if !jar.is_file() {
            return;
        }
        let mut idx = BinaryIndex::from_user_paths(vec![jar]);
        let Some(bytes) = idx.find_class("scala/math/Ordering").unwrap() else {
            panic!("scala-library Ordering.class must be readable");
        };
        let c = parse_java_classfile(&bytes).expect("parse Ordering");
        assert!(c.is_scala, "Ordering must carry ScalaSig: {c:?}");
        assert!(
            c.inner_classes
                .iter()
                .any(|ic| ic.inner_jvm == "scala/math/Ordering$Int$"),
            "InnerClasses must list Ordering$Int$: {:?}",
            c.inner_classes
                .iter()
                .map(|ic| &ic.inner_jvm)
                .collect::<Vec<_>>()
        );
        assert!(
            c.methods.iter().any(|m| m.name == "compare"),
            "Ordering.compare missing: {:?}",
            c.methods.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
        let Some(ibytes) = idx.find_class("scala/math/Ordering$Int$").unwrap() else {
            panic!("scala-library Ordering$Int$.class must be readable");
        };
        let ic = parse_java_classfile(&ibytes).expect("parse Ordering$Int$");
        assert!(
            ic.has_module_field,
            "Ordering$Int$ must have MODULE$: {:?}",
            ic.fields
        );
        assert!(
            ic.methods
                .iter()
                .any(|m| m.name == "compare" && m.desc == "(II)I"),
            "Ordering$Int$.compare(int,int) missing: {:?}",
            ic.methods
                .iter()
                .filter(|m| m.name == "compare")
                .collect::<Vec<_>>()
        );
    }
}
