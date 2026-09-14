//! A small, allocation-light scanner for JVM field and method descriptors.
//!
//! This module deliberately knows nothing about the compiler's `Type` or
//! `SymbolTable`.  The same raw descriptor syntax is used by Scala pickles,
//! Scala classfiles, and Java classfiles; those consumers attach different
//! meanings to a few object names and belong in the parent module.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Primitive {
    Void,
    Boolean,
    Byte,
    Short,
    Char,
    Int,
    Long,
    Float,
    Double,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ObjectName<'a>(&'a str);

impl<'a> ObjectName<'a> {
    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RawType<'a> {
    Primitive(Primitive),
    Array(Box<RawType<'a>>),
    Object(ObjectName<'a>),
}

/// One field descriptor and the exact source slice it consumed.
///
/// Keeping the slice lets callers compare JVM parameter descriptors without
/// allocating one `String` per parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Field<'a> {
    pub(crate) ty: RawType<'a>,
    pub(crate) source: &'a str,
    pub(crate) len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Method<'a> {
    pub(crate) params: Vec<Field<'a>>,
    pub(crate) ret: Field<'a>,
}

/// Scan one JVM field descriptor from the start of `input`.
///
/// Descriptor input comes from classfiles and is expected to be valid.  The
/// object branch intentionally accepts a missing trailing `;`, matching the
/// old classpath reader's lenient fallback for malformed descriptors.
pub(crate) fn field(input: &str) -> Option<Field<'_>> {
    let first = *input.as_bytes().first()?;
    let (ty, len) = match first {
        b'V' => (RawType::Primitive(Primitive::Void), 1),
        b'Z' => (RawType::Primitive(Primitive::Boolean), 1),
        b'B' => (RawType::Primitive(Primitive::Byte), 1),
        b'S' => (RawType::Primitive(Primitive::Short), 1),
        b'C' => (RawType::Primitive(Primitive::Char), 1),
        b'I' => (RawType::Primitive(Primitive::Int), 1),
        b'J' => (RawType::Primitive(Primitive::Long), 1),
        b'F' => (RawType::Primitive(Primitive::Float), 1),
        b'D' => (RawType::Primitive(Primitive::Double), 1),
        b'[' => {
            let inner = field(&input[1..])?;
            (RawType::Array(Box::new(inner.ty)), inner.len + 1)
        }
        b'L' => {
            let end = input.find(';').unwrap_or(input.len());
            (
                RawType::Object(ObjectName(&input[1..end])),
                if end < input.len() { end + 1 } else { end },
            )
        }
        _ => return None,
    };
    Some(Field {
        ty,
        source: &input[..len],
        len,
    })
}

/// Scan a complete method descriptor, retaining each parameter's source
/// slice as well as its raw token tree.
pub(crate) fn method(input: &str) -> Option<Method<'_>> {
    if input.as_bytes().first() != Some(&b'(') {
        return None;
    }
    let mut offset = 1;
    let mut params = Vec::new();
    while offset < input.len() && input.as_bytes()[offset] != b')' {
        let param = field(&input[offset..])?;
        offset += param.len;
        params.push(param);
    }
    if input.as_bytes().get(offset) != Some(&b')') {
        return None;
    }
    let ret = field(&input[offset + 1..])?;
    Some(Method { params, ret })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_primitives_arrays_and_nested_objects() {
        let parsed = method("(ZBCSIJFD[[Ljava/lang/String;Lpkg/Outer$Inner;)V").unwrap();
        assert_eq!(parsed.params.len(), 10);
        assert_eq!(parsed.params[0].source, "Z");
        assert_eq!(parsed.params[8].source, "[[Ljava/lang/String;");
        assert_eq!(parsed.params[9].source, "Lpkg/Outer$Inner;");
        assert_eq!(parsed.ret.source, "V");
        assert_eq!(
            parsed.params[9].ty,
            RawType::Object(ObjectName("pkg/Outer$Inner"))
        );
    }

    #[test]
    fn scanner_rejects_non_descriptors_without_consuming_input() {
        assert!(field("").is_none());
        assert!(field("Q").is_none());
        assert!(method("I").is_none());
        assert!(method("(I").is_none());
    }
}
