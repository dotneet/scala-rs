//! E2E tests for the `agent/stmtval` slice: three independent basic-shape
//! bugs.
//!
//! 1. A block whose last statement is a *definition* (`val` / `var` / `def` /
//!    `class` / `object` / `import` / `type`) has value `()` — nsc
//!    `TreeBuilder.makeBlock`. Without the trailing unit the block took the
//!    definition's type and codegen popped a value that was never pushed
//!    (`VerifyError: Operand stack underflow` for
//!    `def main(a: Array[String]): Unit = { val v = 1 }`).
//! 2. An op-assignment (`+=`, `-=`, `<<=`, …) has precedence 0 in nsc, below
//!    every other operator. Ranking `+=` with `+` parsed `n += i + x` as
//!    `(n += i) + x`, whose `Unit` left operand sent the typer into
//!    `any2stringadd`.
//! 3. `anewarray`'s operand is an internal name; for an array component that
//!    is the descriptor (`[I`). `new Array[Array[Int]](n)` emitted
//!    `anewarray java/lang/Object`, so `arr(i)(j)` failed verification.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All new
//! fixtures use the `sv` prefix.

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
        "scala-rs-sv-{tag}-{}-{nanos}-{seq}",
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
        "compile {name} failed extra={extra:?}:\n{}{}",
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

/// `-Xverify:all`: the whole point of items 1 and 3 is that the classfile the
/// old codegen produced did not verify.
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

/// Private-runtime check (`--no-scala-library`).
fn check(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        let got = run_java(&out, None);
        assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        Some(cached)
    } else {
        None
    }
}

/// library-ABI check (`--scala-library`), against the real scala-library
/// 2.13.16 jar.
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

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
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
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// ---------------------------------------------------------------------------
// 1. Blocks whose last statement is a definition.
// ---------------------------------------------------------------------------

#[test]
fn fixtures_sv_block() {
    check("sv_block");
}

#[test]
fn fixtures_sv_block_lib() {
    dual_run_fixture("sv_block");
}

/// The exact program from the report: a `Unit` method whose whole body is a
/// `val`. It used to emit `iconst_1; istore_2; pop; return`.
#[test]
fn method_body_that_is_only_a_val_verifies() {
    let dir = tmp_dir("onlyval-src");
    let src = dir.join("Main.scala");
    fs::write(
        &src,
        "object Main { def main(a: Array[String]): Unit = { val v = 1 } }\n",
    )
    .unwrap();
    let out = tmp_dir("onlyval");
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile failed");
    if java_available() {
        assert_eq!(run_java(&out, None), "");
    }
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&out);
}

// ---------------------------------------------------------------------------
// 2. Op-assignment precedence.
// ---------------------------------------------------------------------------

#[test]
fn fixtures_sv_opassign() {
    check("sv_opassign");
}

#[test]
fn fixtures_sv_opassign_lib() {
    dual_run_fixture("sv_opassign");
}

/// An op-assignment to an immutable receiver keeps nsc's
/// `convertToAssignment` diagnostic — the mis-parse used to hide it behind an
/// `any2stringadd` overload error.
#[test]
fn fixtures_sv_bad_reports_unassignable_receiver() {
    compile_fails(
        "sv_bad",
        &["--no-scala-library"],
        &[
            "value += is not a member of Int",
            "Expression does not convert to assignment because receiver is not assignable.",
        ],
    );
}

// ---------------------------------------------------------------------------
// 3. Nested array element erasure.
// ---------------------------------------------------------------------------

#[test]
fn fixtures_sv_array() {
    check("sv_array");
}

#[test]
fn fixtures_sv_array_lib() {
    dual_run_fixture("sv_array");
}

// ---------------------------------------------------------------------------
// 4. Op-assignment whose receiver is an indexing, and `Array.ofDim`.
// ---------------------------------------------------------------------------

#[test]
fn fixtures_sv_update() {
    check("sv_update");
}

#[test]
fn fixtures_sv_update_lib() {
    dual_run_fixture("sv_update");
}

/// `Array.ofDim[T](n1, …)` is five alternatives that all take one type
/// parameter: the explicit `[T]` can only be applied once the value arguments
/// have picked one, and codegen has to see that pick on the `Select` under the
/// `TypeApply`.
#[test]
fn fixtures_sv_ofdim_lib() {
    dual_run_fixture("sv_ofdim");
}

/// `Array.ofDim` is not backed by the private runtime.
#[test]
fn fixtures_sv_ofdim_without_library_is_error() {
    compile_fails(
        "sv_ofdim",
        &["--no-scala-library"],
        &["value ofDim is not a member of Array"],
    );
}

// ---------------------------------------------------------------------------
// All of it against members only the real scala-library backs.
// ---------------------------------------------------------------------------

#[test]
fn fixtures_sv_lib() {
    dual_run_fixture("sv_lib");
}

/// `List.apply` / `Int.max` are not backed by the private runtime;
/// `--no-scala-library` must keep diagnosing them rather than silently
/// accepting the fixture.
#[test]
fn fixtures_sv_lib_without_library_is_error() {
    compile_fails(
        "sv_lib",
        &["--no-scala-library"],
        &["value apply is not a member of List$"],
    );
}
