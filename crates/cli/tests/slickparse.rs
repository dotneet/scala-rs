//! Syntax slick uses that the parser did not accept yet.
//!
//! Every positive fixture is checked *differentially*: the same source goes
//! through scalac 2.13.16 and through scala-rs, both linked against the real
//! `scala-library-2.13.16.jar`, and the two programs must print the same
//! thing. Where scalac or the jar is missing the comparison is skipped and the
//! fixture is only required to compile and run.

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
    // Two tests can share a tag, and the clock is not fine enough to
    // separate them: they ran in the same directory and each `java Main` saw
    // the other's half-written output.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-slickparse-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    let which = Command::new("which").arg("scalac").output().ok()?;
    which
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8_lossy(&which.stdout).trim().to_string()))
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_main(cp: &str) -> Result<String, String> {
    let out = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn diagnostics(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    )
}

fn compile_ours(src: &Path, out: &Path, jar: &Path, extra: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
        "--scala-library",
        jar.to_str().unwrap(),
    ]);
    cmd.args(extra);
    cmd.output().expect("run scala-rs compile")
}

/// Compile `name` with both compilers and require identical stdout.
fn same_as_scalac(name: &str, extra: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip slickparse {name}: scala-library not available");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    assert!(src.is_file(), "missing fixture {name}.scala");

    let ours = tmp_dir(name);
    let output = compile_ours(&src, &ours, &jar, extra);
    assert!(
        output.status.success(),
        "scala-rs failed to compile {name} extra={extra:?}:\n{}",
        diagnostics(&output)
    );
    if !java_available() {
        let _ = fs::remove_dir_all(&ours);
        return;
    }
    let actual = run_main(&format!("{}:{}", ours.display(), jar.display()))
        .unwrap_or_else(|e| panic!("our {name} failed to run: {e}"));

    let Some(scalac) = scalac() else {
        eprintln!("skip slickparse {name} comparison: scalac not available");
        let _ = fs::remove_dir_all(&ours);
        return;
    };
    let ref_out = tmp_dir(&format!("{name}-scalac"));
    let mut cmd = Command::new(&scalac);
    cmd.args(["-d", ref_out.to_str().unwrap()]);
    cmd.args(extra);
    cmd.arg(src.to_str().unwrap());
    let status = cmd.status().expect("run scalac");
    assert!(status.success(), "scalac failed to compile {name}");
    let expected = run_main(&format!("{}:{}", ref_out.display(), jar.display()))
        .unwrap_or_else(|e| panic!("scalac-built {name} failed to run: {e}"));

    assert_eq!(actual, expected, "stdout differs from scalac for {name}");
    let _ = fs::remove_dir_all(&ref_out);
    let _ = fs::remove_dir_all(&ours);
}

/// Compile `name` and require it to be rejected with `needle` in the message.
fn fails_with(name: &str, extra: &[&str], needle: &str) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip slickparse {name}: scala-library not available");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    assert!(src.is_file(), "missing fixture {name}.scala");
    let out = tmp_dir(name);
    let output = compile_ours(&src, &out, &jar, extra);
    let _ = fs::remove_dir_all(&out);
    assert!(
        !output.status.success(),
        "expected compile of {name} extra={extra:?} to fail"
    );
    let err = diagnostics(&output);
    assert!(
        err.contains(needle),
        "expected {needle:?} in diagnostics for {name}, got:\n{err}"
    );
}

// -------------------------------------------------- `try b catch <handler>`

/// nsc `makeCatchFromExpr`: the handler is a `PartialFunction` value, is
/// evaluated only when the body throws, and rethrows what it does not accept.
#[test]
fn catch_of_a_partial_function_value() {
    same_as_scalac("slickparse_catch_expr", &[]);
}

// ------------------------------------------- `-Xsource:3` varargs patterns

/// `case Cast(ch*)` is the Scala 3 spelling of `case Cast(ch @ _*)`. nsc
/// accepts it under `-Xsource:3` and `-Xsource:3-cross` only.
#[test]
fn pattern_star_with_xsource3() {
    same_as_scalac("slickparse_pattern_star", &["-Xsource:3"]);
}

#[test]
fn pattern_star_with_xsource3_cross() {
    same_as_scalac("slickparse_pattern_star", &["-Xsource:3-cross"]);
}

/// Plain 2.13 rejects it with nsc's own wording.
#[test]
fn pattern_star_needs_xsource3() {
    fails_with(
        "slickparse_pattern_star_bad",
        &[],
        "bad simple pattern: use _* to match a sequence",
    );
}

// ------------------------------------------------ `super.T` in type position

/// `def createUpsertBuilder(node: Insert): super.InsertBuilder` — a path to a
/// type member of a parent. Also `C.super.T`, and `super.T` as a parent in an
/// `extends` clause, where the `super` is the *enclosing* class's.
#[test]
fn super_in_type_position() {
    same_as_scalac("slickparse_super_type", &[]);
}

#[test]
fn pattern_star_rejected_at_source_2_13() {
    fails_with(
        "slickparse_pattern_star_bad",
        &["-Xsource:2.13"],
        "bad simple pattern: use _* to match a sequence",
    );
}
