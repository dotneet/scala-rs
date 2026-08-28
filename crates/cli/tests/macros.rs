//! Def macros (`def f = macro Impl.method`). See `docs/macros.md`.
//!
//! Phase 1 covers the definition side only: the parser accepts the syntax, the
//! typer resolves and records the binding to the implementation, and the
//! backend emits no method for the macro def. Expansion is not implemented, so
//! every call site must be diagnosed rather than silently accepted.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-macros-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn compile(name: &str, out: &Path) -> std::process::Output {
    Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile")
}

fn diagnostics(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}

/// Compiling `name` must fail with `needle` in the diagnostics.
fn compile_fails(name: &str, needle: &str) {
    let out = tmp_dir(name);
    let output = compile(name, &out);
    let err = diagnostics(&output);
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail, got: {err}"
    );
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// A macro def that is never called compiles and runs.
#[test]
fn macro_def_compiles_and_is_not_emitted() {
    let out = tmp_dir("macro_def");
    let output = compile("macro_def", &out);
    assert!(
        output.status.success(),
        "compile macro_def failed: {}",
        diagnostics(&output)
    );

    // nsc emits no JVM method for a macro def, which is why macros cannot be
    // called from Java. The enclosing module class is still emitted.
    assert!(
        out.join("Sugar$.class").is_file(),
        "Sugar$.class missing in {}",
        out.display()
    );
    let bytes = fs::read(out.join("Sugar$.class")).expect("read Sugar$.class");
    let names = utf8_constants(&bytes);
    assert!(
        !names.iter().any(|n| n == "f"),
        "macro def `f` was emitted as a method: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "g"),
        "macro def `g` was emitted as a method: {names:?}"
    );

    if java_available() {
        let run = Command::new("java")
            .args(["-cp", out.to_str().unwrap(), "Main"])
            .output()
            .expect("java");
        assert!(
            run.status.success(),
            "java Main failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        let expected =
            fs::read_to_string(fixtures_dir().join("expected").join("macro_def.txt")).unwrap();
        assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
    }
    let _ = fs::remove_dir_all(&out);
}

/// Calling a macro is diagnosed. Silently accepting it would emit a call to a
/// method the class file does not contain.
#[test]
fn macro_call_is_diagnosed() {
    compile_fails("macro_call_bad", "macro expansion is not implemented");
}

#[test]
fn macro_def_without_result_type_is_error() {
    compile_fails(
        "macro_no_result_type_bad",
        "must have an explicitly specified result type",
    );
}

#[test]
fn macro_impl_without_context_is_error() {
    compile_fails(
        "macro_impl_shape_bad",
        "must take scala.reflect.macros.blackbox.Context",
    );
}

#[test]
fn unresolved_macro_impl_is_error() {
    compile_fails("macro_impl_missing_bad", "macro implementation not found");
}

#[test]
fn whitebox_macro_is_rejected() {
    compile_fails("macro_whitebox_bad", "whitebox macros are not implemented");
}

/// Read the UTF8 constant pool entries of a class file.
///
/// Enough to assert a method name is absent without shelling out to `javap`.
fn utf8_constants(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if bytes.len() < 10 || bytes[0..4] != [0xCA, 0xFE, 0xBA, 0xBE] {
        return out;
    }
    let count = u16::from_be_bytes([bytes[8], bytes[9]]) as usize;
    let mut i = 10;
    let mut n = 1;
    while n < count && i < bytes.len() {
        let tag = bytes[i];
        i += 1;
        match tag {
            1 => {
                if i + 2 > bytes.len() {
                    break;
                }
                let len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
                i += 2;
                if i + len > bytes.len() {
                    break;
                }
                out.push(String::from_utf8_lossy(&bytes[i..i + len]).into_owned());
                i += len;
            }
            // Constants that occupy two pool slots.
            5 | 6 => {
                i += 8;
                n += 1;
            }
            7 | 8 | 16 | 19 | 20 => i += 2,
            15 => i += 3,
            3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => i += 4,
            _ => break,
        }
        n += 1;
    }
    out
}
