//! E2E tests for the `boxed_*` fixtures: `java.lang.Integer` and its seven
//! siblings are types of their own, distinct from `scala.Int` & co.
//!
//! Before this, `crates/typer/src/prelude.rs` gave `scala.Int` the JVM name
//! `java/lang/Integer` (the box it erases to) and `classpath::find_by_jvm`
//! treated that name as an *identity*, so installing the real
//! `java.lang.Integer` classfile found `scala.Int`, poured `Integer`'s members
//! into it and never entered `Integer` into `java.lang`.
//! `java.lang.Integer.valueOf(3)` then failed with "value Integer is not a
//! member of <notype>", and `new java.util.ArrayList[java.lang.Long]` was an
//! `ArrayList[scala.Long]` that `add(7L)` could not satisfy.
//!
//! Kept in its own file (rather than appended to `e2e.rs`) per
//! `.agent-brief.md`'s guidance on avoiding merge conflicts; the helpers are
//! deliberately duplicated from `e2e.rs` for the same reason.

use std::fs;
use std::path::PathBuf;
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
        "scala-rs-boxed-{tag}-{}-{nanos}",
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

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    if cached.is_file() {
        return Some(cached);
    }
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
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
    let output = cmd.output().expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed extra={extra:?}: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn diagnostics_of(name: &str, extra: &[&str]) -> String {
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
        "expected compile of {name} to fail extra={extra:?}"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    err
}

/// Compile+run against the real scala-library jar under `-Xverify:all` and
/// diff against `expected/<name>.txt`, which was produced by running *real
/// scalac*'s build of the same fixture.
fn dual_run(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip dual-run {name}: scala-library jar not obtainable");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    let cp = format!("{}:{}", out.display(), jar.display());
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp out:scala-library failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Compile+run on the private runtime (`--no-scala-library`).
fn private_runtime_run(name: &str) {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", out.to_str().unwrap(), "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all Main failed for {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout(name),
        "stdout mismatch for {name} (private runtime)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Live diff against real scalac rather than the baked snapshot.
fn diff_against_real_scalac(name: &str) {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (find_scalac(), scala_library_jar()) else {
        eprintln!("skip real-scalac diff {name}: scalac or jar not obtainable");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-scalac-ref"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    let ref_cp = format!("{}:{}", ref_out.display(), jar.display());
    let ref_run = Command::new("java")
        .args(["-cp", &ref_cp, "Main"])
        .output()
        .expect("java (scalac reference)");
    assert!(
        ref_run.status.success(),
        "java Main (real-scalac build) failed for {name}: {}",
        String::from_utf8_lossy(&ref_run.stderr)
    );

    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    let cp = format!("{}:{}", out.display(), jar.display());
    let ours = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java (our build)");
    assert!(
        ours.status.success(),
        "java -Xverify:all Main (our build) failed for {name}: {}",
        String::from_utf8_lossy(&ours.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&ours.stdout),
        String::from_utf8_lossy(&ref_run.stdout),
        "output diverged from real scalac for {name}"
    );
    let _ = fs::remove_dir_all(&out);
    let _ = fs::remove_dir_all(&ref_out);
}

#[test]
fn boxed_dual_run_matches_expected() {
    dual_run("boxed");
}

#[test]
fn boxed_matches_real_scalac() {
    diff_against_real_scalac("boxed");
}

/// The boxing views are intrinsics (`Integer.valueOf` / `Integer.intValue`),
/// so they hold up on the private runtime, which has no `scala/Predef$`
/// `int2Integer` to call.
#[test]
fn boxed_rt_runs_on_private_runtime() {
    private_runtime_run("boxed_rt");
}

#[test]
fn boxed_rt_dual_run_matches_expected() {
    dual_run("boxed_rt");
}

/// Every line of `boxed_bad.scala` is an error in real scalac too; the
/// wrappers must not silently accept a neighbouring primitive's box.
#[test]
fn boxed_bad_is_rejected() {
    let jar = scala_library_jar();
    let jar_s = jar.as_ref().map(|p| p.to_str().unwrap().to_string());
    let mut modes: Vec<Vec<&str>> = vec![vec!["--no-scala-library"]];
    if let Some(j) = jar_s.as_deref() {
        modes.push(vec!["--scala-library", j]);
    }
    for extra in modes {
        let err = diagnostics_of("boxed_bad", &extra);
        for needle in [
            "found: 3L  required: Integer",
            "found: Long  required: Integer",
            "found: Long  required: Int",
            "found: Integer  required: String",
            "value parseInt is not a member of Integer",
        ] {
            assert!(
                err.contains(needle),
                "expected {needle:?} in diagnostics for boxed_bad ({extra:?}), got {err}"
            );
        }
    }
}
