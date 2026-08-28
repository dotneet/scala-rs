//! Read JVM classfiles well enough to recover methods and ScalaSignature.

use crate::classfile::decode_method_name;
use crate::pickle::{decode_annotation_string, unpickle, PickledClass};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct LoadedMethod {
    pub name: String,
    pub desc: String,
}

#[derive(Clone, Debug)]
pub struct LoadedClass {
    pub internal_name: String,
    pub is_module: bool,
    pub methods: Vec<LoadedMethod>,
    pub pickle: Option<PickledClass>,
}

struct Cp {
    tags: Vec<u8>,
    data: Vec<Vec<u8>>,
}

impl Cp {
    fn utf8(&self, i: u16) -> Option<String> {
        let i = i as usize;
        if i == 0 || i >= self.tags.len() {
            return None;
        }
        if self.tags[i] != 1 {
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

struct Cursor<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn new(b: &'a [u8]) -> Self {
        Cursor { b, i: 0 }
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

fn parse_classfile(bytes: &[u8]) -> Option<LoadedClass> {
    let mut c = Cursor::new(bytes);
    if c.u4()? != 0xCAFEBABE {
        return None;
    }
    let _minor = c.u2()?;
    let _major = c.u2()?;
    let cp_count = c.u2()? as usize;
    let mut tags = vec![0u8; cp_count];
    let mut data = vec![Vec::new(); cp_count];
    let mut i = 1usize;
    while i < cp_count {
        let tag = c.u1()?;
        tags[i] = tag;
        match tag {
            1 => {
                let n = c.u2()? as usize;
                data[i] = c.bytes(n)?.to_vec();
            }
            7 | 8 | 16 | 19 | 20 => {
                data[i] = c.u2()?.to_be_bytes().to_vec();
            }
            3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => {
                let mut v = c.u2()?.to_be_bytes().to_vec();
                v.extend_from_slice(&c.u2()?.to_be_bytes());
                data[i] = v;
            }
            5 | 6 => {
                data[i] = c.bytes(8)?.to_vec();
                i += 1; // long/double take two slots
            }
            15 => {
                let mut v = vec![c.u1()?];
                v.extend_from_slice(&c.u2()?.to_be_bytes());
                data[i] = v;
            }
            _ => return None,
        }
        i += 1;
    }
    let cp = Cp { tags, data };
    let _access = c.u2()?;
    let this_i = c.u2()?;
    let internal_name = cp.class_name(this_i)?;
    let _super = c.u2()?;
    let niface = c.u2()? as usize;
    for _ in 0..niface {
        let _ = c.u2()?;
    }
    let nfields = c.u2()? as usize;
    let mut is_module = false;
    for _ in 0..nfields {
        let _acc = c.u2()?;
        let name_i = c.u2()?;
        let _desc_i = c.u2()?;
        if cp.utf8(name_i).as_deref() == Some("MODULE$") {
            is_module = true;
        }
        skip_attrs(&mut c)?;
    }
    if internal_name.ends_with('$') && !internal_name.contains("$anon") {
        is_module = true;
    }
    let nmethods = c.u2()? as usize;
    let mut methods = Vec::new();
    for _ in 0..nmethods {
        let _acc = c.u2()?;
        let name_i = c.u2()?;
        let desc_i = c.u2()?;
        let name = cp.utf8(name_i)?;
        let desc = cp.utf8(desc_i)?;
        if name != "<init>" && name != "<clinit>" {
            methods.push(LoadedMethod {
                name: decode_method_name(&name),
                desc,
            });
        }
        skip_attrs(&mut c)?;
    }
    let nattrs = c.u2()? as usize;
    let mut pickle = None;
    for _ in 0..nattrs {
        let name_i = c.u2()?;
        let len = c.u4()? as usize;
        let body = c.bytes(len)?;
        let aname = cp.utf8(name_i).unwrap_or_default();
        if aname == "RuntimeVisibleAnnotations" {
            pickle = parse_scala_signature(body, &cp);
        }
    }
    Some(LoadedClass {
        internal_name,
        is_module,
        methods,
        pickle,
    })
}

fn skip_attrs(c: &mut Cursor) -> Option<()> {
    let n = c.u2()? as usize;
    for _ in 0..n {
        let _ = c.u2()?;
        let len = c.u4()? as usize;
        let _ = c.bytes(len)?;
    }
    Some(())
}

fn parse_scala_signature(body: &[u8], cp: &Cp) -> Option<PickledClass> {
    let mut c = Cursor::new(body);
    let nann = c.u2()? as usize;
    for _ in 0..nann {
        let type_i = c.u2()?;
        let ty = cp.utf8(type_i).unwrap_or_default();
        let npairs = c.u2()? as usize;
        let is_sig = ty.contains("ScalaSignature");
        for _ in 0..npairs {
            let name_i = c.u2()?;
            let tag = c.u1()?;
            let name = cp.utf8(name_i).unwrap_or_default();
            match tag {
                b's' => {
                    let ui = c.u2()?;
                    if is_sig && name == "bytes" {
                        let s = cp.utf8(ui)?;
                        let raw = decode_annotation_string(&s);
                        if let Some(p) = unpickle(&raw) {
                            return Some(p);
                        }
                    }
                }
                b'B' | b'C' | b'I' | b'S' | b'Z' => {
                    let _ = c.u2()?;
                }
                b'F' | b'D' | b'J' => {
                    let _ = c.u2()?;
                }
                b'e' => {
                    let _ = c.u2()?;
                    let _ = c.u2()?;
                }
                b'c' => {
                    let _ = c.u2()?;
                }
                b'@' => {
                    // nested annotation: skip conservatively by failing this pair
                    return None;
                }
                b'[' => {
                    return None;
                }
                _ => return None,
            }
        }
    }
    None
}

fn skip_runtime(name: &str) -> bool {
    name.starts_with("scala/")
        || name.starts_with("java/")
        || name.contains("$anon")
        || name.ends_with("$class")
}

/// Load `.class` files from classpath directories (non-recursive for the
/// default package, recursive for subdirectories).
pub fn load_classpath(paths: &[impl AsRef<Path>]) -> Vec<LoadedClass> {
    let mut out = Vec::new();
    for p in paths {
        let p = p.as_ref();
        if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("class") {
            if let Ok(bytes) = std::fs::read(p) {
                if let Some(c) = parse_classfile(&bytes) {
                    if !skip_runtime(&c.internal_name) {
                        out.push(c);
                    }
                }
            }
            continue;
        }
        collect_classes(p, &mut out);
    }
    out
}

fn collect_classes(dir: &Path, out: &mut Vec<LoadedClass>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if path.is_dir() {
            collect_classes(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("class") {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if let Some(c) = parse_classfile(&bytes) {
                if !skip_runtime(&c.internal_name) {
                    out.push(c);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Raw ScalaSignature extraction (for the full pickle reader)
// ---------------------------------------------------------------------------

/// Decoded pickle bytes of a classfile's `ScalaSignature` / `ScalaLongSignature`.
///
/// Unlike [`parse_classfile`] this keeps the raw pickle so
/// [`crate::pickle_read::read_pickle`] can parse all of it, and it handles the
/// `ScalaLongSignature` form (an array of strings, used for classes whose
/// pickle exceeds the 64K constant-pool string limit).
pub fn scala_signature_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut c = Cursor::new(bytes);
    if c.u4()? != 0xCAFEBABE {
        return None;
    }
    let _minor = c.u2()?;
    let _major = c.u2()?;
    let cp = parse_cp(&mut c)?;
    let _access = c.u2()?;
    let _this = c.u2()?;
    let _super = c.u2()?;
    let niface = c.u2()? as usize;
    for _ in 0..niface {
        let _ = c.u2()?;
    }
    for _ in 0..(c.u2()? as usize) {
        let _ = c.u2()?;
        let _ = c.u2()?;
        let _ = c.u2()?;
        skip_attrs(&mut c)?;
    }
    for _ in 0..(c.u2()? as usize) {
        let _ = c.u2()?;
        let _ = c.u2()?;
        let _ = c.u2()?;
        skip_attrs(&mut c)?;
    }
    let nattrs = c.u2()? as usize;
    for _ in 0..nattrs {
        let name_i = c.u2()?;
        let len = c.u4()? as usize;
        let body = c.bytes(len)?;
        if cp.utf8(name_i).as_deref() != Some("RuntimeVisibleAnnotations") {
            continue;
        }
        if let Some(s) = signature_string(body, &cp) {
            return Some(decode_annotation_string(&s));
        }
    }
    None
}

fn parse_cp(c: &mut Cursor) -> Option<Cp> {
    let cp_count = c.u2()? as usize;
    let mut tags = vec![0u8; cp_count];
    let mut data = vec![Vec::new(); cp_count];
    let mut i = 1usize;
    while i < cp_count {
        let tag = c.u1()?;
        tags[i] = tag;
        match tag {
            1 => {
                let n = c.u2()? as usize;
                data[i] = c.bytes(n)?.to_vec();
            }
            7 | 8 | 16 | 19 | 20 => {
                data[i] = c.u2()?.to_be_bytes().to_vec();
            }
            3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => {
                let mut v = c.u2()?.to_be_bytes().to_vec();
                v.extend_from_slice(&c.u2()?.to_be_bytes());
                data[i] = v;
            }
            5 | 6 => {
                data[i] = c.bytes(8)?.to_vec();
                i += 1; // long/double take two slots
            }
            15 => {
                let mut v = vec![c.u1()?];
                v.extend_from_slice(&c.u2()?.to_be_bytes());
                data[i] = v;
            }
            _ => return None,
        }
        i += 1;
    }
    Some(Cp { tags, data })
}

/// Concatenated `bytes` payload of `ScalaSignature` or `ScalaLongSignature`,
/// still in its `avoidZero`/7-bit encoding.
fn signature_string(body: &[u8], cp: &Cp) -> Option<String> {
    let mut c = Cursor::new(body);
    let nann = c.u2()? as usize;
    for _ in 0..nann {
        let type_i = c.u2()?;
        let ty = cp.utf8(type_i).unwrap_or_default();
        let is_sig = ty.contains("ScalaSignature") || ty.contains("ScalaLongSignature");
        let npairs = c.u2()? as usize;
        let mut found: Option<String> = None;
        for _ in 0..npairs {
            let name_i = c.u2()?;
            let name = cp.utf8(name_i).unwrap_or_default();
            let mut sink = String::new();
            read_element_value(&mut c, cp, &mut sink)?;
            if is_sig && name == "bytes" {
                found = Some(sink);
            }
        }
        if let Some(s) = found {
            return Some(s);
        }
    }
    None
}

/// Walk one `element_value`, appending any string constants to `sink`.
fn read_element_value(c: &mut Cursor, cp: &Cp, sink: &mut String) -> Option<()> {
    let tag = c.u1()?;
    match tag {
        b's' => {
            let ui = c.u2()?;
            sink.push_str(&cp.utf8(ui)?);
        }
        b'B' | b'C' | b'I' | b'S' | b'Z' | b'F' | b'D' | b'J' | b'c' => {
            let _ = c.u2()?;
        }
        b'e' => {
            let _ = c.u2()?;
            let _ = c.u2()?;
        }
        b'@' => {
            let _type_i = c.u2()?;
            let npairs = c.u2()? as usize;
            for _ in 0..npairs {
                let _ = c.u2()?;
                read_element_value(c, cp, sink)?;
            }
        }
        b'[' => {
            let n = c.u2()? as usize;
            for _ in 0..n {
                read_element_value(c, cp, sink)?;
            }
        }
        _ => return None,
    }
    Some(())
}
