//! Pattern matching that type-checked but miscompiled: `x @ Pat` bindings and
//! `null`.
//!
//! Two silent wrong-code bugs, both found by diffing against real scalac:
//!
//!  * `case n @ N(v, _) => n.copy(...)` stored the raw scrutinee, so `n` stayed
//!    typed as the *selector's* type in the frame and reading `N`'s fields off
//!    it was a `VerifyError`.
//!  * `case null =>` was compiled as `x.equals(null)`, which threw a
//!    `NullPointerException` on the one scrutinee the case exists to catch.
//!    The `==` operator had the same shape: `x == null` and, on the private
//!    runtime, any `x == y` with a null receiver.
//!
//! Every fixture is run three ways -- private runtime, real `scala-library`
//! jar, and real scalac -- and all three have to print the same thing.

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
        "scala-rs-patbind-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn compile(out: &Path, name: &str, extra: &[&str]) -> (bool, String) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let output = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .args(["-d", out.to_str().unwrap()])
        .args(extra)
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

/// `-Xverify:all` so a bad `StackMapTable` is a failure, not a silent pass.
fn run_main(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
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

/// Compile against the jar and check the program's stdout.
fn jar_run(name: &str) {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip {name}: jar or java not present");
        return;
    };
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    assert_eq!(
        run_main(&out, Some(&jar)),
        expected_stdout(name),
        "stdout mismatch for {name} (scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture on the private runtime (`--no-scala-library`).
fn private_run(name: &str) {
    if !java_available() {
        return;
    }
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--no-scala-library"]);
    assert!(ok, "compile {name} --no-scala-library failed:\n{msgs}");
    assert_eq!(
        run_main(&out, None),
        expected_stdout(name),
        "stdout mismatch for {name} (private runtime)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The recorded expectation has to be what real scalac 2.13.16 prints.
fn matches_real_scalac(name: &str) {
    let (Some(scalac), Some(jar), true) = (find_scalac(), scala_library_jar(), java_available())
    else {
        eprintln!("skip real-scalac diff {name}: scalac, jar or java not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-nsc"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    assert_eq!(
        run_main(&ref_out, Some(&jar)),
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

fn compile_fails(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: jar not present");
        return;
    };
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(!ok, "expected compile of {name} to fail, got:\n{msgs}");
    for needle in needles {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------------ fixtures

#[test]
fn pb_bind_private_runtime() {
    private_run("pb_bind");
}

#[test]
fn pb_bind_scala_library() {
    jar_run("pb_bind");
}

#[test]
fn pb_bind_matches_real_scalac() {
    matches_real_scalac("pb_bind");
}

#[test]
fn pb_null_private_runtime() {
    private_run("pb_null");
}

#[test]
fn pb_null_scala_library() {
    jar_run("pb_null");
}

#[test]
fn pb_null_matches_real_scalac() {
    matches_real_scalac("pb_null");
}

#[test]
fn pb_lit_private_runtime() {
    private_run("pb_lit");
}

#[test]
fn pb_lit_scala_library() {
    jar_run("pb_lit");
}

#[test]
fn pb_lit_matches_real_scalac() {
    matches_real_scalac("pb_lit");
}

#[test]
fn pb_eqnull_private_runtime() {
    private_run("pb_eqnull");
}

#[test]
fn pb_eqnull_scala_library() {
    jar_run("pb_eqnull");
}

#[test]
fn pb_eqnull_matches_real_scalac() {
    matches_real_scalac("pb_eqnull");
}

/// Sequence patterns, `Array` type patterns and the `Unit` constant need the
/// real library ABI; the private runtime has no `Seq` factory to unapply.
#[test]
fn pb_nullseq_scala_library() {
    jar_run("pb_nullseq");
}

#[test]
fn pb_nullseq_matches_real_scalac() {
    matches_real_scalac("pb_nullseq");
}

/// `Null` conforms to no value type: `case null` on a primitive scrutinee is
/// the mismatch nsc reports, not a case that is silently never taken.
#[test]
fn pb_null_on_primitive_is_an_error() {
    compile_fails(
        "pb_null_bad",
        &[
            "type mismatch; found: Null(null)  required: Int",
            "type mismatch; found: Null(null)  required: Double",
        ],
    );
}
