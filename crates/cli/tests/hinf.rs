//! Hard type-inference roots (`agent/hardinfer`):
//!
//! * GADT-style refinement of a method type parameter inside a case
//!   (`hinf_gadt`): nsc's `inferTypedPattern` / `inferConstructorInstance`
//!   bounds, kept in `SymbolTable::gadt_bounds` for the case.
//! * A nested polymorphic argument solved with the outer call through the
//!   lenient prototype (`hinf_nested`): `proto_arg_type` hands the argument
//!   `Builder[_, CC[B]]`, nsc's `WildcardType` for the unsettled variable.
//! * A lambda body under an undetermined result (`hinf_lambda`): typed with
//!   no expected type, as nsc's `typedFunction` does, so `if (c) 1L else 0`
//!   takes the weak-conformance lub instead of boxing to `AnyVal`.
//! * A retracted `Nothing` (`hinf_nothing`): nsc's `adjustTypeArgs`, kept
//!   undetermined for an invariant or contravariant occurrence in the result
//!   and closed at a definition.
//!
//! Every positive fixture is compiled and run by both scala-rs and scalac
//! 2.13.16 against the same expected output; every negative fixture must be
//! rejected by both at the same lines.
//!
//! Fixture prefix: `hinf_`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-hinf-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn compile(name: &str, out: &Path, jar: &Path, scalac_path: Option<&Path>) -> std::process::Output {
    let src = fixtures_dir().join(format!("{name}.scala"));
    match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(&src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(&src)
            .args(["-d", out.to_str().unwrap()])
            .args(["--scala-library", jar.to_str().unwrap()])
            .output()
            .expect("run scala-rs compile"),
    }
}

fn run_java(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let o = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        o.status.success(),
        "java Main failed:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// Compile `name` (scala-rs, or scalac when given), run it under
/// `-Xverify:all`, and compare with the expected output.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, &out, &jar, scalac_path);
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// The source lines scala-rs (`--> path:LINE:COL`) or scalac
/// (`path:LINE: error:`) report an error at.
fn error_lines(name: &str, text: &str, scalac_output: bool) -> BTreeSet<u32> {
    let file = format!("{name}.scala");
    let mut lines = BTreeSet::new();
    // scala-rs prints the level on its own line (`error: …` / `warning: …`)
    // and the location on the `-->` line after it; only errors count.
    let mut in_error = false;
    for l in text.lines() {
        if scalac_output {
            if let Some(rest) = l.split_once(&format!("{file}:")) {
                if let Some((n, tail)) = rest.1.split_once(':') {
                    if tail.trim_start().starts_with("error") {
                        lines.insert(n.parse::<u32>().unwrap());
                    }
                }
            }
        } else if l.starts_with("error") || l.starts_with("warning") {
            in_error = l.starts_with("error");
        } else if let Some(rest) = l.trim_start().strip_prefix("--> ") {
            if let Some(after) = rest.split_once(&format!("{file}:")) {
                if in_error {
                    let n = after.1.split(':').next().unwrap();
                    lines.insert(n.parse::<u32>().unwrap());
                }
            }
        }
    }
    lines
}

/// Compile a negative fixture with scala-rs and, when present, scalac; both
/// must fail, and scala-rs must report at exactly the lines scalac does.
fn check_rejects(name: &str, expected_lines: &[u32]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let want: BTreeSet<u32> = expected_lines.iter().copied().collect();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let o = compile(name, &out, &jar, None);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!o.status.success(), "scala-rs accepted {name}:\n{text}");
    assert_eq!(
        error_lines(name, &text, false),
        want,
        "scala-rs error lines for {name}:\n{text}"
    );
    if let Some(sc) = scalac() {
        let o = compile(name, &out, &jar, Some(&sc));
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(!o.status.success(), "scalac accepted {name}:\n{text}");
        assert_eq!(
            error_lines(name, &text, true),
            want,
            "scalac error lines for {name}:\n{text}"
        );
    } else {
        eprintln!("skip: scalac not present");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn hinf_gadt_runs() {
    check_runs("hinf_gadt", None);
}

#[test]
fn scalac_agrees_hinf_gadt() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("hinf_gadt", Some(&sc));
}

#[test]
fn hinf_gadt_bad_rejected() {
    check_rejects("hinf_gadt_bad", &[12, 15, 19, 25, 31]);
}

#[test]
fn hinf_nested_runs() {
    check_runs("hinf_nested", None);
}

#[test]
fn scalac_agrees_hinf_nested() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("hinf_nested", Some(&sc));
}

#[test]
fn hinf_nested_bad_rejected() {
    check_rejects("hinf_nested_bad", &[12, 13, 14, 15, 16]);
}

#[test]
fn hinf_lambda_runs() {
    check_runs("hinf_lambda", None);
}

#[test]
fn scalac_agrees_hinf_lambda() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("hinf_lambda", Some(&sc));
}

#[test]
fn hinf_lambda_bad_rejected() {
    check_rejects("hinf_lambda_bad", &[6, 7, 8]);
}

#[test]
fn hinf_nothing_runs() {
    check_runs("hinf_nothing", None);
}

#[test]
fn scalac_agrees_hinf_nothing() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("hinf_nothing", Some(&sc));
}

#[test]
fn hinf_nothing_bad_rejected() {
    check_rejects("hinf_nothing_bad", &[11, 13, 15, 17, 18]);
}
