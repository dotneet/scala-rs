//! Canonical low-level metadata shared at the classfile/ScalaSignature boundary.
//!
//! This module deliberately stops before the typer's symbol table.  A classfile
//! loader can retain binary identities and flags here without depending on the
//! backend or on semantic compiler types.

use std::fmt;
use std::ops::Deref;

/// A binary class name in JVM internal form (`java/lang/String`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JvmInternalName(String);

impl JvmInternalName {
    pub fn new(name: impl Into<String>) -> Result<Self, AbiError> {
        let name = name.into();
        if name.is_empty()
            || name.starts_with('/')
            || name.ends_with('/')
            || name.split('/').any(str::is_empty)
            || name.bytes().any(|b| matches!(b, b'.' | b';' | b'['))
        {
            return Err(AbiError::InvalidInternalName(name));
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl Deref for JvmInternalName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for JvmInternalName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for JvmInternalName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The kind of classfile member whose flags/descriptor are being interpreted.
///
/// Some JVM access bits are context-sensitive: `0x0040` means `VOLATILE` on a
/// field and `BRIDGE` on a method; `0x0080` similarly means `TRANSIENT` or
/// `VARARGS`. Keeping the kind next to the bits prevents capability loss at an
/// adapter boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JvmMemberKind {
    Field,
    Method,
}

/// A validated JVM field or method descriptor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JvmDescriptor {
    value: String,
    kind: JvmMemberKind,
}

impl JvmDescriptor {
    pub fn field(value: impl Into<String>) -> Result<Self, AbiError> {
        Self::new(value.into(), JvmMemberKind::Field)
    }

    pub fn method(value: impl Into<String>) -> Result<Self, AbiError> {
        Self::new(value.into(), JvmMemberKind::Method)
    }

    fn new(value: String, kind: JvmMemberKind) -> Result<Self, AbiError> {
        let valid = match kind {
            JvmMemberKind::Field => parse_field_type(value.as_bytes(), false) == Some(value.len()),
            JvmMemberKind::Method => valid_method_descriptor(&value),
        };
        if !valid {
            return Err(AbiError::InvalidDescriptor { value, kind });
        }
        Ok(Self { value, kind })
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn into_string(self) -> String {
        self.value
    }

    pub fn kind(&self) -> JvmMemberKind {
        self.kind
    }
}

impl Deref for JvmDescriptor {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for JvmDescriptor {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for JvmDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn valid_method_descriptor(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'(') {
        return false;
    }
    let mut i = 1;
    while bytes.get(i) != Some(&b')') {
        let Some(next) = parse_field_type(&bytes[i..], false) else {
            return false;
        };
        i += next;
        if i >= bytes.len() {
            return false;
        }
    }
    i += 1;
    parse_field_type(&bytes[i..], true).is_some_and(|n| i + n == bytes.len())
}

fn parse_field_type(bytes: &[u8], allow_void: bool) -> Option<usize> {
    match *bytes.first()? {
        b'V' if allow_void => Some(1),
        b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z' => Some(1),
        b'[' => parse_field_type(&bytes[1..], false).map(|n| n + 1),
        b'L' => {
            let end = bytes.iter().position(|b| *b == b';')?;
            let name = &bytes[1..end];
            (end > 1
                && !name.starts_with(b"/")
                && !name.ends_with(b"/")
                && !name.windows(2).any(|w| w == b"//")
                && !name.iter().any(|b| matches!(*b, b'.' | b';' | b'[' | 0)))
            .then_some(end + 1)
        }
        _ => None,
    }
}

/// Raw member access flags plus the member kind needed to interpret them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JvmMemberFlags {
    bits: u16,
    kind: JvmMemberKind,
}

impl JvmMemberFlags {
    pub const fn new(kind: JvmMemberKind, bits: u16) -> Self {
        Self { bits, kind }
    }

    pub const fn bits(self) -> u16 {
        self.bits
    }

    pub const fn kind(self) -> JvmMemberKind {
        self.kind
    }

    pub const fn is_synthetic(self) -> bool {
        self.bits & 0x1000 != 0
    }

