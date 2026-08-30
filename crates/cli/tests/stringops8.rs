//! E2E tests for the `agent/stringops8` slice: `StringOps` coverage.
//!
//! The slice's main change is architectural -- `Check::search_extension` now
//! asks the library `ScalaSignature` to complete an implicit conversion's
//! *result*, so most of `scala.collection.StringOps` arrives from the jar
//! instead of being hand-declared member by member. `so8.scala` exercises the
//! members that used to be missing, in both the `StringOps` and the
//! `WrappedString` (`Predef.wrapString`) families, and its expected output was
//! produced by **real scalac 2.13.16**, not by scala-rs.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All fixtures use
//! the `so8` prefix.

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
        "scala-rs-so8-{tag}-{}-{nanos}-{seq}",
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

/// Library-ABI dual-run. The expected file is scalac 2.13.16's own stdout, so
/// this asserts scala-rs agrees with the reference compiler, not merely with
/// itself. `-Xverify:all` is what catches a `$extension` descriptor that does
/// not match the jar.
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

/// The whole `StringOps` / `WrappedString` surface this slice added, run
/// against the real jar and compared with scalac's own output.
#[test]
fn fixtures_so8_lib() {
    dual_run_fixture("so8");
}

/// The private runtime (`--no-scala-library`) has no `StringOps` class at all,
/// so every one of these must stay a *diagnostic*. Silently accepting them
/// would emit calls to a class the private runtime never writes.
#[test]
fn fixtures_so8_without_library_is_error() {
    compile_fails(
        "so8",
        &["--no-scala-library"],
        "value zipWithIndex is not a member of String",
    );
}

/// `collect`'s two alternatives differ only in return type. Resolving to
/// *some* alternative is not enough -- a case block returning `Int` must pick
/// `IndexedSeq[B]`, so assigning it to a `String` has to be rejected. scalac
/// rejects this too (it reports the mismatch inside the case body).
#[test]
fn fixtures_so8_bad_collect_result_type_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: jar not obtainable");
        return;
    };
    compile_fails(
        "so8_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "type mismatch",
    );
}
