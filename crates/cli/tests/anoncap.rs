//! Anonymous / local classes capturing enclosing-method locals.
//!
//! Every fixture is run twice: against the private runtime
//! (`--no-scala-library`) and against the real scala-library jar
//! (`--scala-library`), which must print the same thing.

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
        "scala-rs-anoncap-{tag}-{}-{nanos}",
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
    assert!(
        out.join("Main.class").is_file(),
        "Main.class missing in {}",
        out.display()
    );
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
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile and run `name` with the private runtime and with the jar; both must
/// print `tests/fixtures/expected/<name>.txt`.
fn check_both(name: &str) {
    if !java_available() {
        return;
    }
    let exp = expected_stdout(name);

    let out = compile_fixture_with(name, &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        exp,
        "stdout mismatch for {name} (private runtime)"
    );
    let _ = fs::remove_dir_all(&out);

    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name} library run: jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(
        !out.join("scala/runtime/IntRef.class").is_file(),
        "library ABI must not emit scala/runtime/IntRef"
    );
    assert_eq!(
        run_java(&out, Some(&jar)),
        exp,
        "stdout mismatch for {name} (scala-library)"
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

/// The capture field really is a field of the anonymous class, not a read of a
/// class named after the enclosing method (the shape that used to be emitted).
fn assert_capture_field(out: &Path, class_file: &str, field: &str) {
    let bytes = fs::read(out.join(class_file)).expect("read anonymous classfile");
    let text: String = bytes
        .iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { ' ' })
        .collect();
    assert!(
        text.contains(field),
        "{class_file} has no capture field {field}"
    );
}

#[test]
fn anoncap1_param_and_local_captures() {
    check_both("anoncap1");
}

#[test]
fn anoncap2_outer_lambda_and_nesting() {
    check_both("anoncap2");
}

#[test]
fn anoncap3_var_and_local_class() {
    check_both("anoncap3");
}

#[test]
fn anoncap1_bad_unknown_capture_is_error() {
    compile_fails("anoncap1_bad", "not found: value missingLocal");
}

#[test]
fn anoncap2_bad_forward_capture_is_error() {
    compile_fails("anoncap2_bad", "not found: value later");
}

/// The anonymous class of `mk(x: Int)` carries the captured `x` as its own
/// field and takes it as a constructor argument.
#[test]
fn anoncap1_emits_capture_field_and_ctor_param() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip capture field check: jar not obtainable");
        return;
    };
    let out = compile_fixture_with("anoncap1", &["--scala-library", jar.to_str().unwrap()]);
    assert_capture_field(&out, "$anon$1.class", "x$1");
    let _ = fs::remove_dir_all(&out);
}

/// A `var` captured by an anonymous class is shared through a
/// `scala.runtime.IntRef`, in both ABIs.
#[test]
fn anoncap3_boxes_captured_var() {
    let out = compile_fixture_with("anoncap3", &["--no-scala-library"]);
    assert!(
        out.join("scala/runtime/IntRef.class").is_file(),
        "private runtime must provide scala/runtime/IntRef"
    );
    let _ = fs::remove_dir_all(&out);
}
