//! `-Xsource:3` syntax: the `?` wildcard type and `A & B` compound types.
//!
//! Behaviour is pinned to scalac 2.13.16:
//!   * `?` in type position is a wildcard **without** any flag (using `?` as a
//!     type name requires backticks),
//!   * `A & B` needs `-Xsource:3` / `-Xsource:3-cross`; plain 2.13 rejects it.

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
    let p = std::env::temp_dir().join(format!("scala-rs-xs3-{tag}-{}-{nanos}", std::process::id()));
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

fn run_java(out: &Path) -> String {
    let output = Command::new("java")
        .args(["-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile(name: &str, extra: &[&str]) -> std::process::Output {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
        "--no-scala-library",
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    // Callers that need the class files re-compile via `check_runs`.
    let _ = fs::remove_dir_all(&out);
    output
}

fn diagnostics(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    )
}

/// Compile with `extra` flags, then run `Main` and compare against the fixture.
fn check_runs(name: &str, extra: &[&str]) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
        "--no-scala-library",
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}:\n{}",
        diagnostics(&output)
    );
    if java_available() {
        assert_eq!(run_java(&out), expected_stdout(name), "stdout for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

fn fails_with(name: &str, extra: &[&str], needle: &str) {
    let output = compile(name, extra);
    assert!(
        !output.status.success(),
        "expected compile of {name} extra={extra:?} to fail"
    );
    let err = diagnostics(&output);
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got {err:?}"
    );
}

// ---------------------------------------------------------------- wildcards

/// `?`, `? <: T`, `? >: T <: U` behave exactly like `_` — no flag needed,
/// matching scalac 2.13.16.
#[test]
fn wildcard_question_without_flag() {
    check_runs("xsource3_wildcard", &[]);
}

#[test]
fn wildcard_question_with_xsource3() {
    check_runs("xsource3_wildcard", &["-Xsource:3"]);
}

#[test]
fn wildcard_question_with_xsource3_cross() {
    check_runs("xsource3_wildcard", &["-Xsource:3-cross"]);
}

/// `type ?[A, B]` is rejected: `?` needs backticks to name a type.
#[test]
fn question_as_type_name_is_error() {
    fails_with(
        "xsource3_question_bad",
        &[],
        "using `?` as a type name requires backticks",
    );
}

#[test]
fn question_as_type_name_is_error_under_xsource3() {
    fails_with(
        "xsource3_question_bad",
        &["-Xsource:3"],
        "using `?` as a type name requires backticks",
    );
}

// ------------------------------------------------------------ intersections

#[test]
fn intersection_amp_with_xsource3() {
    check_runs("xsource3_intersection", &["-Xsource:3"]);
}

#[test]
fn intersection_amp_with_xsource3_cross() {
    check_runs("xsource3_intersection", &["-Xsource:3-cross"]);
}

/// Without the flag `&` stays an ordinary (unknown) infix type constructor.
#[test]
fn intersection_amp_without_flag_is_error() {
    fails_with("xsource3_intersection_bad", &[], "&");
}

/// …and the very same source compiles once the flag is on.
#[test]
fn intersection_bad_fixture_is_accepted_under_xsource3() {
    let output = compile("xsource3_intersection_bad", &["-Xsource:3"]);
    assert!(
        output.status.success(),
        "expected -Xsource:3 to accept the `&` fixture:\n{}",
        diagnostics(&output)
    );
}

// ------------------------------------------------------------------- flags

#[test]
fn xsource_2_13_is_accepted_and_keeps_2_13_syntax() {
    fails_with("xsource3_intersection_bad", &["-Xsource:2.13"], "&");
}

#[test]
fn xsource_below_current_major_is_rejected() {
    let output = compile("xsource3_wildcard", &["-Xsource:2.12"]);
    assert!(!output.status.success(), "expected -Xsource:2.12 to fail");
    assert!(
        diagnostics(&output).contains("-Xsource must be at least the current major version"),
        "got {:?}",
        diagnostics(&output)
    );
}

#[test]
fn help_mentions_xsource() {
    let output = Command::new(bin()).arg("--help").output().unwrap();
    assert!(output.status.success());
    let s = String::from_utf8_lossy(&output.stdout);
    assert!(s.contains("-Xsource:"), "help missing -Xsource: {s}");
}

// ------------------------------------------- function literals in block position

/// nsc `expr(InBlock)`: `{ x => val n = 1; n }` is a lambda whose body is the
/// rest of the block, and `{ x: Int => … }` ascribes with `InfixType` so the
/// `=>` stays with the lambda. Not `-Xsource:3` syntax — plain 2.13 — but it
/// shares this fixture prefix.
#[test]
fn function_literal_takes_the_rest_of_the_block() {
    check_runs("xsource3_block_lambda", &[]);
}

#[test]
fn function_literal_block_body_under_xsource3() {
    check_runs("xsource3_block_lambda", &["-Xsource:3"]);
}
