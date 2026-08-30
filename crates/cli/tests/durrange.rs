//! E2E tests for the `agent/durrange` slice:
//!
//! 1. `scala.concurrent.duration`'s postfix units (`5.seconds`, `100.millis`,
//!    `1.second + 500.millis`) — the `package object duration` conversions
//!    `DurationInt` / `DurationLong` / `DurationDouble` and every unit method
//!    `DurationConversions` declares.
//! 2. The `Range` companion (`Range(0, 5)`, `Range(0, 10, 2)`,
//!    `Range.inclusive(1, 3)`, `Range.count`).
//! 3. An implicit parameter of *function* type filled from an `implicit def`,
//!    eta-expanded — the SLS 7.2 view route, which view bounds
//!    (`def f[A <% B]`) desugar onto as well.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All fixtures use
//! the `dr` prefix.

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
        "scala-rs-dr-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
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

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        Some(cached)
    } else {
        None
    }
}

/// Private-runtime check (`--no-scala-library`).
fn check(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        let got = run_java(&out, None);
        assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// library-ABI check (`--scala-library`), against the real 2.13.16 jar. The
/// expected file is real scalac's own stdout for the same source.
fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    let got = run_java(&out, Some(jar_s));
    assert_eq!(
        got,
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
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

// ---------------------------------------------------------------- 1. duration

#[test]
fn fixtures_dr_duration() {
    dual_run_fixture("dr_duration");
}

/// `scala.concurrent.duration` exists only in the jar: the private runtime
/// emits nothing under it. `--no-scala-library` must keep diagnosing the
/// postfix units rather than silently accept or miscompile them.
#[test]
fn fixtures_dr_duration_without_library_is_error() {
    compile_fails(
        "dr_duration",
        &["--no-scala-library"],
        "value nanoseconds is not a member of 1",
    );
}

// ------------------------------------------------------------------- 2. Range

#[test]
fn fixtures_dr_range() {
    dual_run_fixture("dr_range");
}

/// `prelude.rs` already gates the `Range` *class* on `library_abi` (without
/// the jar, `1 until 10` is a diagnostic), so the companion is gated the same
/// way — and the private runtime writes no `Range$` to call.
#[test]
fn fixtures_dr_range_without_library_is_error() {
    compile_fails(
        "dr_range",
        &["--no-scala-library"],
        "not found: value Range",
    );
}

// -------------------------------------------------- 3. function-typed views

#[test]
fn fixtures_dr_view() {
    dual_run_fixture("dr_view");
}

/// The *witness* for `A => Ordered[A]` is `scala.math.Ordered.orderingToOrdered`
/// plus an `Ordering[A]`, and neither `Ordered$` nor `Ordering` exists on the
/// private runtime. The view route itself is library-independent — see
/// `fixtures_dr_viewuser`, which exercises it with `--no-scala-library`.
#[test]
fn fixtures_dr_view_without_library_is_error() {
    compile_fails(
        "dr_view",
        &["--no-scala-library"],
        "no implicit: could not find implicit value of type (Int) => Ordered[Int]",
    );
}

/// The same route with the user's own `implicit def`s, so it runs on the
/// private runtime too: a monomorphic conversion, a polymorphic one that
/// carries its own implicit clause, a view bound, and an implicit parameter
/// passed on to a nested call.
#[test]
fn fixtures_dr_viewuser() {
    check("dr_viewuser");
}

#[test]
fn fixtures_dr_viewuser_lib() {
    dual_run_fixture("dr_viewuser");
}

/// Nothing is accepted that the view search did not actually witness: real
/// scalac reports `No implicit view available from Plain => Ordered[Plain]`
/// for the same source, in both modes.
#[test]
fn fixtures_dr_view_bad_is_error() {
    compile_fails(
        "dr_view_bad",
        &["--no-scala-library"],
        "no implicit: could not find implicit value of type (Plain) => Ordered[Plain]",
    );
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "dr_view_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "no implicit: could not find implicit value of type (Plain) => Ordered[Plain]",
    );
}

/// A method whose only parameter clause is implicit is not a value. When the
/// clause cannot be filled the result must be a diagnostic — never a silently
/// eta-expanded function value, which is what
/// `println(List(Some(1), None, Some(3)).flatten)` used to compile to
/// (printing `Main$$$anonfun$0@7a765367` at run time). Real scalac reports
/// `could not find implicit value for parameter m: Marker[A]` for this source.
#[test]
fn fixtures_dr_noimpl_bad_is_error() {
    compile_fails(
        "dr_noimpl_bad",
        &["--no-scala-library"],
        "no implicit: could not find implicit value of type Marker[A]",
    );
    let Some(jar) = scala_library_jar() else {
        return;
    };
    compile_fails(
        "dr_noimpl_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "no implicit: could not find implicit value of type Marker[A]",
    );
}
