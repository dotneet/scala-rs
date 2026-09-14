//! Just enough JVM classfile parsing to reach a class's `ScalaSignature`.
//!
//! The backend has a fuller classfile reader (`load.rs`); this is the shared
//! substrate both it and the pickle reader need, so the constant-pool walk is
//! written once.

pub struct Cp {
    pub tags: Vec<u8>,
    pub data: Vec<Vec<u8>>,
}

impl Cp {
    pub fn utf8(&self, i: u16) -> Option<String> {
        let i = i as usize;
        if i == 0 || i >= self.tags.len() {
            return None;
        }
        if self.tags[i] != 1 {
            return None;
        }
        modified_utf8_to_string(&self.data[i])
    }

    pub fn class_name(&self, i: u16) -> Option<String> {
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

pub fn modified_utf8_to_string(b: &[u8]) -> Option<String> {
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

pub struct Cursor<'a> {
    pub b: &'a [u8],
    pub i: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Cursor { b, i: 0 }
    }
    pub fn u1(&mut self) -> Option<u8> {
        if self.i >= self.b.len() {
            return None;
        }
        let v = self.b[self.i];
        self.i += 1;
        Some(v)
    }
    pub fn u2(&mut self) -> Option<u16> {
        let hi = self.u1()? as u16;
        let lo = self.u1()? as u16;
        Some((hi << 8) | lo)
    }
    pub fn u4(&mut self) -> Option<u32> {
        let a = self.u2()? as u32;
        let b = self.u2()? as u32;
        Some((a << 16) | b)
    }
    pub fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.i + n > self.b.len() {
            return None;
        }
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        Some(s)
    }
}

pub fn skip_attrs(c: &mut Cursor) -> Option<()> {
    let n = c.u2()? as usize;
    for _ in 0..n {
        let _ = c.u2()?;
        let len = c.u4()? as usize;
        let _ = c.bytes(len)?;
    }
    Some(())
}

/// Decoded pickle bytes of a classfile's `ScalaSignature` / `ScalaLongSignature`.
///
/// The whole pickle is kept, so [`crate::read::read_pickle`] can parse all of
/// it. Handles the `ScalaLongSignature` form (an array of strings, used for
/// classes whose pickle exceeds the 64K constant-pool string limit).
pub fn scala_signature_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    scala_signature_bytes_result(bytes).ok().flatten()
}

/// Result-returning form of [`scala_signature_bytes`].
///
/// `Ok(None)` means that no Scala signature is present. An invalid classfile
/// or invalid annotation is an error, so an ABI loader need not silently
/// downgrade a Scala class to a Java class.
pub fn scala_signature_bytes_result(bytes: &[u8]) -> Result<Option<Vec<u8>>, ClassfileError> {
    let mut c = Cursor::new(bytes);
    if c.u4().ok_or(ClassfileError::Truncated("magic"))? != 0xCAFEBABE {
        return Err(ClassfileError::BadMagic);
    }
    let _minor = c.u2().ok_or(ClassfileError::Truncated("version"))?;
    let _major = c.u2().ok_or(ClassfileError::Truncated("version"))?;
    let cp = parse_cp(&mut c).ok_or(ClassfileError::Malformed("constant pool"))?;
    let _access = c.u2().ok_or(ClassfileError::Truncated("class header"))?;
    let _this = c.u2().ok_or(ClassfileError::Truncated("class header"))?;
    let _super = c.u2().ok_or(ClassfileError::Truncated("class header"))?;
    let niface = c.u2().ok_or(ClassfileError::Truncated("interfaces"))? as usize;
    for _ in 0..niface {
        let _ = c.u2().ok_or(ClassfileError::Truncated("interfaces"))?;
    }
    for _ in 0..(c.u2().ok_or(ClassfileError::Truncated("fields"))? as usize) {
        let _ = c.u2().ok_or(ClassfileError::Truncated("field"))?;
        let _ = c.u2().ok_or(ClassfileError::Truncated("field"))?;
        let _ = c.u2().ok_or(ClassfileError::Truncated("field"))?;
        skip_attrs(&mut c).ok_or(ClassfileError::Truncated("field attributes"))?;
    }
    for _ in 0..(c.u2().ok_or(ClassfileError::Truncated("methods"))? as usize) {
        let _ = c.u2().ok_or(ClassfileError::Truncated("method"))?;
        let _ = c.u2().ok_or(ClassfileError::Truncated("method"))?;
        let _ = c.u2().ok_or(ClassfileError::Truncated("method"))?;
        skip_attrs(&mut c).ok_or(ClassfileError::Truncated("method attributes"))?;
    }
    scala_signature_bytes_from_class_attributes(&mut c, &cp)
}

