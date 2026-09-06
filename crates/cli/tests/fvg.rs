//! End-to-end coverage for value definitions and guards in for-comprehensions.
//!
//! Scala 2.13 extends the current element with a tuple while evaluating
//! consecutive value definitions. A following guard filters that tuple, and
//! a value definition after the guard extends the filtered stream again.
//! These tests compare both runtime ABIs and a real scalac 2.13.16 build.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn scala_library_jar() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    path.is_file().then_some(path)
}

fn scalac() -> Option<PathBuf> {
    let path = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    path.is_file().then_some(path)
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "scala-rs-fvg-{tag}-{}-{nanos}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn expected(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn compile(name: &str, out: &Path, extra: &[&str]) -> (bool, String) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .args(extra)
        .output()
        .expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), text)
}

fn run(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(jar) => format!("{}:{}", out.display(), jar.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run Main");
    assert!(
        output.status.success(),
        "java -Xverify:all failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn run_failure(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(jar) => format!("{}:{}", out.display(), jar.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run Main");
    assert!(!output.status.success(), "java unexpectedly succeeded");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn compile_and_run(name: &str, extra: &[&str], jar: Option<&Path>) -> String {
    let out = tmp_dir(name);
    let (ok, messages) = compile(name, &out, extra);
    assert!(ok, "compile {name} failed:\n{messages}");
    let result = run(&out, jar);
    let _ = fs::remove_dir_all(out);
    result
}

#[test]
fn map_with_filter_overloads_match_scalac() {
    let jar = scala_library_jar().expect("Map WithFilter requires scala-library");
    let nsc = scalac().expect("Map WithFilter requires scalac 2.13.16");
    let expected = expected("fvg_map_filter");
    let out = tmp_dir("map-with-filter-nsc");
    let control = Command::new(&nsc)
        .arg(fixtures_dir().join("fvg_map_filter.scala"))
        .arg("-d")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        control.status.success(),
        "{}",
        String::from_utf8_lossy(&control.stderr)
    );
    assert_eq!(run(&out, Some(&jar)), expected);
    assert_eq!(
        compile_and_run(
            "fvg_map_filter",
            &["--scala-library", jar.to_str().unwrap()],
            Some(&jar)
        ),
        expected
    );
    let negative = Command::new(nsc)
        .arg(fixtures_dir().join("fvg_map_filter_bad.scala"))
        .arg("-d")
        .arg(&out)
        .output()
        .unwrap();
    assert!(!negative.status.success());
    assert!(String::from_utf8_lossy(&negative.stderr).contains("type mismatch"));
    let (ok, diagnostic) = compile(
        "fvg_map_filter_bad",
        &out,
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(
        !ok,
        "flatMap must not inherit the lambda's List constructor"
    );
    assert!(diagnostic.contains("type mismatch"), "{diagnostic}");
    let _ = fs::remove_dir_all(out);
}

#[test]
fn value_definition_guards_match_scalac_in_both_runtime_modes() {
    let exp = expected("fvg_for");
    assert_eq!(
        compile_and_run("fvg_for", &["--no-scala-library"], None),
        exp,
        "private runtime output differs from scalac"
    );
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library ABI: jar not present");
        return;
    };
    assert_eq!(
        compile_and_run(
            "fvg_for",
            &["--scala-library", jar.to_str().unwrap()],
            Some(&jar)
        ),
        exp,
        "scala-library output differs from scalac"
    );
}

#[test]
fn value_definition_guards_match_real_scalac() {
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip scalac dual-run: scalac or scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("fvg_for.scala");
    let nsc_out = tmp_dir("fvg_for-scalac");
    let status = Command::new(scalac)
        .args([src.to_str().unwrap(), "-d", nsc_out.to_str().unwrap()])
        .status()
        .expect("run scalac");
    assert!(status.success(), "real scalac failed to compile fvg_for");
    assert_eq!(run(&nsc_out, Some(&jar)), expected("fvg_for"));
    let _ = fs::remove_dir_all(nsc_out);

    let ours = compile_and_run(
        "fvg_for",
        &["--scala-library", jar.to_str().unwrap()],
        Some(&jar),
    );
    assert_eq!(ours, expected("fvg_for"));
}

#[test]
fn value_definition_tuple_shape_matches_scalac() {
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip tuple-shape nsc comparison");
        return;
    };
    let exp = expected("fvg_for_shape");
    let src = fixtures_dir().join("fvg_for_shape.scala");

    let nsc_out = tmp_dir("fvg_for_shape-scalac");
    let status = Command::new(scalac)
        .args([src.to_str().unwrap(), "-d", nsc_out.to_str().unwrap()])
        .status()
        .expect("run scalac");
    assert!(
        status.success(),
        "real scalac failed to compile fvg_for_shape"
    );
    assert_eq!(run(&nsc_out, Some(&jar)), exp);
    let _ = fs::remove_dir_all(nsc_out);

    let ours = compile_and_run(
        "fvg_for_shape",
        &["--scala-library", jar.to_str().unwrap()],
        Some(&jar),
    );
    assert_eq!(ours, exp, "custom map observed a different tuple shape");
}

