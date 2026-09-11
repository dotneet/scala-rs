//! Application, overload resolution and argument conformance, as found by
//! compiling scala/scala's own standard library (`tests/scalalib_measure.sh`).
//!
//! Each fixture section is a standalone reduction of one library root:
//!
//! * `libapp_apply` -- a Java `Object[]` parameter takes `Array[AnyRef]` and
//!   `Array[Any]`; a bare overloaded call is not resolved against a group an
//!   earlier selection filed for another instantiation; `x += 1` on a
//!   getter/setter pair; the numeric lub of `if` / `match` branches; `var x:
//!   A = _`; a single constructor's parameters as its arguments' expected
//!   types.
//! * `libapp_outer` -- inherited members written bare inside a nested class,
//!   inner-class constructors read through their prefix, view specificity,
//!   and an `import num._` whose qualifier a later parameter shadows.
//! * `libapp_elemguess` -- a declared `foreach` parameter over in-scope type
//!   parameters is never replaced by the collection element guess.
//! * `libapp_bad` -- the neighbouring programs scalac rejects.
//!
//! Every fixture is checked against real scalac 2.13.16 in both directions.
//! Fixture prefix: `libapp_`.

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
        "scala-rs-libapp-{tag}-{}-{nanos}-{seq}",
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

fn compile(src: &Path, out: &Path, jar: &Path, scalac_path: Option<&Path>) -> std::process::Output {
    match scalac_path {
        Some(sc) => Command::new(sc)
            .args([
                "-classpath",
                jar.to_str().unwrap(),
                "-d",
                out.to_str().unwrap(),
            ])
            .arg(src)
            .output()
            .expect("run scalac"),
        None => Command::new(bin())
            .arg("compile")
            .arg(src)
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

/// Compile `name` with scala-rs (or scalac), run it under `-Xverify:all`, and
/// compare with the expected output.
fn check_runs(name: &str, scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let expected =
        fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap();
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(&src, &out, &jar, scalac_path);
    assert!(
        output.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(run_java(&out, &jar), expected);
    let _ = fs::remove_dir_all(&dir);
}

/// The source lines scala-rs (`  --> file:LINE:COL`) or scalac
/// (`file:LINE: error`) reports an error on.
fn error_lines(name: &str, text: &str, scalac: bool) -> Vec<u32> {
    let file = format!("{name}.scala:");
    let mut lines: Vec<u32> = text
        .lines()
        .filter(|l| {
            if scalac {
                l.contains(": error:")
            } else {
                l.trim_start().starts_with("-->")
            }
        })
        .filter_map(|l| {
            let rest = &l[l.find(&file)? + file.len()..];
            rest.split(':').next()?.parse().ok()
        })
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// Compile `name` and require it to be rejected with errors on exactly
/// `lines`.
fn check_rejects(name: &str, lines: &[u32], scalac_path: Option<&Path>) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(&src, &out, &jar, scalac_path);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success(),
        "compile unexpectedly succeeded:\n{text}"
    );
    assert_eq!(
        error_lines(name, &text, scalac_path.is_some()),
        lines,
        "error lines differ:\n{text}"
    );
    let _ = fs::remove_dir_all(&dir);
}

const BAD_LINES: &[u32] = &[8, 9, 19, 20, 29, 34, 39, 40, 42, 44, 46, 47, 49, 50, 59];

#[test]
fn libapp_apply_runs() {
    check_runs("libapp_apply", None);
}

#[test]
fn scalac_agrees_libapp_apply() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("libapp_apply", Some(&sc));
}

#[test]
fn libapp_outer_runs() {
    check_runs("libapp_outer", None);
}

#[test]
fn scalac_agrees_libapp_outer() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("libapp_outer", Some(&sc));
}

#[test]
fn libapp_elemguess_runs() {
    check_runs("libapp_elemguess", None);
}

#[test]
fn scalac_agrees_libapp_elemguess() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_runs("libapp_elemguess", Some(&sc));
}

#[test]
fn libapp_bad_is_rejected() {
    check_rejects("libapp_bad", BAD_LINES, None);
}

#[test]
fn scalac_agrees_libapp_bad() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_rejects("libapp_bad", BAD_LINES, Some(&sc));
}
