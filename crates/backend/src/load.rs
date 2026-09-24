//! Read JVM classfiles well enough to recover methods and ScalaSignature.

use crate::classfile::decode_method_name;
use crate::pickle::unpickle;
use std::path::{Path, PathBuf};

use scala_rs_pickle::{
    AbiError, JvmClassFlags, JvmDescriptor, JvmInternalName, JvmMemberFlags, JvmMemberKind,
};
pub use scala_rs_pickle::{
    LoadedClass as AbiLoadedClass, LoadedField as AbiLoadedField, LoadedMethod as AbiLoadedMethod,
};

use scala_rs_pickle::classfile::{parse_cp, skip_attrs, Cp, Cursor};

/// Decoded pickle bytes of a classfile's `ScalaSignature` / `ScalaLongSignature`.
pub use scala_rs_pickle::classfile::{
    scala_signature_bytes, scala_signature_bytes_result, ClassfileError,
};

/// Canonical, validated metadata for callers that need an ABI boundary.
pub fn parse_abi_class(bytes: &[u8]) -> Result<AbiLoadedClass, AbiClassError> {
    parse_abi_class_with_policy(bytes, true)
}

fn parse_abi_class_with_policy(
    bytes: &[u8],
    strict_scala_signature: bool,
) -> Result<AbiLoadedClass, AbiClassError> {
    let mut c = Cursor::new(bytes);
    if c.u4().ok_or("truncated classfile magic")? != 0xCAFEBABE {
        return Err(AbiClassError::Classfile(ClassfileError::BadMagic));
    }
    let _minor = c.u2().ok_or("truncated classfile version")?;
    let _major = c.u2().ok_or("truncated classfile version")?;
    let cp = parse_cp(&mut c).ok_or("malformed classfile constant pool")?;
    let access = c.u2().ok_or("truncated classfile access flags")?;
    let flags = JvmClassFlags::new(access);
    let this_i = c.u2().ok_or("truncated classfile this_class")?;
    let internal_name = JvmInternalName::new(
        cp.class_name(this_i)
            .ok_or("malformed classfile this_class")?,
    )?;
    let super_i = c.u2().ok_or("truncated classfile super_class")?;
    let super_name = if super_i == 0 {
        None
    } else {
        Some(JvmInternalName::new(
            cp.class_name(super_i)
                .ok_or("malformed classfile super_class")?,
        )?)
    };
    let niface = c.u2().ok_or("truncated classfile interfaces")? as usize;
    let mut interfaces = Vec::new();
    for _ in 0..niface {
        let i = c.u2().ok_or("truncated classfile interface")?;
        let name = cp.class_name(i).ok_or("malformed classfile interface")?;
        interfaces.push(JvmInternalName::new(name)?);
    }
    let nfields = c.u2().ok_or("truncated classfile fields")? as usize;
    let mut is_module = false;
    let mut fields = Vec::new();
    for _ in 0..nfields {
        let access = c.u2().ok_or("truncated classfile field")?;
        let name_i = c.u2().ok_or("truncated classfile field")?;
        let desc_i = c.u2().ok_or("truncated classfile field")?;
        if cp.utf8(name_i).as_deref() == Some("MODULE$") {
            is_module = true;
        }
        fields.push(AbiLoadedField {
            flags: JvmMemberFlags::new(JvmMemberKind::Field, access),
            name: cp.utf8(name_i).ok_or("malformed classfile field name")?,
            descriptor: JvmDescriptor::field(
                cp.utf8(desc_i)
                    .ok_or("malformed classfile field descriptor")?,
            )?,
        });
        skip_attrs(&mut c).ok_or("truncated classfile field attributes")?;
    }
    if internal_name.ends_with('$') && !internal_name.contains("$anon") {
        is_module = true;
    }
    let nmethods = c.u2().ok_or("truncated classfile methods")? as usize;
    let mut methods = Vec::new();
    for _ in 0..nmethods {
        let access = c.u2().ok_or("truncated classfile method")?;
        let name_i = c.u2().ok_or("truncated classfile method")?;
        let desc_i = c.u2().ok_or("truncated classfile method")?;
        let name = cp.utf8(name_i).ok_or("malformed classfile method name")?;
        let descriptor = JvmDescriptor::method(
            cp.utf8(desc_i)
                .ok_or("malformed classfile method descriptor")?,
        )?;
        let signature = method_signature(&mut c, &cp)?;
        // Constructors distinguish a real class with a companion object from
        // the constructor-free static mirror emitted for an object. Preserve
        // them in the canonical ABI even though ordinary member lookup later
        // gets the source-level constructor shape from the ScalaSignature.
        if name != "<clinit>" {
            methods.push(AbiLoadedMethod {
                flags: JvmMemberFlags::new(JvmMemberKind::Method, access),
                name: decode_method_name(&name),
                descriptor,
                signature,
            });
        }
    }
    let signature =
        scala_rs_pickle::classfile::scala_signature_bytes_from_class_attributes(&mut c, &cp);
    let pickle = if strict_scala_signature {
        signature?.and_then(|raw| unpickle(&raw))
    } else {
        signature.ok().flatten().and_then(|raw| unpickle(&raw))
    };
    Ok(AbiLoadedClass {
        internal_name,
        flags,
        is_module,
        methods,
        fields,
        pickle,
        super_name,
        interfaces,
    })
}

