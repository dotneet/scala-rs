//! E2E tests for the prelude-gap / small-type-check fixes tracked under the
//! `gap_*` fixture prefix:
//!   - numeric companion constants (`Int.MaxValue`, `Double.NaN`, ...)
//!   - `null.asInstanceOf[T]` / a generic `asInstanceOf[T]` actually typing
//!     as `T` (it used to always widen to `Any`) / `isInstanceOf` on
//!     primitives, classes, and an unbounded type parameter
//!   - case-class `copy(...)` (positional, partial, and named arguments —
//!     `copy` did not work at all before this fix, not just named args)
//!   - `java.lang.RuntimeException`/`Exception`/`Throwable` constructors and
//!     common methods, plus filling in an *omitted* constructor default
//!     argument (needed by slick's own
//!     `class SlickException(msg: String, parent: Throwable = null)
//!       extends RuntimeException(msg, parent)`)
//!
//! Kept in its own file (rather than appended to `e2e.rs`) per
//! `.agent-brief.md`'s guidance to avoid merge conflicts with other agents
//! editing the shared e2e suite concurrently. Helpers below are deliberately
//! duplicated from `e2e.rs` rather than shared, for the same reason.

use std::fs;
use std::path::PathBuf;
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
        "scala-rs-gaps-{tag}-{}-{nanos}",
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

fn find_scalac() -> Option<PathBuf> {
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
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
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
    out
}

fn compile_fails(name: &str, needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
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
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Compile+run under `--no-scala-library` (the private runtime). Used only
/// for fixtures whose feature does not depend on the real scala-library ABI.
fn check_private_runtime(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        let output = Command::new("java")
            .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
            .output()
            .expect("java");
        assert!(
            output.status.success(),
            "java -Xverify:all Main failed for {name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected_stdout(name),
            "stdout mismatch for {name} (private runtime)"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Compile+run against the real scala-library 2.13.16 jar, verified with
/// `-Xverify:all` and diffed against `expected_stdout` (itself produced by
/// running real scalac's output for the same fixture — see the module docs
/// on `gap_numeric`/`gap_asinstanceof`/`gap_copy`/`gap_exception` below for
/// where that was cross-checked).
fn dual_run(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip dual-run {name}: scala-library jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp out:scala-library failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Compile the fixture with *real* scalac and run it, then compare against
/// our own `--scala-library` compile — a live diff against real scalac
/// output, rather than a pre-baked `expected/*.txt` snapshot.
fn diff_against_real_scalac(name: &str) {
    if !java_available() {
        return;
    }
    let Some(scalac) = find_scalac() else {
        eprintln!("skip real-scalac diff {name}: scalac not obtainable");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip real-scalac diff {name}: scala-library jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let ref_cp = format!("{}:{}", ref_out.display(), jar.display());
    let ref_run = Command::new("java")
        .args(["-cp", &ref_cp, "Main"])
        .output()
        .expect("java (scalac reference)");
    assert!(
        ref_run.status.success(),
        "java Main (real-scalac build) failed for {name}: {}",
        String::from_utf8_lossy(&ref_run.stderr)
    );

    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    let cp = format!("{}:{}", out.display(), jar.display());
    let ours = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java (our build)");
    assert!(
        ours.status.success(),
        "java -Xverify:all Main (our build) failed for {name}: {}",
        String::from_utf8_lossy(&ours.stderr)
    );

    assert_eq!(
        String::from_utf8_lossy(&ours.stdout),
        String::from_utf8_lossy(&ref_run.stdout),
        "output diverged from real scalac for {name}"
    );
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&out);
}

// ---------------------------------------------------------------------- 1.
// Numeric companion constants (`Int.MaxValue`, `Double.NaN`, ...).
// Gated on `library_abi` (backed by `scala/Int$.MODULE$.MaxValue()` etc,
// real methods on the real scala-library jar) — `--no-scala-library` must
// diagnose rather than silently emit bytecode referencing a nonexistent
// private-runtime class.

#[test]
fn gap_numeric_dual_run() {
    dual_run("gap_numeric");
}

#[test]
fn gap_numeric_matches_real_scalac() {
    diff_against_real_scalac("gap_numeric");
}

#[test]
fn gap_numeric_bad_is_error_without_library() {
    compile_fails("gap_numeric_bad", "value MaxValue is not a member of Int");
}

// ---------------------------------------------------------------------- 2.
// `asInstanceOf`/`isInstanceOf`: `Type::Null` member resolution, a generic
// `asInstanceOf[T]` actually typing (and casting/unboxing) as `T` instead of
// always widening to `Any`, and member resolution on an unbounded
// `Type::TypeParam`. None of this needs the real scala-library ABI.

#[test]
fn gap_asinstanceof_dual_run() {
    dual_run("gap_asinstanceof");
}

#[test]
fn gap_asinstanceof_matches_real_scalac() {
    diff_against_real_scalac("gap_asinstanceof");
}

#[test]
fn gap_asinstanceof_bad_is_error() {
    compile_fails(
        "gap_asinstanceof_bad",
        "value thisMethodDoesNotExist is not a member of null",
    );
}

// ---------------------------------------------------------------------- 3.
// Case-class `copy(...)`: positional, partial (trailing args omitted and
// defaulted from the receiver), and named arguments. Works in both codegen
// modes (no scala-library ABI dependency), so this also runs against the
// private runtime.

#[test]
fn gap_copy_dual_run() {
    dual_run("gap_copy");
}

#[test]
fn gap_copy_private_runtime() {
    check_private_runtime("gap_copy");
}

#[test]
fn gap_copy_matches_real_scalac() {
    diff_against_real_scalac("gap_copy");
}

#[test]
fn gap_copy_bad_is_error() {
    compile_fails("gap_copy_bad", "unknown parameter name: z");
}

// ---------------------------------------------------------------------- 4.
// `RuntimeException`/`Exception`/`Throwable` constructors + common methods,
// and filling in an omitted trailing constructor default argument (needed
// for slick's own `SlickException(msg, parent = null)` pattern). The
// exception classes are real `java.lang.*` types (bootstrap classpath, not
// scala-library.jar), so — like `copy` — this isn't gated on `library_abi`.

#[test]
fn gap_exception_dual_run() {
    dual_run("gap_exception");
}

#[test]
fn gap_exception_matches_real_scalac() {
    diff_against_real_scalac("gap_exception");
}

#[test]
fn gap_exception_bad_is_error() {
    compile_fails(
        "gap_exception_bad",
        "no matching overload for constructor RuntimeException",
    );
}
