//! Scala's `NameTransformer`: the encoding that makes operator method names
//! legal JVM identifiers (`++` is `$plus$plus`).
//!
//! Shared, because both ends of the pipeline need the same table: the backend
//! encodes names it emits, and the typer has to encode a member name from a
//! pickle before it can find that method in a classfile.

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
        match c {
            '~' => out.push_str("$tilde"),
            '=' => out.push_str("$eq"),
            '<' => out.push_str("$less"),
            '>' => out.push_str("$greater"),
            '!' => out.push_str("$bang"),
            '#' => out.push_str("$hash"),
            '%' => out.push_str("$percent"),
            '^' => out.push_str("$up"),
            '&' => out.push_str("$amp"),
            '|' => out.push_str("$bar"),
            '*' => out.push_str("$times"),
            '/' => out.push_str("$div"),
            '+' => out.push_str("$plus"),
            '-' => out.push_str("$minus"),
            ':' => out.push_str("$colon"),
            '?' => out.push_str("$qmark"),
            '@' => out.push_str("$at"),
            _ => out.push(c),
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
        if let Some(r) = rest.strip_prefix("$tilde") {
            out.push('~');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$eq") {
            out.push('=');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$less") {
            out.push('<');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$greater") {
            out.push('>');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$bang") {
            out.push('!');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$hash") {
            out.push('#');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$percent") {
            out.push('%');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$up") {
            out.push('^');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$amp") {
            out.push('&');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$bar") {
            out.push('|');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$times") {
            out.push('*');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$div") {
            out.push('/');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$plus") {
            out.push('+');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$minus") {
            out.push('-');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$colon") {
            out.push(':');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$qmark") {
            out.push('?');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("$at") {
            out.push('@');
            rest = r;
        } else {
            let ch = rest.chars().next().unwrap();
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}
