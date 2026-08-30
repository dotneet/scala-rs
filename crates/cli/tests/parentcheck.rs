//! An unresolvable name in a template header must be reported, not swallowed.
//!
//! `object Bogus extends NoSuchThingHere` used to compile in both modes and
//! write a class file extending `java/lang/Object`: the name came out of
//! `resolve_type_name` as the `Type::Named` placeholder and nothing ever asked
//! whether it stood for anything. The same hole covered `with` items, the head
//! of an applied parent, a type argument inside one, self types, `new X` and
//! `new X {}`, and every qualified form of all of those.
//!
//! Every expected string here was read off real scalac 2.13.16 first (see the
//! header comment of each fixture), and the accepting fixture's expected
//! stdout is scalac's own output for the same source.
//!
//! Kept in its own file so it does not collide with the parallel work landing
//! in `e2e.rs`.

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
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-pc-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        return Some(cached);
    }
    let _ = fs::create_dir_all("/tmp/scala-rs-lib");
    let url = "https://repo1.maven.org/maven2/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar";
    let status = Command::new("curl")
        .args(["-fsSL", "-o", cached.to_str().unwrap(), url])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) && cached.is_file() {
        return Some(cached);
    }
    None
}

fn compile_fixture_with(name: &str, extra: &[&str]) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&Path>) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn check_private(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout(name),
            "stdout mismatch for private-runtime {name}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

fn check_library(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn diagnostics(name: &str, extra: &[&str]) -> String {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail ({extra:?})"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    // A rejected program must not leave class files behind.
    let wrote_classes = fs::read_dir(&out)
        .map(|d| {
            d.flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "class"))
        })
        .unwrap_or(false);
    assert!(
        !wrote_classes,
        "{name} was rejected but still wrote classes"
    );
    let _ = fs::remove_dir_all(&out);
    err
}

/// Both modes must reject it: a diagnostic that only fires with the jar on the
/// classpath would let the private runtime miscompile in silence.
fn compile_fails_both(name: &str, needles: &[&str]) {
    let mut modes: Vec<Vec<String>> = vec![vec!["--no-scala-library".to_string()]];
    if let Some(jar) = scala_library_jar() {
        modes.push(vec![
            "--scala-library".to_string(),
            jar.to_str().unwrap().to_string(),
        ]);
    }
    for m in &modes {
        let args: Vec<&str> = m.iter().map(|s| s.as_str()).collect();
        let err = diagnostics(name, &args);
        for needle in needles {
            assert!(
                err.contains(needle),
                "expected {needle:?} in diagnostics for {name} ({args:?}), got {err:?}"
            );
        }
    }
}

/// scalac 2.13.16, verbatim, for `tests/fixtures/pc_extends_bad.scala`.
#[test]
fn fixtures_pc_extends_bad_is_error() {
    compile_fails_both(
        "pc_extends_bad",
        &[
            "not found: type NoSuchThingHere",
            "not found: type AlsoMissing",
            "not found: type MissingTrait",
            "not found: type MissingMixin",
            "not found: type MissingGen",
            "not found: type MissingArg",
        ],
    );
}

/// A self type and a `new` are type positions too.
#[test]
fn fixtures_pc_selfnew_bad_is_error() {
    compile_fails_both(
        "pc_selfnew_bad",
        &[
            "not found: type MissingSelf",
            "not found: type Missing3",
            "not found: type Missing4",
            "not found: type MissingNew",
            "not found: type MissingNew2",
            // `new Obj` where `Obj` is an object: there is no *type* to build.
            "not found: type Obj",
        ],
    );
}

/// A qualified parent blames the segment that is actually missing.
#[test]
fn fixtures_pc_qualified_bad_is_error() {
    compile_fails_both(
        "pc_qualified_bad",
        &[
            "type NoSuch is not a member of object pcq.Holder",
            "type NoSuchInPkg is not a member of package pcq",
            "type NoSuchJU is not a member of package java.util",
            "type Nope is not a member of object pcq.Ob",
            "not found: value pkgless",
            "object nosuchpkg is not a member of package collection",
        ],
    );
}

/// The check must not fire on any parent that *does* resolve. Every shape in
/// the fixture reaches the same code path as the rejected ones.
#[test]
fn fixtures_pc_parents_private_runtime() {
    check_private("pc_parents");
}

#[test]
fn fixtures_pc_parents_scala_library() {
    check_library("pc_parents");
}

/// The wrong-namespace report `new Missing` used to draw. Kept as its own
/// assertion because the fix is a different one from the parent check.
#[test]
fn pc_new_of_a_missing_type_is_not_a_missing_value() {
    let err = diagnostics("pc_selfnew_bad", &["--no-scala-library"]);
    assert!(
        !err.contains("not found: value MissingNew"),
        "`new MissingNew` must report the type namespace, got {err:?}"
    );
}