#[test]
fn value_definition_tuple_group_boundary_matches_scalac() {
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip tuple-group boundary nsc comparison");
        return;
    };
    let exp = expected("fvg_for_arity");
    let src = fixtures_dir().join("fvg_for_arity.scala");

    let nsc_out = tmp_dir("fvg_for_arity-scalac");
    let status = Command::new(scalac)
        .args([src.to_str().unwrap(), "-d", nsc_out.to_str().unwrap()])
        .status()
        .expect("run scalac");
    assert!(
        status.success(),
        "real scalac failed to compile fvg_for_arity"
    );
    assert_eq!(run(&nsc_out, Some(&jar)), exp);
    let _ = fs::remove_dir_all(nsc_out);

    let ours = compile_and_run(
        "fvg_for_arity",
        &["--scala-library", jar.to_str().unwrap()],
        Some(&jar),
    );
    assert_eq!(ours, exp, "value-definition group boundary differs");
}

#[test]
fn value_definition_before_generator_is_rejected() {
    let out = tmp_dir("fvg_for_bad");
    let (ok, messages) = compile(
        "fvg_for_bad",
        &out,
        &[
            "--scala-library",
            "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
        ],
    );
    assert!(!ok, "expected fvg_for_bad to fail:\n{messages}");
    assert!(
        messages.contains("value definition must follow a generator"),
        "unexpected diagnostics:\n{messages}"
    );
    let _ = fs::remove_dir_all(out);
}

#[test]
fn refutable_value_definition_failure_matches_scalac() {
    assert_match_error_fixture(
        "fvg_for_match_error",
        "scala.MatchError: 1 (of class java.lang.Integer)",
        "scala.MatchError: 1 (of class java.lang.Integer)",
        "scala.MatchError: 1 (of class java.lang.Integer)",
    );
    assert_match_error_fixture(
        "fvg_for_rematch_error",
        "scala.MatchError:",
        "scala.MatchError: (1,1)",
        "scala.MatchError: (1,1)",
    );
}

fn assert_match_error_fixture(
    name: &str,
    private_expected: &str,
    ours_expected: &str,
    nsc_expected: &str,
) {
    let src = fixtures_dir().join(format!("{name}.scala"));

    let private_out = tmp_dir(&format!("{name}-private"));
    let (ok, messages) = compile(name, &private_out, &["--no-scala-library"]);
    assert!(ok, "private runtime compile failed for {name}:\n{messages}");
    let private_error = run_failure(&private_out, None);
    assert!(
        private_error.contains(private_expected),
        "private runtime raised the wrong failure for {name}:\n{private_error}"
    );
    let _ = fs::remove_dir_all(private_out);

    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip refutable value-definition nsc comparison for {name}");
        return;
    };

    let ours_out = tmp_dir(&format!("{name}-ours"));
    let (ok, messages) = compile(name, &ours_out, &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "scala-library compile failed for {name}:\n{messages}");
    let ours_error = run_failure(&ours_out, Some(&jar));
    assert!(
        ours_error.contains(ours_expected),
        "scala-library raised the wrong failure for {name}:\n{ours_error}"
    );
    let _ = fs::remove_dir_all(ours_out);

    let nsc_out = tmp_dir(&format!("{name}-scalac"));
    let status = Command::new(scalac)
        .args([src.to_str().unwrap(), "-d", nsc_out.to_str().unwrap()])
        .status()
        .expect("run scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let nsc_error = run_failure(&nsc_out, Some(&jar));
    assert!(
        nsc_error.contains(nsc_expected),
        "scalac raised the wrong failure for {name}:\n{nsc_error}"
    );
    let _ = fs::remove_dir_all(nsc_out);
}
