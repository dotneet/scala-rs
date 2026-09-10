//! Scala's `NameTransformer`: the encoding that makes operator method names
//! legal JVM identifiers (`++` is `$plus$plus`).
//!
//! Shared, because both ends of the pipeline need the same table: the backend
//! encodes names it emits, and the typer has to encode a member name from a
//! pickle before it can find that method in a classfile.

#[path = "java_identifier_parts.rs"]
mod java_identifier_parts;

const OPERATORS: &[(char, &str)] = &[
    ('~', "$tilde"),
    ('=', "$eq"),
    ('<', "$less"),
    ('>', "$greater"),
    ('!', "$bang"),
    ('#', "$hash"),
    ('%', "$percent"),
    ('^', "$up"),
    ('&', "$amp"),
    ('|', "$bar"),
    ('*', "$times"),
    ('/', "$div"),
    ('+', "$plus"),
    ('-', "$minus"),
    (':', "$colon"),
    ('\\', "$bslash"),
    ('?', "$qmark"),
    ('@', "$at"),
];

fn is_java_identifier_part(c: char) -> bool {
    let Ok(c) = u16::try_from(c as u32) else {
        // nsc examines UTF-16 chars, so even supplementary letters arrive as
        // two non-identifier surrogate code units and are escaped separately.
        return false;
    };
    let ranges = java_identifier_parts::JAVA_IDENTIFIER_PARTS;
    let i = ranges.partition_point(|&(_, hi)| hi < c);
    ranges.get(i).is_some_and(|&(lo, _)| lo <= c)
}

/// Scala NameTransformer encoding so operator methods are legal JVM names.
/// `<init>` / `<clinit>` are left alone. `->` becomes `$minus$greater`.
pub fn encode_method_name(name: &str) -> String {
    if name == "<init>" || name == "<clinit>" {
        return name.to_string();
    }
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return name.to_string();
    }
    let mut out = String::new();
    for c in name.chars() {
        if let Some((_, code)) = OPERATORS.iter().find(|&&(ch, _)| ch == c) {
            out.push_str(code);
        } else if is_java_identifier_part(c) {
            out.push(c);
        } else {
            use std::fmt::Write;
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                write!(out, "$u{unit:04X}").unwrap();
            }
        }
    }
    out
}

/// Inverse of [`encode_method_name`] for names recovered from classfiles.
pub fn decode_method_name(name: &str) -> String {
    if !name.contains('$') {
        return name.to_string();
    }
    let mut out = String::new();
    let mut rest = name;
    while !rest.is_empty() {
        if let Some((c, n)) = decode_unicode_escape(rest).or_else(|| {
            OPERATORS
                .iter()
                .find_map(|&(c, code)| rest.starts_with(code).then_some((c, code.len())))
        }) {
            out.push(c);
            rest = &rest[n..];
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// A dollar introduced by nesting, excluding dollars in encoded symbols.
/// `Outer$$plus$plus` has its boundary before `$plus$plus`, while `$plus$plus`
/// is a single top-level class name. Callers remove a module suffix first.
pub fn last_nesting_separator(name: &str) -> Option<usize> {
    let mut last = None;
    let mut offset = 0;
    while offset < name.len() {
        let rest = &name[offset..];
        if let Some((_, n)) = decode_unicode_escape(rest).or_else(|| {
            OPERATORS
                .iter()
                .find_map(|&(c, code)| rest.starts_with(code).then_some((c, code.len())))
        }) {
            offset += n;
        } else {
            let c = rest.chars().next().unwrap();
            if c == '$' {
                last = Some(offset);
            }
            offset += c.len_utf8();
        }
    }
    last
}

/// Convert JVM nesting separators to pickle owner separators, preserving
/// encoded operator and Unicode chunks. The caller removes any module suffix.
pub fn nested_to_dotted(name: &str) -> String {
    let mut out = name.to_string();
    let mut end = out.len();
    while let Some(i) = last_nesting_separator(&out[..end]) {
        out.replace_range(i..i + 1, ".");
        end = i;
    }
    out
}

fn decode_unicode_escape(s: &str) -> Option<(char, usize)> {
    fn unit(s: &str) -> Option<u16> {
        let hex = s.strip_prefix("$u")?.get(..4)?;
        if !hex.starts_with(|c: char| c.is_ascii_digit() || ('A'..='F').contains(&c)) {
            return None;
        }
        u16::from_str_radix(hex, 16).ok()
    }
    let first = unit(s)?;
    if let Some(c) = char::from_u32(first as u32) {
        return Some((c, 6));
    }
    let second = unit(s.get(6..)?)?;
    if (0xD800..=0xDBFF).contains(&first) && (0xDC00..=0xDFFF).contains(&second) {
        let c = 0x10000 + (((first as u32 - 0xD800) << 10) | (second as u32 - 0xDC00));
        Some((char::from_u32(c)?, 12))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path, process::Command};

    #[test]
    fn names_match_released_scala_for_bmp_and_escape_sequences() {
        let jar = Path::new("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
        assert!(jar.is_file(), "Scala 2.13.16 oracle jar is required");
        let root = std::env::temp_dir().join(format!(
            "scala-rs-name-oracle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/tools/NameTransformerOracle.java");
        let built = Command::new("javac")
            .arg("-cp")
            .arg(jar)
            .arg("-d")
            .arg(&root)
            .arg(source)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let extra = [
            "unary_-",
            "a.b",
            "space name",
            "a\\b",
            "λ",
            "😀",
            "𐐀",
            "$uD83D$uDE00",
            "$u0020",
            "$u002e",
            "$uZZZZ",
            "$uabcd",
            "$uABCD",
            "$uD800",
            "$uDC00",
            "$plus",
            "$bslash",
            "$_setter_$value_$eq",
        ];
        let result = Command::new("java")
            .args(["-Xverify:all", "-cp"])
            .arg(format!("{}:{}", root.display(), jar.display()))
            .arg("NameTransformerOracle")
            .args(extra)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let reference = String::from_utf8(result.stdout).unwrap();
        let mut inputs: Vec<String> = (0..=0xFFFF)
            .filter_map(char::from_u32)
            .map(|c| c.to_string())
            .collect();
        inputs.extend(extra.iter().map(|s| s.to_string()));
        let rows: Vec<_> = reference.lines().collect();
        assert_eq!(rows.len(), inputs.len());
        fn hex(s: &str) -> String {
            s.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
        }
        for (input, row) in inputs.iter().zip(rows) {
            // Rust strings cannot carry an isolated UTF-16 surrogate. Such a
            // malformed escape stays literal rather than becoming a new name.
            let (encoded, decoded) = row.split_once(':').unwrap();
            assert_eq!(hex(&encode_method_name(input)), encoded, "encode {input:?}");
            if !matches!(input.as_str(), "$uD800" | "$uDC00") {
                assert_eq!(hex(&decode_method_name(input)), decoded, "decode {input:?}");
            } else {
                assert_eq!(decode_method_name(input), *input);
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}
