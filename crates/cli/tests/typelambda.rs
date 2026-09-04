//! E2E tests for the `agent/typelambda` slice: `({ type L[a] = … })#L` as a
//! type constructor argument.
//!
//! A *named* higher-kinded alias (`type EitherL[a] = Either[String, a]`) was
//! already usable as a type constructor. The structural form -- the one
//! kind-projector expands to, and the one cats writes by hand where the plugin
//! is not available -- was not, for two reasons:
//!
//! 1. Every written refinement allocates its own `TypeMember` symbol, so two
//!    spellings of the same lambda could never compare equal. Conformance now
//!    eta-expands both sides and compares the bodies.
//! 2. A lambda may mention type parameters of whatever encloses it
//!    (`Monad[({ type L[X] = Reader[R, X] })#L]` captures `R`), and a symbol
//!    reference cannot be substituted into. Captured parameters are now the
//!    member's own *leading* parameters and the projection is handed out
//!    partially applied to them, so ordinary substitution reaches the body.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All fixtures use
//! the `tl` prefix.

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
        "scala-rs-tl-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
}

/// Private-runtime run (`--no-scala-library`).
fn check(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        let got = run_java(&out, None);
        assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

/// library-ABI run (`--scala-library <jar>`). The expected file is the stdout
/// of real scalac 2.13.16 on the same source.
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

fn compile_errors(name: &str, extra: &[&str], needles: &[&str]) -> String {
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
    for n in needles {
        assert!(
            err.contains(n),
            "expected {name} error to contain {n:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
    err
}

/// The whole positive set, against the private runtime.
#[test]
fn fixtures_tl_lambda() {
    check("tl_lambda");
}

/// The same source against the real scala-library ABI. Both must print what
/// real scalac 2.13.16 prints.
#[test]
fn fixtures_tl_lambda_lib() {
    dual_run_fixture("tl_lambda");
}

/// Comparing lambdas by their bodies must not accept lambdas that differ.
/// scalac reports the same four errors on this file.
#[test]
fn fixtures_tl_lambda_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library check: jar not obtainable");
        return;
    };
    let err = compile_errors(
        "tl_lambda_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "required: Functor[[a]Box[a]]",
            "required: Functor[[a]Pair[String, a]]",
            "kinds of the type arguments ([a, b]Pair[a, b])",
            "type M is not a member of { type L[a] = Box[a] }",
        ],
    );
    assert!(
        err.contains("4 error(s)"),
        "expected exactly 4 errors, got: {err}"
    );
}

#[test]
fn fixtures_tl_lambda_bad_is_error_without_library() {
    compile_errors(
        "tl_lambda_bad",
        &["--no-scala-library"],
        &[
            "required: Functor[[a]Box[a]]",
            "required: Functor[[a]Pair[String, a]]",
            "kinds of the type arguments ([a, b]Pair[a, b])",
            "type M is not a member of { type L[a] = Box[a] }",
        ],
    );
}
