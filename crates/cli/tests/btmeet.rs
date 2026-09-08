//! E2E tests for the `agent/basetypemeet` slice: a base class two parents
//! reach at two different instantiations is read at the **meet** of them.
//!
//! `agent/basetypeseq` made `SymbolTable::base_type_args` keep the arrival the
//! most derived *reacher* supplies, walking the linearization and taking the
//! first. That is not nsc's rule and it is not enough: two parents can reach
//! one base without either standing above the other. `trait Str[+A] extends
//! LinOps[A, Str[A]] with Iter[A]` reaches `IterOps` as `IterOps[A, Str[A]]`
//! and as `IterOps[A, Iter[A]]`, SLS 5.1.2 lists `Iter` first because it is
//! written last, and `IterOps.tail` then read `Iter[A]` where scalac 2.13.16
//! types it `Str[A]`. In the standard library that is `Stream` reaching
//! `IterableOps` at `Iterable[A]`.
//!
//! `BaseTypeSeqs.compoundBaseTypeSeq` keeps every variant and resolves the
//! entry with `mergePrefixAndArgs(variants, Variance.Contravariant, _)`: the
//! glb at a covariant parameter, the lub at a contravariant one. Both halves
//! are in the fixture, because "take the most derived arrival" gets the
//! covariant half right and the contravariant half backwards.
//!
//! This is not a question a compile/no-compile check can answer on its own:
//! which base type a class is seen at decides which member is found and at
//! what signature, and a wrong answer routinely still compiles. So the
//! positive fixture is written so that a wrong instantiation cannot compile
//! *and* the right one produces output, and
//! [`scalac_agrees_btmeet_runs`] recompiles the same source with real scalac
//! 2.13.16 and compares the two programs.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. Both fixtures use the `btmeet` prefix and are checked in
//! both modes.

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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-btmeet-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
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
        "compile of {name} (extra={extra:?}) failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

fn run_java(out: &Path, cp_extra: Option<&str>) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("run java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
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
    for needle in needles {
        assert!(
            err.contains(needle),
            "expected {name} error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------- positive: it runs

/// The merged base type, in both modes. Every line is checked by its value, so
/// an alternative that merely type-checks cannot pass.
///
/// On the pre-fix binary this fixture does not compile: `value onlyOnStr is
/// not a member of Iter[A]` (the covariant half, taken at the first arrival)
/// and `no matching overload for (Dog)String with arguments (Animal)` (the
/// contravariant half, likewise).
#[test]
fn fixtures_btmeet_basetypemeet_runs() {
    if !java_available() {
        eprintln!("skip fixtures_btmeet_basetypemeet_runs: no java");
        return;
    }
    let out = compile_fixture_with("btmeet_basetypemeet", &["--no-scala-library"]);
    assert_eq!(run_java(&out, None), expected_stdout("btmeet_basetypemeet"));
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_btmeet_basetypemeet_runs_against_the_library() {
    let (true, Some(jar)) = (java_available(), scala_library_jar()) else {
        eprintln!("skip fixtures_btmeet_basetypemeet_runs_against_the_library: no java or jar");
        return;
    };
    let out = compile_fixture_with(
        "btmeet_basetypemeet",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap())),
        expected_stdout("btmeet_basetypemeet")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source through real scalac 2.13.16, run and compared. The expected
/// output is scalac's answer and not a transcription of ours.
#[test]
fn scalac_agrees_btmeet_runs() {
    let (true, Some(sc), Some(jar)) = (java_available(), scalac(), scala_library_jar()) else {
        eprintln!("skip scalac_agrees_btmeet_runs: scalac, jar or java not available");
        return;
    };
    let theirs_out = tmp_dir("scalac");
    let run = Command::new(sc)
        .args([
            "-classpath",
            jar.to_str().unwrap(),
            "-d",
            theirs_out.to_str().unwrap(),
        ])
        .arg(fixtures_dir().join("btmeet_basetypemeet.scala"))
        .output()
        .expect("run scalac");
    assert!(
        run.status.success(),
        "scalac 2.13.16 rejected btmeet_basetypemeet: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let theirs = run_java(&theirs_out, Some(jar.to_str().unwrap()));
    let ours_out = compile_fixture_with("btmeet_basetypemeet", &["--no-scala-library"]);
    assert_eq!(
        theirs,
        run_java(&ours_out, None),
        "base type meet differs from scalac 2.13.16"
    );
    assert_eq!(theirs, expected_stdout("btmeet_basetypemeet"));
    let _ = fs::remove_dir_all(&theirs_out);
    let _ = fs::remove_dir_all(&ours_out);
}

// ------------------------------------------------- negative: the restrictions

#[test]
fn fixtures_btmeet_basetypemeet_bad_is_rejected() {
    for mode in [
        vec!["--no-scala-library"],
        vec![
            "--scala-library",
            "/tmp/scala-rs-lib/scala-library-2.13.16.jar",
        ],
    ] {
        if mode[0] == "--scala-library" && scala_library_jar().is_none() {
            continue;
        }
        compile_fails(
            "btmeet_basetypemeet_bad",
            &mode,
            &[
                // The glb, not the most derived class in sight.
                "value onlyOnSub is not a member of Str[A]",
                // The lub at a contravariant parameter: `take` accepts an
                // `Animal`, and a `String` is not one.
                "no matching overload for (Animal)String",
                // And the merged type is still only what it is.
                "type mismatch; found: Str[Int]  required: Animal",
            ],
        );
    }
}

/// Real scalac 2.13.16 rejects the same three lines, at 23, 44 and 52, and
/// names `Str[A]` and `Str[Int]` in two of them -- which is the whole claim of
/// this slice, made by the oracle rather than by us.
#[test]
fn scalac_rejects_btmeet_bad_at_the_same_lines() {
    let Some(sc) = scalac() else {
        eprintln!("skip scalac_rejects_btmeet_bad_at_the_same_lines: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("btmeet_basetypemeet_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(!output.status.success(), "scalac accepted the bad fixture");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "btmeet_basetypemeet_bad.scala:23: error: value onlyOnSub is not a member of Str[A]",
        "btmeet_basetypemeet_bad.scala:44: error: type mismatch",
        "btmeet_basetypemeet_bad.scala:52: error: type mismatch",
        "found   : Str[Int]",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac to report {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