    pub const fn is_bridge(self) -> bool {
        matches!(self.kind, JvmMemberKind::Method) && self.bits & 0x0040 != 0
    }

    pub const fn is_volatile(self) -> bool {
        matches!(self.kind, JvmMemberKind::Field) && self.bits & 0x0040 != 0
    }

    pub const fn is_varargs(self) -> bool {
        matches!(self.kind, JvmMemberKind::Method) && self.bits & 0x0080 != 0
    }

    pub const fn is_transient(self) -> bool {
        matches!(self.kind, JvmMemberKind::Field) && self.bits & 0x0080 != 0
    }
}

/// Raw class access flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JvmClassFlags(u16);

impl JvmClassFlags {
    pub const fn new(bits: u16) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn is_interface(self) -> bool {
        self.0 & 0x0200 != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbiError {
    InvalidInternalName(String),
    InvalidDescriptor { value: String, kind: JvmMemberKind },
}

impl fmt::Display for AbiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInternalName(name) => write!(f, "invalid JVM internal name `{name}`"),
            Self::InvalidDescriptor { value, kind } => {
                write!(f, "invalid JVM {kind:?} descriptor `{value}`")
            }
        }
    }
}

impl std::error::Error for AbiError {}

/// Failure of the deliberately small eager ScalaSignature reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScalaSignatureError;

impl fmt::Display for ScalaSignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("malformed or unsupported ScalaSignature")
    }
}

impl std::error::Error for ScalaSignatureError {}

/// A type recovered by the eager ScalaSignature subset reader.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PickledType {
    pub name: String,
    pub args: Vec<PickledType>,
    /// The type is a singleton (`O.type`) rather than the class named `O`.
    /// This distinction is present in the ScalaSignature's `SINGLEtpe` entry
    /// and must survive the compact classpath ABI so module members remain
    /// selectable after a separate compilation.
    pub singleton: bool,
}

impl PickledType {
    pub fn simple(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            args: Vec::new(),
            singleton: false,
        }
    }

    /// The eager classpath ABI's lossless spelling for a Scala intersection
    /// (`A with B`). The nsc pickle has a dedicated `REFINEDtpe` entry, while
    /// this compact ABI otherwise only carries a name and type arguments.
    /// Keeping the parents in `args` lets the typer reconstruct the actual
    /// intersection instead of silently widening an abstract bound to `Any`.
    pub fn intersection(parents: Vec<Self>) -> Self {
        Self {
            name: "&".into(),
            args: parents,
            singleton: false,
        }
    }
}

impl PartialEq<str> for PickledType {
    fn eq(&self, other: &str) -> bool {
        self.args.is_empty() && self.name == other
    }
}

impl PartialEq<&str> for PickledType {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl PartialEq<String> for PickledType {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}

/// A type parameter recovered by the eager ScalaSignature subset reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickledTypeParam {
    pub name: String,
    pub tparams: Vec<PickledTypeParam>,
}

impl PickledTypeParam {
    pub fn simple(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tparams: Vec::new(),
        }
    }
}

impl PartialEq<str> for PickledTypeParam {
    fn eq(&self, other: &str) -> bool {
        self.tparams.is_empty() && self.name == other
    }
}

impl PartialEq<&str> for PickledTypeParam {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl PartialEq<String> for PickledTypeParam {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}

/// A method, constructor, or val from the eager ScalaSignature subset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickledMethod {
    pub name: String,
    pub param_names: Vec<String>,
    pub param_types: Vec<PickledType>,
    /// Parameter counts in each source clause; the JVM descriptor is flat.
    pub clause_sizes: Vec<usize>,
    /// Raw pickle flags per value parameter, including `DEFAULTPARAM`.
    pub param_flags: Vec<u64>,
    pub ret: PickledType,
    pub tparams: Vec<PickledTypeParam>,
    pub is_val: bool,
    pub is_ctor: bool,
    pub is_implicit: bool,
    /// The pickle's `DEFERRED` bit. Interface access flags cannot distinguish
    /// a concrete Scala trait member from a declaration.
    pub is_deferred: bool,
    /// A `var` getter, derived from the ScalaSignature accessor/stability bits.
    pub is_mutable: bool,
}

