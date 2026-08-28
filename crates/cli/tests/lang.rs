//! Language-level end-to-end tests (patterns, type parameter bounds).
//!
//! Kept out of `e2e.rs` so language work and library work do not collide.

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
    let p = std::env::temp_dir().join(format!(
        "scala-rs-lang-{tag}-{}-{nanos}",
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
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn compile_with(name: &str, extra: &[&str]) -> PathBuf {
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

fn run_java(out: &Path, cp_extra: Option<&Path>) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Runs the fixture on the private runtime and, when the jar is present, again
/// against scala-library 2.13.16.
fn run_both(name: &str) {
    if !java_available() {
        return;
    }
    let out = compile_with(name, &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        expected_stdout(name),
        "{name} private"
    );
    let _ = fs::remove_dir_all(&out);
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name} dual-run: scala-library jar not present");
        return;
    };
    let out = compile_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        expected_stdout(name),
        "{name} scala-library"
    );
    let _ = fs::remove_dir_all(&out);
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
    assert!(!output.status.success(), "{name} unexpectedly compiled");
    let err = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    assert!(err.contains(needle), "expected {needle:?}, got {err:?}");
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn infix_pattern_runs() {
    run_both("infix_pat");
}

#[test]
fn infix_pattern_member_is_checked() {
    compile_fails("infix_pat_bad", "nosuch");
}

#[test]
fn type_param_bound_members_run() {
    run_both("tparam_bound");
}

#[test]
fn type_param_bound_member_is_checked() {
    compile_fails("tparam_bound_bad", "nosuchmember");
}

#[test]
fn pattern_definitions_run() {
    run_both("pat_def");
}

#[test]
fn pattern_definition_binding_is_typed() {
    compile_fails("pat_def_bad", "nosuchmember");
}

#[test]
fn tuple_literals_run() {
    run_both("tuple_lit");
}

#[test]
fn tuple_literal_binding_is_typed() {
    compile_fails("tuple_lit_bad", "nosuchmember");
}

/// `Tuple3` and up exist only on the library ABI; the private runtime ships
/// `scala/Tuple2` alone.
#[test]
fn tuple3_runs_on_the_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let out = compile_with("tuple3_lit", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(run_java(&out, Some(&jar)), expected_stdout("tuple3_lit"));
    let _ = std::fs::remove_dir_all(&out);
}

/// `for ((a, b) <- xs)` needs List's library `map`/`withFilter`, so this one is
/// library-only.
#[test]
fn for_generator_patterns_run() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let out = compile_with("for_pat", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(run_java(&out, Some(&jar)), expected_stdout("for_pat"));
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn for_generator_pattern_binding_is_typed() {
    compile_fails("for_pat_bad", "nosuchmember");
}

#[test]
fn case_class_copy_runs() {
    run_both("case_copy");
}

#[test]
fn case_class_copy_rejects_unknown_parameter() {
    compile_fails("case_copy_bad", "no parameter named");
}

/// A repeated parameter is a `Seq` in the body; the sequence types come from
/// the library ABI.
#[test]
fn varargs_run() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let out = compile_with("varargs", &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(run_java(&out, Some(&jar)), expected_stdout("varargs"));
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn varargs_parameter_is_a_seq() {
    compile_fails("varargs_bad", "nosuchmember");
}