/// Extract a Scala signature from the class attributes at the cursor's current
/// position, reusing a constant pool that the caller has already parsed.
///
/// This is public for the backend classfile loader; most callers should use
/// [`scala_signature_bytes_result`].
#[doc(hidden)]
pub fn scala_signature_bytes_from_class_attributes(
    c: &mut Cursor<'_>,
    cp: &Cp,
) -> Result<Option<Vec<u8>>, ClassfileError> {
    let nattrs = c
        .u2()
        .ok_or(ClassfileError::Truncated("class attributes"))? as usize;
    for _ in 0..nattrs {
        let name_i = c.u2().ok_or(ClassfileError::Truncated("class attribute"))?;
        let len = c.u4().ok_or(ClassfileError::Truncated("class attribute"))? as usize;
        let body = c
            .bytes(len)
            .ok_or(ClassfileError::Truncated("class attribute body"))?;
        if cp.utf8(name_i).as_deref() != Some("RuntimeVisibleAnnotations") {
            continue;
        }
        if let Some(s) = signature_string(body, cp)
            .map_err(|()| ClassfileError::Malformed("RuntimeVisibleAnnotations"))?
        {
            return Ok(Some(crate::codec::decode_annotation_string(&s)));
        }
    }
    Ok(None)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassfileError {
    BadMagic,
    Truncated(&'static str),
    Malformed(&'static str),
}

impl std::fmt::Display for ClassfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic => f.write_str("not a classfile (bad magic)"),
            Self::Truncated(part) => write!(f, "truncated classfile {part}"),
            Self::Malformed(part) => write!(f, "malformed classfile {part}"),
        }
    }
}

impl std::error::Error for ClassfileError {}

pub fn parse_cp(c: &mut Cursor) -> Option<Cp> {
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
fn signature_string(body: &[u8], cp: &Cp) -> Result<Option<String>, ()> {
    let mut c = Cursor::new(body);
    let nann = c.u2().ok_or(())? as usize;
    for _ in 0..nann {
        let type_i = c.u2().ok_or(())?;
        let ty = cp.utf8(type_i).unwrap_or_default();
        let is_sig = ty.contains("ScalaSignature") || ty.contains("ScalaLongSignature");
        let npairs = c.u2().ok_or(())? as usize;
        let mut found: Option<String> = None;
        for _ in 0..npairs {
            let name_i = c.u2().ok_or(())?;
            let name = cp.utf8(name_i).unwrap_or_default();
            let mut sink = String::new();
            read_element_value(&mut c, cp, &mut sink).ok_or(())?;
            if is_sig && name == "bytes" {
                found = Some(sink);
            }
        }
        if let Some(s) = found {
            return Ok(Some(s));
        }
    }
    Ok(None)
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

#[cfg(test)]
mod result_tests {
    use super::*;

    #[test]
    fn result_api_distinguishes_absent_signature_from_malformed_classfile() {
        let mut bare = Vec::new();
        bare.extend_from_slice(&0xCAFEBABEu32.to_be_bytes());
        bare.extend_from_slice(&0u16.to_be_bytes());
        bare.extend_from_slice(&52u16.to_be_bytes());
        bare.extend_from_slice(&1u16.to_be_bytes()); // empty constant pool
        bare.extend_from_slice(&0u16.to_be_bytes()); // access
        bare.extend_from_slice(&0u16.to_be_bytes()); // this
        bare.extend_from_slice(&0u16.to_be_bytes()); // super
        bare.extend_from_slice(&0u16.to_be_bytes()); // interfaces
        bare.extend_from_slice(&0u16.to_be_bytes()); // fields
        bare.extend_from_slice(&0u16.to_be_bytes()); // methods
        bare.extend_from_slice(&0u16.to_be_bytes()); // attributes

        assert_eq!(scala_signature_bytes_result(&bare), Ok(None));
        assert_eq!(
            scala_signature_bytes_result(b"not a class"),
            Err(ClassfileError::BadMagic)
        );
        assert!(scala_signature_bytes_result(&bare[..8]).is_err());
    }
}
