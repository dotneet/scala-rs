//! Override checking, inheritance and `this.type` results, from compiling
//! scala/scala's own standard library (`tests/scalalib_measure.sh`).
//!
//! * `this.type` (SLS 3.2.1): `p.m()` with `m: this.type` is `p.type` on a
//!   stable `p` (a parameter, `this`, `super`), and a `C` on a receiver typed
//!   by an abstract `C`; a `var` or `def` inferred from one is widened.
//! * `Any.getClass(): Class[_]`, `AnyVal.getClass(): Class[_ <: AnyVal]`, and
//!   `java.lang.Object`'s final methods.
//! * A Java method's `Object` parameter matches a Scala `AnyRef` one.
//! * `override` of a member only the template's own self type has, and a
//!   self alias that is not a member.
//! * The superclass chain of a Java class file, and a member trait's self
//!   type read at the enclosing class.
//!
//! Fixture prefix: `libov_`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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
        "scala-rs-libov-{tag}-{}-{nanos}-{seq}",
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

/// Compile fixture `name` with scalac (`Some`) or scala-rs (`None`).
fn compile(name: &str, scalac_path: Option<&Path>, jar: &Path, out: &Path) -> Output {
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

fn both(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// Compile `name`, run it when `run`, and compare with the expected output.
fn check_accepts(name: &str, scalac_path: Option<&Path>, run: bool) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, scalac_path, &jar, &out);
    assert!(
        output.status.success(),
        "compile failed:\n{}",
        both(&output)
    );
    if run {
        let expected =
            fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt")))
                .unwrap();
        assert_eq!(run_java(&out, &jar), expected);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The source lines `name` is rejected at: scala-rs's `--> file:LINE:COL`,
/// scalac's `file:LINE: error:`.
fn rejected_lines(name: &str, scalac_path: Option<&Path>) -> Option<BTreeSet<usize>> {
    let jar = scala_library_jar()?;
    let dir = tmp_dir(name);
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = compile(name, scalac_path, &jar, &out);
    let text = both(&output);
    assert!(!output.status.success(), "{name} was accepted:\n{text}");
    let file = format!("{name}.scala:");
    let mut lines = BTreeSet::new();
    for l in text.lines() {
        let Some(at) = l.find(&file) else { continue };
        let rest = &l[at + file.len()..];
        let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let is_error = if scalac_path.is_some() {
            rest[n.len()..].starts_with(": error")
        } else {
            l.trim_start().starts_with("-->")
        };
        if is_error {
            lines.insert(n.parse().unwrap());
        }
    }
    let _ = fs::remove_dir_all(&dir);
    Some(lines)
}

fn check_rejects(name: &str, expected: &[usize]) {
    let Some(got) = rejected_lines(name, None) else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let want: BTreeSet<usize> = expected.iter().copied().collect();
    assert_eq!(got, want, "{name}: rejected lines");
}

fn scalac_rejects(name: &str, expected: &[usize]) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let Some(got) = rejected_lines(name, Some(&sc)) else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let want: BTreeSet<usize> = expected.iter().copied().collect();
    assert_eq!(got, want, "{name}: scalac's rejected lines");
}

const THISTYPE_BAD: &[usize] = &[10, 11, 12, 13, 15, 20];
const OVERRIDE_BAD: &[usize] = &[5, 6, 7, 8, 10, 11, 14, 16, 19];
const INHERIT_BAD: &[usize] = &[8, 9, 14, 15, 18];

#[test]
fn libov_thistype_runs() {
    check_accepts("libov_thistype", None, true);
}

#[test]
fn scalac_agrees_libov_thistype() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_accepts("libov_thistype", Some(&sc), true);
}

#[test]
fn libov_thistype_bad_rejected() {
    check_rejects("libov_thistype_bad", THISTYPE_BAD);
}

#[test]
fn scalac_agrees_libov_thistype_bad() {
    scalac_rejects("libov_thistype_bad", THISTYPE_BAD);
}

#[test]
fn libov_override_runs() {
    check_accepts("libov_override", None, true);
}

#[test]
fn scalac_agrees_libov_override() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_accepts("libov_override", Some(&sc), true);
}

#[test]
fn libov_getclass_compiles() {
    check_accepts("libov_getclass", None, false);
}

#[test]
fn scalac_agrees_libov_getclass() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    check_accepts("libov_getclass", Some(&sc), false);
}

#[test]
fn libov_override_bad_rejected() {
    check_rejects("libov_override_bad", OVERRIDE_BAD);
}

#[test]
fn scalac_agrees_libov_override_bad() {
    scalac_rejects("libov_override_bad", OVERRIDE_BAD);
}

#[test]
fn libov_inherit_bad_rejected() {
    check_rejects("libov_inherit_bad", INHERIT_BAD);
}

#[test]
fn scalac_agrees_libov_inherit_bad() {
    scalac_rejects("libov_inherit_bad", INHERIT_BAD);
}
