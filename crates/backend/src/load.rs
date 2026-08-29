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
    /// `ACC_INTERFACE`. A Scala trait compiles to an interface, and calling one
    /// of its members needs `invokeinterface`, not `invokevirtual`.
    pub is_interface: bool,
    /// `super_class`, absent for `java/lang/Object` and for interfaces (whose
    /// super is always `Object`).
    pub super_name: Option<String>,
    /// `interfaces`, in declaration order.
    pub interfaces: Vec<String>,
}

use scala_rs_pickle::classfile::{parse_cp, skip_attrs, Cp, Cursor};

/// Decoded pickle bytes of a classfile's `ScalaSignature` / `ScalaLongSignature`.
pub use scala_rs_pickle::classfile::scala_signature_bytes;

fn parse_classfile(bytes: &[u8]) -> Option<LoadedClass> {
    let mut c = Cursor::new(bytes);
    if c.u4()? != 0xCAFEBABE {
        return None;
    }
    let _minor = c.u2()?;
    let _major = c.u2()?;
    let cp = parse_cp(&mut c)?;
    let access = c.u2()?;
    let is_interface = access & 0x0200 != 0;
    let this_i = c.u2()?;
    let internal_name = cp.class_name(this_i)?;
    let super_i = c.u2()?;
    let super_name = if super_i == 0 {
        None
    } else {
        cp.class_name(super_i)
    };
    let niface = c.u2()? as usize;
    let mut interfaces = Vec::new();
    for _ in 0..niface {
        let i = c.u2()?;
        if let Some(n) = cp.class_name(i) {
            interfaces.push(n);
        }
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
        is_interface,
        super_name,
        interfaces,
    })
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
