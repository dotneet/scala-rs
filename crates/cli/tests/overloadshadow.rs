//! E2E tests for the `agent/overloadshadow` slice: reading one class must not
//! shrink an overload set that is already in the symbol table.
//!
//! `scala.math.BigDecimal` declares an *instance* `apply(MathContext)` and its
//! companion object declares the `apply` overloads programs call. The pickled
//! instance one could only be expressed once something had pulled
//! `java.math.MathContext` into the table, so `BigDecimal(2)` compiled or not
//! depending on whether the unit had already mentioned `java.math.BigDecimal`.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All fixtures use
//! the `oshadow` prefix.

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
    // Two tests can share a tag, and the clock is not fine enough to separate
    // them: they ran in the same directory and each `java Main` saw the
    // other's half-written output.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-oshadow-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    let which = Command::new("which").arg("scalac").output().ok()?;
    which
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&which.stdout).trim().to_string()))
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    out
}

fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile against the real jar and check what `Main` prints.
fn run_fixture_lib(name: &str) -> Option<String> {
    if !java_available() {
        return None;
    }
    let jar = scala_library_jar()?;
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    let got = run_java(&out, jar_s);
    assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    let _ = fs::remove_dir_all(&out);
    Some(got)
}

fn compile_fails(name: &str, extra: &[&str], needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-bad"));
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
        "expected compile of {name} (extra={extra:?}) to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {name} error to contain {needle:?}, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `BigDecimal(Int)` / `(Long)` / `(String)` / `(BigInt)` /
/// `(java.math.BigDecimal)`, `Option[BigDecimal].getOrElse`, and
/// `java.math.BigDecimal` named both before and after them.
#[test]
fn fixtures_oshadow_lib() {
    run_fixture_lib("oshadow");
}

/// The same program under real scalac 2.13.16: identical stdout.
#[test]
fn oshadow_matches_scalac() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip oshadow scalac dual-run: scalac or scala-library not available");
        return;
    };
    let src = fixtures_dir().join("oshadow.scala");
    let ref_out = tmp_dir("oshadow-scalac");
    let status = Command::new(&scalac)
        .args(["-d", ref_out.to_str().unwrap(), src.to_str().unwrap()])
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac failed to compile oshadow");
    let expected = run_java(&ref_out, jar.to_str().unwrap());
    assert_eq!(
        expected,
        expected_stdout("oshadow"),
        "scalac disagrees with the recorded expectation"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// The regression itself: the same program with `java.math.BigDecimal` before
/// and after the companion's `apply` calls must compile and behave alike.
#[test]
fn oshadow_order_independent() {
    let first = run_fixture_lib("oshadow_java_first");
    let last = run_fixture_lib("oshadow_java_last");
    assert_eq!(
        first, last,
        "moving `java.math.BigDecimal` changed the program's behaviour"
    );
}

/// The companion has no `apply(Option)`, and the report must still show the
/// whole overload set rather than the single instance `apply(MathContext)`
/// that reading `java.math.BigDecimal` used to leave behind.
#[test]
fn oshadow_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip oshadow_bad: scala-library not available");
        return;
    };
    compile_fails(
        "oshadow_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "no matching overload",
    );
    compile_fails(
        "oshadow_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "(String)BigDecimal",
    );
}

/// `scala.math.BigDecimal` is backed only by the real jar; the private
/// runtime emits no `scala/math/BigDecimal$`, so `--no-scala-library` has to
/// keep diagnosing it instead of silently accepting it.
#[test]
fn oshadow_without_library_is_error() {
    compile_fails(
        "oshadow",
        &["--no-scala-library"],
        "not found: value BigDecimal",
    );
}
