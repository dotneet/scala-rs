//! End-to-end CLI tests against `tests/fixtures`.

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
    let p = std::env::temp_dir().join(format!("scala-rs-e2e-{tag}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile_fixture(name: &str) -> PathBuf {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed");
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
    assert!(
        out.join("Main$.class").is_file(),
        "Main$.class missing in {}",
        out.display()
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

fn check(name: &str) {
    let out = compile_fixture(name);
    if java_available() {
        let got = run_java(&out);
        let exp = expected_stdout(name);
        assert_eq!(got, exp, "stdout mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn cli_help() {
    let output = Command::new(bin()).arg("--help").output().unwrap();
    assert!(output.status.success());
    let s = String::from_utf8_lossy(&output.stdout);
    assert!(s.contains("compile"));
    assert!(s.contains("Scala 2.13"));
}

#[test]
fn fixtures_hello() {
    check("hello");
}
#[test]
fn fixtures_arithmetic() {
    check("arithmetic");
}
#[test]
fn fixtures_class_methods() {
    check("class_methods");
}
#[test]
fn fixtures_case_match() {
    check("case_match");
}
#[test]
fn fixtures_factorial() {
    check("factorial");
}
#[test]
fn fixtures_trait_impl() {
    check("trait_impl");
}
#[test]
fn fixtures_while_loop() {
    check("while_loop");
}
#[test]
fn fixtures_string_interp() {
    check("string_interp");
}
#[test]
fn fixtures_list_for() {
    check("list_for");
}
#[test]
fn fixtures_option_for() {
    check("option_for");
}
#[test]
fn fixtures_lazy_val() {
    check("lazy_val");
}
#[test]
fn fixtures_implicits() {
    check("implicits");
}
#[test]
fn fixtures_generic_id() {
    check("generic_id");
}
#[test]
fn fixtures_defaults() {
    check("defaults");
}
#[test]
fn fixtures_byname() {
    check("byname");
}

#[test]
fn cli_run_hello() {
    if !java_available() {
        return;
    }
    let src = fixtures_dir().join("hello.scala");
    let output = Command::new(bin())
        .args(["run", src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout("hello")
    );
}

#[test]
fn parse_dump_contains_module() {
    let src = fixtures_dir().join("hello.scala");
    let output = Command::new(bin())
        .args(["compile", src.to_str().unwrap(), "--parse"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let s = String::from_utf8_lossy(&output.stdout);
    assert!(s.contains("Module Main"), "{s}");
}
