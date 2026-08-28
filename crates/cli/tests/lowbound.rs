//! End-to-end tests for lower-bounded (`[B >: A]`) method type parameters and
//! for type-parameter bound checking.
//!
//! Helpers mirror `crates/cli/tests/e2e.rs`; `java` is always run with
//! `-Xverify:all` so a wrong inferred element type shows up as a `VerifyError`
//! rather than silently passing.

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

/// Unique per call: tests run concurrently and several of them compile the same
/// fixture, so a shared output directory would let one test's cleanup delete
/// another's classfiles.
fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-lowbound-{tag}-{}-{nanos}-{seq}",
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
        "compile {name} failed extra={extra:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
    out
}

/// `java -Xverify:all -cp <cp> Main`.
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

/// Private-runtime run.
fn check_private(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        let got = run_java_verified(out.to_str().unwrap());
        assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// Real scala-library run.
fn check_library(name: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: scala-library jar not obtainable");
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

fn compile_fails_with(name: &str, needle: &str, extra: &[&str]) {
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
        "expected compile of {name} to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `Circle(1) :: Rect(2, 3) :: Nil` must be a `List[Shape]` in both modes.
#[test]
fn fixtures_lowbound_private_runtime() {
    check_private("lowbound");
}

#[test]
fn fixtures_lowbound_scala_library() {
    check_library("lowbound");
}

/// `List(Circle(1), Rect(2, 3))` takes the lub of the varargs. `List.apply` only
/// exists against the real scala-library ABI.
#[test]
fn fixtures_lowbound_lib_varargs() {
    check_library("lowbound_lib");
}

/// The private runtime has no `List.apply`; that must still be a diagnostic,
/// not a silent success.
#[test]
fn fixtures_lowbound_lib_needs_scala_library() {
    compile_fails_with("lowbound_lib", "apply", &["--no-scala-library"]);
}

/// `f(42)` against `def f[A <: Named]`.
#[test]
fn fixtures_lowbound_bad_is_error() {
    compile_fails_with(
        "lowbound_bad",
        "inferred type arguments [Int] do not conform to method f's type parameter bounds [A <: Named]",
        &["--no-scala-library"],
    );
}

/// `f[Int](42)` against `def f[A <: Named]`.
#[test]
fn fixtures_lowbound_bad2_is_error() {
    compile_fails_with(
        "lowbound_bad2",
        "type arguments [Int] do not conform to method f's type parameter bounds [A <: Named]",
        &["--no-scala-library"],
    );
}

/// `Box[Shape].widen[Circle]` violates `[B >: A]`.
#[test]
fn fixtures_lowbound_bad3_is_error() {
    compile_fails_with(
        "lowbound_bad3",
        "type arguments [Circle] do not conform to method widen's type parameter bounds [B >: A]",
        &["--no-scala-library"],
    );
}