/// Read just the optional generic signature from a method's attributes.
///
/// `skip_attrs` is intentionally lossless for the pickle reader, but the
/// classpath typer also needs the method's generic return type. Keep this
/// parser local to the lightweight loader so the shared classfile substrate
/// remains unchanged.
fn method_signature(c: &mut Cursor<'_>, cp: &Cp) -> Result<Option<String>, AbiClassError> {
    let nattrs = c.u2().ok_or("truncated method attributes")? as usize;
    let mut signature = None;
    for _ in 0..nattrs {
        let name_i = c.u2().ok_or("truncated method attribute")?;
        let len = c.u4().ok_or("truncated method attribute")? as usize;
        let body = c.bytes(len).ok_or("truncated method attribute body")?;
        if cp.utf8(name_i).as_deref() == Some("Signature") {
            if len != 2 {
                return Err(AbiClassError::Malformed(
                    "method Signature attribute has an invalid length",
                ));
            }
            let mut body_cursor = Cursor::new(body);
            signature = Some(
                cp.utf8(
                    body_cursor
                        .u2()
                        .ok_or("truncated method Signature attribute")?,
                )
                .ok_or("malformed method Signature attribute")?,
            );
        }
    }
    Ok(signature)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbiClassError {
    Classfile(ClassfileError),
    InvalidAbi(AbiError),
    Malformed(&'static str),
}

impl From<&'static str> for AbiClassError {
    fn from(message: &'static str) -> Self {
        Self::Malformed(message)
    }
}

impl From<ClassfileError> for AbiClassError {
    fn from(error: ClassfileError) -> Self {
        Self::Classfile(error)
    }
}

impl From<AbiError> for AbiClassError {
    fn from(error: AbiError) -> Self {
        Self::InvalidAbi(error)
    }
}

impl std::fmt::Display for AbiClassError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Classfile(error) => error.fmt(f),
            Self::InvalidAbi(error) => error.fmt(f),
            Self::Malformed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for AbiClassError {}

/// Compatibility view returned by [`load_classpath`].
///
/// Keep these field names stable for backend callers while the checked ABI API
/// below exposes validated newtypes under the distinct `AbiLoaded*` names.
#[derive(Clone, Debug)]
pub struct LoadedMethod {
    pub access: u16,
    pub name: String,
    pub desc: String,
    pub signature: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LoadedField {
    pub access: u16,
    pub name: String,
    pub desc: String,
}

#[derive(Clone, Debug)]
pub struct LoadedClass {
    pub internal_name: String,
    pub is_module: bool,
    pub methods: Vec<LoadedMethod>,
    pub fields: Vec<LoadedField>,
    pub pickle: Option<scala_rs_pickle::PickledClass>,
    pub is_interface: bool,
    pub super_name: Option<String>,
    pub interfaces: Vec<String>,
}

impl From<AbiLoadedMethod> for LoadedMethod {
    fn from(method: AbiLoadedMethod) -> Self {
        Self {
            access: method.flags.bits(),
            name: method.name,
            desc: method.descriptor.into_string(),
            signature: method.signature,
        }
    }
}

impl From<AbiLoadedField> for LoadedField {
    fn from(field: AbiLoadedField) -> Self {
        Self {
            access: field.flags.bits(),
            name: field.name,
            desc: field.descriptor.into_string(),
        }
    }
}

impl From<AbiLoadedClass> for LoadedClass {
    fn from(class: AbiLoadedClass) -> Self {
        Self {
            internal_name: class.internal_name.into_string(),
            is_module: class.is_module,
            methods: class.methods.into_iter().map(Into::into).collect(),
            fields: class.fields.into_iter().map(Into::into).collect(),
            pickle: class.pickle,
            is_interface: class.flags.is_interface(),
            super_name: class.super_name.map(JvmInternalName::into_string),
            interfaces: class
                .interfaces
                .into_iter()
                .map(JvmInternalName::into_string)
                .collect(),
        }
    }
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
    collect_abi_classpath(paths, false)
        .0
        .into_iter()
        .map(Into::into)
        .collect()
}

/// Strict classpath loader for callers that cannot accept silent ABI loss.
///
/// Archive entries are still left to the typer's indexed jar loader. Classpath
/// directories remain best-effort like javac/scalac classpaths: stale roots and
/// unrelated unreadable or malformed entries are ignored. A standalone
/// `.class` path is an explicit request, so its I/O or parse failure is
/// returned instead of being mistaken for an absent class.
pub fn load_classpath_checked(
    paths: &[impl AsRef<Path>],
) -> Result<Vec<AbiLoadedClass>, Vec<ClasspathLoadError>> {
    let (classes, errors) = collect_abi_classpath(paths, true);
    if errors.is_empty() {
        Ok(classes)
    } else {
        Err(errors)
    }
}

fn collect_abi_classpath(
    paths: &[impl AsRef<Path>],
    strict_scala_signature: bool,
) -> (Vec<AbiLoadedClass>, Vec<ClasspathLoadError>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for p in paths {
        let p = p.as_ref();
        let explicit_class =
            p.extension().and_then(|s| s.to_str()) == Some("class") && (p.is_file() || !p.exists());
        if explicit_class {
            match std::fs::read(p) {
                Ok(bytes) => match parse_abi_class_with_policy(&bytes, strict_scala_signature) {
                    Ok(c) => {
                        if !skip_runtime(&c.internal_name) {
                            out.push(c);
                        }
                    }
                    Err(error) => errors.push(ClasspathLoadError::parse(p, error)),
                },
                Err(error) => errors.push(ClasspathLoadError::io(p, error)),
            }
            continue;
        }
        // Jars/jmods are handled lazily by `BinaryIndex`; the eager loader is
        // intentionally a directory/class-file scanner.
        if p.is_file() {
            continue;
        }
        collect_classes(p, &mut out, strict_scala_signature);
    }
    (out, errors)
}

fn collect_classes(dir: &Path, out: &mut Vec<AbiLoadedClass>, strict_scala_signature: bool) {
    let mut paths = Vec::new();
    collect_class_paths(dir, &mut paths);
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
    // Bound both I/O concurrency and buffered bytes. Decode in discovery order:
    // declaration order and classpath precedence must not depend on scheduling.
    for batch in paths.chunks(128) {
        let readers = if batch.len() < 16 { 1 } else { workers };
        for bytes in read_class_files(batch, readers, &|p| std::fs::read(p))
            .into_iter()
            .flatten()
        {
            if let Ok(c) = parse_abi_class_with_policy(&bytes, strict_scala_signature) {
                if !skip_runtime(&c.internal_name) {
                    out.push(c);
                }
            }
        }
    }
}

fn read_class_files(
    paths: &[PathBuf],
    workers: usize,
    read: &(impl Fn(&Path) -> std::io::Result<Vec<u8>> + Sync),
) -> Vec<Option<Vec<u8>>> {
    let workers = workers.clamp(1, 4).min(paths.len());
    if workers <= 1 {
        return paths.iter().map(|p| read(p).ok()).collect();
    }
    std::thread::scope(|scope| {
        let jobs: Vec<_> = paths
            .chunks(paths.len().div_ceil(workers))
            .map(|chunk| {
                scope.spawn(move || chunk.iter().map(|p| read(p).ok()).collect::<Vec<_>>())
            })
            .collect();
        jobs.into_iter()
            .flat_map(|job| job.join().expect("classpath reader panicked"))
            .collect()
    })
}

fn collect_class_paths(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd {
        let Ok(ent) = entry else {
            continue;
        };
        let path = ent.path();
        if path.is_dir() {
            collect_class_paths(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("class") {
            out.push(path);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClasspathLoadError {
    pub path: PathBuf,
    pub message: String,
}

impl ClasspathLoadError {
    fn parse(path: &Path, error: AbiClassError) -> Self {
        Self {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    }

    fn io(path: &Path, error: std::io::Error) -> Self {
        Self {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for ClasspathLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ClasspathLoadError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classfile::{ClassEmit, Field, Method, Pool};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn parallel_class_reads_are_bounded_and_preserve_order_and_failures() {
        let paths: Vec<_> = (0..128).map(|i| PathBuf::from(i.to_string())).collect();
        let threads = std::sync::Mutex::new(std::collections::HashSet::new());
        let read = |path: &Path| {
            threads.lock().unwrap().insert(std::thread::current().id());
            if path == Path::new("5") {
                Err(std::io::Error::from(std::io::ErrorKind::NotFound))
            } else {
                Ok(path.to_str().unwrap().as_bytes().to_vec())
            }
        };
        let parallel = read_class_files(&paths, 100, &read);
        assert_eq!(threads.lock().unwrap().len(), 4);
        assert_eq!(parallel[5], None);
        assert_eq!(parallel, read_class_files(&paths, 1, &read));
        assert!(read_class_files(&[], 4, &read).is_empty());
    }

    struct TestDir(PathBuf);

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_dir() -> TestDir {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "scala-rs-backend-load-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        TestDir(path)
    }

    fn metadata_emit(field_desc: &str, method_desc: &str) -> ClassEmit {
        ClassEmit {
            access: 0x0001 | 0x0020,
            this_name: "example/Metadata".into(),
            super_name: "java/lang/Object".into(),
            interfaces: vec!["java/io/Serializable".into()],
            fields: vec![Field {
                access: 0x0001 | 0x0040 | 0x0080 | 0x1000,
                name: "state".into(),
                desc: field_desc.into(),
            }],
            methods: vec![Method {
                access: 0x0001 | 0x0040 | 0x0080 | 0x1000 | 0x0400,
                name: "call".into(),
                desc: method_desc.into(),
                code: None,
                java_annots: Vec::new(),
                signature: None,
                param_names: Vec::new(),
                param_flags: Vec::new(),
            }],
            source: "Metadata.scala".into(),
            scala_signature: None,
            scala_raw: false,
            inner_classes: Vec::new(),
            enclosing_method: None,
            signature: None,
            field_signatures: std::collections::HashMap::new(),
            field_constants: std::collections::HashMap::new(),
        }
    }

    fn metadata_class(field_desc: &str, method_desc: &str) -> Vec<u8> {
        metadata_emit(field_desc, method_desc)
            .write_with_pool(Pool::new())
            .unwrap()
    }

    #[test]
    fn loader_roundtrips_names_descriptors_and_kind_specific_flags() {
        let loaded = parse_abi_class(&metadata_class("I", "(I)Ljava/lang/String;")).unwrap();
        assert_eq!(loaded.internal_name.as_str(), "example/Metadata");
        assert_eq!(
            loaded.super_name.as_ref().unwrap().as_str(),
            "java/lang/Object"
        );
        assert_eq!(loaded.interfaces[0].as_str(), "java/io/Serializable");

        let field = &loaded.fields[0];
        assert_eq!(field.descriptor.as_str(), "I");
        assert!(field.flags.is_volatile());
        assert!(field.flags.is_transient());
        assert!(field.flags.is_synthetic());
        assert!(!field.flags.is_bridge());

        let method = &loaded.methods[0];
        assert_eq!(method.descriptor.as_str(), "(I)Ljava/lang/String;");
        assert!(method.flags.is_bridge());
        assert!(method.flags.is_varargs());
        assert!(method.flags.is_synthetic());
        assert!(!method.flags.is_volatile());
        assert_eq!(loaded.all_methods().count(), 1);
        assert_eq!(loaded.bridge_methods().count(), 1);
        assert_eq!(loaded.scala_visible_methods().count(), 0);
    }

    #[test]
    fn loader_retains_jvm_constructors_for_class_identity() {
        let mut emit = metadata_emit("I", "()V");
        emit.methods.push(Method {
            access: 0x0001,
            name: "<init>".into(),
            desc: "(I)V".into(),
            code: None,
            java_annots: Vec::new(),
            signature: None,
            param_names: Vec::new(),
            param_flags: Vec::new(),
        });
        let loaded = parse_abi_class(&emit.write_with_pool(Pool::new()).unwrap()).unwrap();
        let constructor = loaded
            .methods
            .iter()
            .find(|method| method.name == "<init>")
            .expect("JVM constructor should remain in the canonical ABI");
        assert_eq!(constructor.descriptor.as_str(), "(I)V");
    }

    #[test]
    fn malformed_descriptor_is_a_load_error() {
        let error = parse_abi_class(&metadata_class("I", "(I"))
            .expect_err("invalid descriptor must not silently drop the class");
        assert!(
            error.to_string().contains("invalid JVM Method descriptor"),
            "{error}"
        );
    }

    #[test]
    fn unsupported_scala_pickle_keeps_validated_jvm_abi() {
        let raw_pickle = [0xff];
        let mut emit = metadata_emit("I", "()V");
        emit.scala_signature = Some(crate::pickle::encode_to_annotation_string(&raw_pickle));
        let bytes = emit.write_with_pool(Pool::new()).unwrap();
        assert_eq!(
            scala_signature_bytes_result(&bytes),
            Ok(Some(raw_pickle.to_vec()))
        );

        let loaded = parse_abi_class(&bytes)
            .expect("unsupported pickle content must not discard valid JVM metadata");

        assert!(loaded.pickle.is_none());
        assert_eq!(loaded.internal_name.as_str(), "example/Metadata");
        assert_eq!(loaded.fields[0].descriptor.as_str(), "I");
        assert_eq!(loaded.methods[0].descriptor.as_str(), "()V");
    }

    #[test]
    fn checked_directory_loading_ignores_stale_roots_and_unrelated_bad_entries() {
        let tmp = test_dir();
        let valid = tmp.0.join("Metadata.class");
        let malformed = tmp.0.join("Broken.class");
        let stale_root = tmp.0.join("stale-classpath-root");
        std::fs::write(&valid, metadata_class("I", "()V")).unwrap();
        std::fs::write(&malformed, b"not a classfile").unwrap();

        let canonical = load_classpath_checked(&[tmp.0.as_path(), stale_root.as_path()])
            .expect("directory entries and stale roots stay best-effort");
        assert_eq!(canonical.len(), 1);
        assert_eq!(canonical[0].internal_name.as_str(), "example/Metadata");

        let legacy = load_classpath(&[tmp.0.as_path(), stale_root.as_path()]);
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].internal_name, "example/Metadata");
        assert_eq!(legacy[0].methods[0].access, 0x14c1);
        assert_eq!(legacy[0].methods[0].desc, "()V");
        assert_eq!(legacy[0].fields[0].desc, "I");

        let errors = load_classpath_checked(&[malformed.as_path()])
            .expect_err("an explicitly named malformed class is fatal");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path, malformed);
        assert!(errors[0].message.contains("bad magic"));
    }

    #[test]
    fn legacy_loaded_struct_field_names_remain_constructible() {
        let method = LoadedMethod {
            access: 1,
            name: "m".into(),
            desc: "()V".into(),
            signature: None,
        };
        let field = LoadedField {
            access: 1,
            name: "f".into(),
            desc: "I".into(),
        };
        let class = LoadedClass {
            internal_name: "example/C".into(),
            is_module: false,
            methods: vec![method],
            fields: vec![field],
            pickle: None,
            is_interface: false,
            super_name: Some("java/lang/Object".into()),
            interfaces: Vec::new(),
        };
        assert_eq!(class.methods[0].desc, "()V");
    }
}
