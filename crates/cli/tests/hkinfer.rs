//! E2E tests for the `agent/hkinfer` slice: two independent holes in how
//! arguments are matched against parameters.
//!
//! 1. An argument's *base type* is what pins the callee's type parameters.
//!    `object OC extends C[Option]` has type `OC.type`, and `C[Option]` is
//!    only reachable through its base types. This was **not** specific to
//!    higher-kinded parameters: the first-order `def u[A](d: D[A])` given an
//!    `object OD extends D[Int]` failed the same way. A *class* instance
//!    already worked, so the missing step was reading a singleton type
//!    (module, `this.type`, `p.type`) through what it widens to.
//! 2. Auto-tupling (SLS 6.6): an argument list that fits nothing is retried
//!    packed into a single tuple. This already worked for a single method;
//!    it was switched off for an overloaded callee, which is exactly the
//!    `println(1, "a")` case.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All fixtures
//! use the `hk` prefix.

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
        "scala-rs-hk-{tag}-{}-{nanos}-{seq}",
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
    let status = cmd.status().expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed extra={extra:?}");
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

/// library-ABI run (`--scala-library <jar>`).
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

// --- (1) base type of the argument -----------------------------------------

/// Module, `this.type` and `p.type` arguments, first order and higher-kinded,
/// against the private runtime.
#[test]
fn fixtures_hk_base() {
    check("hk_base");
}

#[test]
fn fixtures_hk_base_lib() {
    dual_run_fixture("hk_base_lib");
}

/// The base type has to *agree*, not merely exist: `object OD extends D[Int]`
/// is no `D[String]`, and it pins `A = Int` so a `String` second argument no
/// longer fits. scalac reports both as type mismatches.
#[test]
fn fixtures_hk_base_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library check: jar not obtainable");
        return;
    };
    let err = compile_errors(
        "hk_base_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &["(D[String])Int", "(D[A], A)Int"],
    );
    assert!(
        err.contains("2 error(s)"),
        "expected exactly 2 errors, got: {err}"
    );
}

#[test]
fn fixtures_hk_base_bad_is_error_without_library() {
    compile_errors(
        "hk_base_bad",
        &["--no-scala-library"],
        &["(D[String])Int", "(D[A], A)Int"],
    );
}

// --- (2) auto-tupling ------------------------------------------------------

#[test]
fn fixtures_hk_tuple() {
    check("hk_tuple");
}

#[test]
fn fixtures_hk_tuple_lib_mode() {
    dual_run_fixture("hk_tuple");
}

/// `println(1, "a")`: `println` is overloaded and none of its alternatives
/// takes two arguments. Only the real jar prints `(1,a)` -- the private
/// runtime's `Tuple2` has no `toString` of its own, and `println((1, "a"))`
/// written with explicit parens differs there in exactly the same way.
#[test]
fn fixtures_hk_tuple_lib() {
    dual_run_fixture("hk_tuple_lib");
}

/// Auto-tupling goes one way only, and never displaces an alternative of the
/// written arity. All four calls must stay errors, in both modes.
#[test]
fn fixtures_hk_tuple_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library check: jar not obtainable");
        return;
    };
    let err = compile_errors(
        "hk_tuple_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "(Int, Int)Int with arguments (Tuple2[Int, Int])",
            "(Int)Int with arguments (1, 2)",
            "()Int with arguments (1, 2)",
            "(String, String)String | ((Int, String))String",
        ],
    );
    assert!(
        err.contains("4 error(s)"),
        "expected exactly 4 errors, got: {err}"
    );
}

#[test]
fn fixtures_hk_tuple_bad_is_error_without_library() {
    let err = compile_errors(
        "hk_tuple_bad",
        &["--no-scala-library"],
        &[
            "(Int, Int)Int with arguments (Tuple2[Int, Int])",
            "(Int)Int with arguments (1, 2)",
            "()Int with arguments (1, 2)",
            "(String, String)String | ((Int, String))String",
        ],
    );
    assert!(
        err.contains("4 error(s)"),
        "expected exactly 4 errors, got: {err}"
    );
}
