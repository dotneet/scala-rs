//! E2E tests for the `agent/parentimpl` slice: the implicit and defaulted
//! arguments a parent constructor call leaves out (`class ConstColumn[T: TT]
//! extends TypedRep[T]`) are filled in the `extends` clause the same way they
//! are at a `new` site, so codegen stops emitting `TypedRep.<init>()`.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All new fixtures
//! use the `pimpl` prefix.

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
        "scala-rs-pimpl-{tag}-{}-{nanos}-{seq}",
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

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn compile_fails(name: &str, extra: &[&str], needle: &str) {
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

/// Private-runtime run: every filled parent constructor call has to verify and
/// produce the same output as scalac's.
#[test]
fn fixtures_pimpl() {
    let out = compile_fixture_with("pimpl", &["--no-scala-library"]);
    if java_available() {
        assert_eq!(run_java(&out, None), expected_stdout("pimpl"));
    }
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture against the real scala-library ABI.
#[test]
fn fixtures_pimpl_library_abi() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("pimpl", &["--scala-library", jar_s]);
    assert_eq!(run_java(&out, Some(jar_s)), expected_stdout("pimpl"));
    let _ = fs::remove_dir_all(&out);
}

/// Dual run: real scalac and scala-rs must print exactly the same thing, so a
/// silently dropped implicit argument cannot hide behind a hand-written
/// expectation file.
#[test]
fn real_scalac_dual_run_pimpl() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip scalac dual-run: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("pimpl.scala");
    let ref_out = tmp_dir("pimpl-scalac");
    let status = Command::new(&scalac)
        .args(["-d", ref_out.to_str().unwrap(), src.to_str().unwrap()])
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac failed on pimpl.scala");
    let jar_s = jar.to_str().unwrap();
    let want = run_java(&ref_out, Some(jar_s));
    let got_out = compile_fixture_with("pimpl", &["--scala-library", jar_s]);
    assert_eq!(run_java(&got_out, Some(jar_s)), want);
    assert_eq!(want, expected_stdout("pimpl"));
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&got_out);
}

/// The child is compiled *before* the file that declares its parent, so the
/// parent's context-bound evidence parameter does not exist yet when the
/// child's signature pass runs. Filling the parent constructor in the body
/// pass is what makes the command-line order irrelevant.
#[test]
fn fixtures_pimpl_late_parent_file() {
    let a = fixtures_dir().join("pimpl_late_a.scala");
    let z = fixtures_dir().join("pimpl_late_z.scala");
    let out = tmp_dir("pimpl_late");
    let output = Command::new(bin())
        .args([
            "compile",
            a.to_str().unwrap(),
            z.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile pimpl_late failed: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    if java_available() {
        assert_eq!(run_java(&out, None), expected_stdout("pimpl_late"));
    }
    let _ = fs::remove_dir_all(&out);
}

/// A parent implicit clause with no witness is diagnosed, never filled with
/// nothing: scalac reports `could not find implicit value for parameter tpe:
/// TT[String]` on the same `extends` line.
#[test]
fn fixtures_pimpl_bad_is_error() {
    compile_fails(
        "pimpl_bad",
        &["--no-scala-library"],
        "could not find implicit value of type TT[String]",
    );
}

/// ... and the diagnostic is on the `extends` clause, not on the `new` below
/// it, and there is exactly one of them.
#[test]
fn pimpl_bad_reports_the_extends_clause_once() {
    let src = fixtures_dir().join("pimpl_bad.scala");
    let out = tmp_dir("pimpl_bad-pos");
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
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        err.matches("could not find implicit value").count(),
        1,
        "expected exactly one missing-implicit diagnostic, got: {err}"
    );
    assert!(
        err.contains("pimpl_bad.scala:11:"),
        "expected the diagnostic on the `extends` line 11, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}
