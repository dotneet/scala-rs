//! End-to-end tests for members supplied from scala-library's `ScalaSignature`
//! pickles rather than from the hand-written prelude.
//!
//! Kept out of `e2e.rs` so pickle work and prelude work do not collide.
//!
//! These are **library-ABI only**: the private runtime has no `IterableOps`,
//! so `--no-scala-library` must keep diagnosing these members, and one of the
//! tests below checks exactly that.

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
        "scala-rs-pickle-{tag}-{}-{nanos}",
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
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn run_java(out: &Path, jar: &Path) -> String {
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile against the real jar and run. Skipped when the jar or a JVM is
/// missing, since there is nothing to supply members from.
fn run_against_library(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed");
    assert_eq!(run_java(&out, &jar), expected_stdout(name), "{name}");
    let _ = fs::remove_dir_all(&out);
}

fn compile_output(name: &str, extra: &[&str]) -> (bool, String) {
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
    let text = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    let ok = output.status.success();
    let _ = fs::remove_dir_all(&out);
    (ok, text)
}

#[test]
fn inherited_list_members_come_from_the_pickle() {
    run_against_library("pickle_lib1");
}

/// Completion adds members; it must never invent one.
#[test]
fn a_member_in_no_pickle_is_still_an_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let (ok, err) = compile_output(
        "pickle_lib1_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "pickle_lib1_bad unexpectedly compiled");
    assert!(
        err.contains("value nosuchmember is not a member"),
        "expected a not-a-member error, got {err:?}"
    );
}

/// The private runtime has no `IterableOps` to read a pickle from, so these
/// members must still be diagnosed there rather than silently accepted.
#[test]
fn private_runtime_still_diagnoses_library_only_members() {
    let (ok, err) = compile_output("pickle_lib1", &["--no-scala-library"]);
    assert!(!ok, "pickle_lib1 unexpectedly compiled without the library");
    assert!(
        err.contains("is not a member"),
        "expected a not-a-member error, got {err:?}"
    );
}
