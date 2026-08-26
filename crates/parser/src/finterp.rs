//! Scala 2.13 `f` interpolator format specs (not a macro).
//!
//! Assembles the `String.format` pattern and the per-hole conversions nsc's
//! `FormatInterpolator` would check. Date/time (`%t`/`%T`), argument index
//! (`%1$s`), and relative (`%<`) are diagnosed rather than faked.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FSpec {
    pub flags: String,
    pub width: Option<u32>,
    pub precision: Option<u32>,
    pub conv: char,
}

impl FSpec {
    pub fn pattern(&self) -> String {
        let mut s = String::from("%");
        s.push_str(&self.flags);
        if let Some(w) = self.width {
            s.push_str(&w.to_string());
        }
        if let Some(p) = self.precision {
            s.push('.');
            s.push_str(&p.to_string());
        }
        s.push(self.conv);
        s
    }

    /// Java `Formatter` conversion kind used for type checks.
    pub fn kind(&self) -> FConvKind {
        match self.conv {
            'b' | 'B' | 'h' | 'H' | 's' | 'S' => FConvKind::General,
            'c' | 'C' => FConvKind::Character,
            'd' | 'o' | 'x' | 'X' => FConvKind::Integral,
            'e' | 'E' | 'f' | 'g' | 'G' | 'a' | 'A' => FConvKind::Floating,
            _ => FConvKind::Unsupported,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FConvKind {
    General,
    Character,
    Integral,
    Floating,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FInterpError {
    Unsupported(String),
    Message(String),
}

/// Build the `String.format` pattern and one spec per interpolator hole.
pub fn assemble_f(parts: &[String], nargs: usize) -> Result<(String, Vec<FSpec>), FInterpError> {
    if parts.is_empty() {
        return Ok((String::new(), Vec::new()));
    }
    let mut out = String::new();
    out.push_str(&check_literal(&parts[0])?);
    let mut specs = Vec::with_capacity(nargs);
    for i in 0..nargs {
        let rest = parts.get(i + 1).map(|s| s.as_str()).unwrap_or("");
        let (spec, after) = leading_spec(rest)?;
        let spec = spec.unwrap_or(FSpec {
            flags: String::new(),
            width: None,
            precision: None,
            conv: 's',
        });
        if spec.kind() == FConvKind::Unsupported {
            return Err(FInterpError::Unsupported(format!(
                "f interpolator: unsupported conversion %{}",
                spec.conv
            )));
        }
        if spec.precision.is_some()
            && matches!(
                spec.conv,
                'd' | 'o' | 'x' | 'X' | 'c' | 'C' | 'b' | 'B' | 'h' | 'H'
            )
        {
            return Err(FInterpError::Message(format!(
                "f interpolator: precision not allowed for %{}",
                spec.conv
            )));
        }
        out.push_str(&spec.pattern());
        out.push_str(&check_literal(after)?);
        specs.push(spec);
    }
    if parts.len() > nargs + 1 {
        for p in &parts[nargs + 1..] {
            out.push_str(&check_literal(p)?);
        }
    }
    Ok((out, specs))
}

fn check_literal(s: &str) -> Result<String, FInterpError> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = String::new();
    while i < b.len() {
        if b[i] == b'%' {
            if i + 1 < b.len() && (b[i + 1] == b'%' || b[i + 1] == b'n' || b[i + 1] == b'N') {
                out.push('%');
                out.push(b[i + 1] as char);
                i += 2;
                continue;
            }
            return Err(FInterpError::Message(
                "f interpolator: stray % in string part (use %% for a literal percent)".into(),
            ));
        }
        out.push(b[i] as char);
        i += 1;
    }
    Ok(out)
}

fn leading_spec(part: &str) -> Result<(Option<FSpec>, &str), FInterpError> {
    let b = part.as_bytes();
    if b.first() != Some(&b'%') {
        return Ok((None, part));
    }
    if b.len() >= 2 && (b[1] == b'%' || b[1] == b'n' || b[1] == b'N') {
        return Ok((None, part));
    }
    let mut i = 1;
    if i < b.len() && b[i] == b'<' {
        return Err(FInterpError::Unsupported(
            "f interpolator: relative argument index %< is not supported".into(),
        ));
    }
    let start_digits = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'$' {
        return Err(FInterpError::Unsupported(
            "f interpolator: explicit argument index is not supported".into(),
        ));
    }
    i = start_digits;
    let mut flags = String::new();
    while i < b.len() {
        let c = b[i] as char;
        if "-#+ 0,(".contains(c) {
            flags.push(c);
            i += 1;
        } else {
            break;
        }
    }
    let mut width = None;
    if i < b.len() && b[i].is_ascii_digit() {
        let mut n = 0u32;
        while i < b.len() && b[i].is_ascii_digit() {
            n = n.saturating_mul(10).saturating_add((b[i] - b'0') as u32);
            i += 1;
        }
        width = Some(n);
    }
    let mut precision = None;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        if i >= b.len() || !b[i].is_ascii_digit() {
            return Err(FInterpError::Message(
                "f interpolator: expected precision digits after '.'".into(),
            ));
        }
        let mut n = 0u32;
        while i < b.len() && b[i].is_ascii_digit() {
            n = n.saturating_mul(10).saturating_add((b[i] - b'0') as u32);
            i += 1;
        }
        precision = Some(n);
    }
    if i >= b.len() {
        return Err(FInterpError::Message(
            "f interpolator: truncated conversion".into(),
        ));
    }
    let conv = b[i] as char;
    i += 1;
    if conv == 't' || conv == 'T' {
        return Err(FInterpError::Unsupported(format!(
            "f interpolator: date/time conversion %{conv} is not supported"
        )));
    }
    if !conv.is_ascii_alphabetic() {
        return Err(FInterpError::Message(format!(
            "f interpolator: illegal conversion %{conv}"
        )));
    }
    Ok((
        Some(FSpec {
            flags,
            width,
            precision,
            conv,
        }),
        &part[i..],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padded_int() {
        let (fmt, specs) = assemble_f(&["".into(), "%02d".into()], 1).unwrap();
        assert_eq!(fmt, "%02d");
        assert_eq!(specs[0].conv, 'd');
        assert_eq!(specs[0].width, Some(2));
        assert!(specs[0].flags.contains('0'));
    }

    #[test]
    fn default_string() {
        let (fmt, specs) = assemble_f(&["hi ".into(), "".into()], 1).unwrap();
        assert_eq!(fmt, "hi %s");
        assert_eq!(specs[0].conv, 's');
    }

    #[test]
    fn float_precision() {
        let (fmt, _) = assemble_f(&["".into(), "%.2f".into()], 1).unwrap();
        assert_eq!(fmt, "%.2f");
    }

    #[test]
    fn date_is_unsupported() {
        let e = assemble_f(&["".into(), "%tY".into()], 1).unwrap_err();
        match e {
            FInterpError::Unsupported(s) => assert!(s.contains("date/time"), "{s}"),
            other => panic!("{other:?}"),
        }
    }
}
