//! E2E tests for the `agent/kindproj` slice: kind-projector's type-lambda
//! syntax behind `-Ykind-projector`.
//!
//! kind-projector is a compiler *plugin*, not Scala. nsc without it rejects
//! `Either[E, *]` and `λ[α => F[α]]` exactly as this compiler does without the
//! flag, so the two halves of this file are equally load-bearing:
//!
//! * with the flag, `kp_lambda.scala` compiles and prints what
//!   `scalac -Xplugin:kind-projector_2.13.16-0.13.3.jar` prints for it;
//! * without the flag, the same file is rejected.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All fixtures use
//! the `kp` prefix.

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
        "scala-rs-kp-{tag}-{}-{nanos}-{seq}",
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

/// Compile with the given flags, expecting failure, and return the output.
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

/// The whole positive set, against the private runtime.
#[test]
fn fixtures_kp_lambda() {
    let out = compile_fixture_with("kp_lambda", &["--no-scala-library", "-Ykind-projector"]);
    if java_available() {
        assert_eq!(run_java(&out, None), expected_stdout("kp_lambda"));
    }
    let _ = fs::remove_dir_all(&out);
}

/// The same source against the real scala-library ABI. The expected file is
/// the stdout of scalac 2.13.16 with kind-projector 0.13.3 on the same source.
#[test]
fn fixtures_kp_lambda_lib() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("kp_lambda", &["--scala-library", jar_s, "-Ykind-projector"]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("kp_lambda"),
        "stdout mismatch for library dual-run kp_lambda"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Without the flag the same file must be rejected, because nsc without the
/// plugin rejects it. This is the whole reason the desugaring is behind a
/// flag, so it is pinned in both library modes.
#[test]
fn kp_lambda_is_rejected_without_the_flag() {
    let needles = ["not found: type λ", "not found: type Lambda"];
    compile_errors("kp_lambda", &["--no-scala-library"], &needles);
    if let Some(jar) = scala_library_jar() {
        compile_errors(
            "kp_lambda",
            &["--scala-library", jar.to_str().unwrap()],
            &needles,
        );
    }
}

/// Shapes the plugin does not rewrite stay as written (nsc then reports
/// `not found: type λ`), and two lambdas that do not match are still an error.
/// scalac with the plugin reports these same four.
#[test]
fn fixtures_kp_lambda_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library check: jar not obtainable");
        return;
    };
    let err = compile_errors(
        "kp_lambda_bad",
        &["--scala-library", jar.to_str().unwrap(), "-Ykind-projector"],
        &[
            "not found: type λ",
            "required: Functor[[β$0$]Pair[String, β$0$]]",
            "required: Functor[[α]Pair[α, α]]",
        ],
    );
    assert!(
        err.contains("4 error(s)"),
        "expected exactly 4 errors, got: {err}"
    );
}

#[test]
fn fixtures_kp_lambda_bad_is_error_without_library() {
    let err = compile_errors(
        "kp_lambda_bad",
        &["--no-scala-library", "-Ykind-projector"],
        &[
            "not found: type λ",
            "required: Functor[[β$0$]Pair[String, β$0$]]",
            "required: Functor[[α]Pair[α, α]]",
        ],
    );
    assert!(
        err.contains("4 error(s)"),
        "expected exactly 4 errors, got: {err}"
    );
}

/// The flag must not change a program that does not use the syntax: `*` is
/// still multiplication, the repeated-parameter marker and a method name, and
/// `Lambda` is still an ordinary name. The expected output is plain scalac
/// 2.13.16's, with no plugin, so this also pins that the fixture is ordinary
/// Scala. Library ABI only: it uses varargs, which need `scala.Seq`.
#[test]
fn flag_does_not_disturb_ordinary_code() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    for extra in [
        vec!["--scala-library", jar_s, "-Ykind-projector"],
        vec!["--scala-library", jar_s],
    ] {
        let out = compile_fixture_with("kp_plain", &extra);
        assert_eq!(
            run_java(&out, Some(jar_s)),
            expected_stdout("kp_plain"),
            "stdout mismatch for kp_plain with {extra:?}"
        );
        let _ = fs::remove_dir_all(&out);
    }
}