/// A type member recovered from a ScalaSignature. The JVM has no entry for
/// `type T`, so classpath consumers carry the declaration alongside methods.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickledTypeMember {
    pub name: String,
    pub lower_bound: PickledType,
    pub upper_bound: PickledType,
    pub alias: Option<PickledType>,
    pub tparams: Vec<PickledTypeParam>,
}

/// A class or module class from a pickle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickledClass {
    pub name: String,
    pub is_module: bool,
    pub tparams: Vec<PickledTypeParam>,
    pub methods: Vec<PickledMethod>,
    pub type_members: Vec<PickledTypeMember>,
    pub extends_anyval: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedMethod {
    /// The full method access bits, including synthetic and bridge.
    pub flags: JvmMemberFlags,
    pub name: String,
    pub descriptor: JvmDescriptor,
    pub signature: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedField {
    pub flags: JvmMemberFlags,
    pub name: String,
    pub descriptor: JvmDescriptor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedClass {
    pub internal_name: JvmInternalName,
    pub flags: JvmClassFlags,
    pub is_module: bool,
    pub methods: Vec<LoadedMethod>,
    pub fields: Vec<LoadedField>,
    pub pickle: Option<PickledClass>,
    pub super_name: Option<JvmInternalName>,
    pub interfaces: Vec<JvmInternalName>,
}

impl LoadedClass {
    /// Every method retained by the classfile ABI reader.
    pub fn all_methods(&self) -> impl Iterator<Item = &LoadedMethod> {
        self.methods.iter()
    }

    /// Methods suitable for ordinary source member lookup.
    ///
    /// This is a view, not destructive filtering: bridge generation can still
    /// query [`Self::all_methods`] and observe synthetic bridge methods.
    pub fn scala_visible_methods(&self) -> impl Iterator<Item = &LoadedMethod> {
        self.methods
            .iter()
            .filter(|m| !m.flags.is_synthetic() && !m.flags.is_bridge())
    }

    pub fn bridge_methods(&self) -> impl Iterator<Item = &LoadedMethod> {
        self.methods.iter().filter(|m| m.flags.is_bridge())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_validate_and_roundtrip_without_normalization() {
        for descriptor in ["I", "[Ljava/lang/String;", "[[Z"] {
            let parsed = JvmDescriptor::field(descriptor).unwrap();
            assert_eq!(parsed.as_str(), descriptor);
            assert_eq!(parsed.kind(), JvmMemberKind::Field);
        }
        for descriptor in ["()V", "(I[Ljava/lang/String;)Ljava/lang/Object;"] {
            let parsed = JvmDescriptor::method(descriptor).unwrap();
            assert_eq!(parsed.as_str(), descriptor);
            assert_eq!(parsed.kind(), JvmMemberKind::Method);
        }
        assert!(JvmDescriptor::field("V").is_err());
        assert!(JvmDescriptor::method("(I").is_err());
        assert!(JvmDescriptor::method("I").is_err());
    }

    #[test]
    fn shared_flag_bits_keep_member_specific_capabilities() {
        let method = JvmMemberFlags::new(JvmMemberKind::Method, 0x0040 | 0x0080 | 0x1000);
        assert!(method.is_bridge());
        assert!(method.is_varargs());
        assert!(method.is_synthetic());
        assert!(!method.is_volatile());
        assert!(!method.is_transient());

        let field = JvmMemberFlags::new(JvmMemberKind::Field, 0x0040 | 0x0080 | 0x1000);
        assert!(field.is_volatile());
        assert!(field.is_transient());
        assert!(field.is_synthetic());
        assert!(!field.is_bridge());
        assert!(!field.is_varargs());
    }

    #[test]
    fn internal_names_roundtrip_without_becoming_source_names() {
        let name = JvmInternalName::new("example/Outer$Inner").unwrap();
        assert_eq!(name.as_str(), "example/Outer$Inner");
        assert!(JvmInternalName::new("example.Outer").is_err());
        assert!(JvmInternalName::new("example//Outer").is_err());
    }
}
