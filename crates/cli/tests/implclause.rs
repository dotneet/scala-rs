//! Regression tests for the bug where an implicit argument clause stayed unapplied in the type of the expression.
//!
//! `tests/fixtures/implclause.scala` collects the four roots (checked by hand to
//! produce the same output as real scalac 2.13.16). They all use `Map` /
//! `Iterator` / `ClassTag` / `Factory`, so none of them run under the private
//! runtime -- `--scala-library` mode only.
//!
//! The helpers follow `crates/cli/tests/lowbound.rs`. `java` is always run with
//! `-Xverify:all`, so codegen that got a type argument wrong cannot slip through
//! quietly.

use std::fs;
use std::path::PathBuf;
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
        "scala-rs-implclause-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
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
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    out
}

fn run_java_verified(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn check_library(name: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not available");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        let got = run_java_verified(&cp);
        assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

fn compile_diagnostics(name: &str) -> String {
    let Some(jar) = scala_library_jar() else {
        return String::new();
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    err
}

/// The fixture collecting the four roots produces the same output as real scalac.
#[test]
fn fixtures_implclause_scala_library() {
    check_library("implclause");
}

/// An implicit clause that cannot be filled is reported as a missing implicit, not
/// as "is not a member", in qualifier position too. Pins that what we relaxed did
/// not tip over into quietly accepting.
#[test]
fn implclause_bad_reports_missing_implicit() {
    let err = compile_diagnostics("implclause_bad");
    if err.is_empty() {
        eprintln!("skip: scala-library jar not available");
        return;
    }
    assert!(
        err.contains("could not find implicit value of type Sh[Int, R]"),
        "expected the missing implicit for the *qualifier*, got {err}"
    );
    assert!(
        !err.contains("is not a member of (Sh["),
        "the unapplied method type must not reach member lookup: {err}"
    );
    assert!(
        err.contains("could not find implicit value of type Coll[Seq]"),
        "a derivation rule whose non-tag implicit is missing must still fail: {err}"
    );
}
