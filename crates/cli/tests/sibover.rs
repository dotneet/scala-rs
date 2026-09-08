//! E2E tests for the `agent/siblingover` slice: two sibling **overrides** of
//! one inherited member are ordered by the receiver's linearization.
//!
//! `TreeSet[A]` mixes in `IterableFactoryDefaults` (through `Set`, at
//! `CC = Set`) and `SortedSetFactoryDefaults` (at `CC = TreeSet`), and both
//! write `override def empty: CC[A @uncheckedVariance]` over the one
//! `IterableOps.empty`. Neither trait derives from the other, so
//! `Check::drop_overridden`'s owner test cannot order them, and both are
//! definitions, so its declaration/definition test does not apply. A bare
//! `empty` in `TreeSet` was `<overload Set[A] | TreeSet[A]>`.
//!
//! nsc's `findMember` walks the receiver's base type sequence and keeps the
//! first match, which here is `SortedSetFactoryDefaults`'s because it is the
//! more derived of the two *in `TreeSet`'s* linearization.
//!
//! That the answer belongs to the receiver and not to the pair is not an
//! argument, it is [`scalac_agrees_sibover_runs`]: one pair of traits, two
//! classes differing only in mixin order, and real scalac 2.13.16 runs a
//! different override in each. Choosing between two overrides decides which
//! body runs, so the positive fixture prints which one did, and the expected
//! output is scalac's own.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. Both fixtures use the `sibover` prefix and are checked in
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
        "scala-rs-sibover-{tag}-{}-{nanos}-{seq}",
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

/// Every line is checked by its value, and each names the override that ran.
///
/// On the pre-fix binary this fixture does not compile: `type mismatch; found:
/// <overload Wide[A @uncheckedVariance] | Narrow[A @uncheckedVariance]>
/// required: Narrow[A]` at the bare `emp`, and `no matching overload for
/// <overload (Wide[Int])String | (Narrow[Int])String>` at the selection --
/// which is the shape `TreeSet.scala` 85/90/96 reported against the real
/// standard library.
#[test]
fn fixtures_sibover_siblingoverride_runs() {
    if !java_available() {
        eprintln!("skip fixtures_sibover_siblingoverride_runs: no java");
        return;
    }
    let out = compile_fixture_with("sibover_siblingoverride", &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        expected_stdout("sibover_siblingoverride")
    );
    let _ = fs::remove_dir_all(&out);
}

#[test]
fn fixtures_sibover_siblingoverride_runs_against_the_library() {
    let (true, Some(jar)) = (java_available(), scala_library_jar()) else {
        eprintln!("skip fixtures_sibover_siblingoverride_runs_against_the_library: no java or jar");
        return;
    };
    let out = compile_fixture_with(
        "sibover_siblingoverride",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap())),
        expected_stdout("sibover_siblingoverride")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source through real scalac 2.13.16, run and compared. The expected
/// output is scalac's answer and not a transcription of ours -- including the
/// two lines that differ only by mixin order (`Fac2.emp` for `class Later
/// extends Fac1 with Fac2`, `Fac1.emp` for `class Earlier extends Fac2 with
/// Fac1`), which is what says the receiver decides.
#[test]
fn scalac_agrees_sibover_runs() {
    let (true, Some(sc), Some(jar)) = (java_available(), scalac(), scala_library_jar()) else {
        eprintln!("skip scalac_agrees_sibover_runs: scalac, jar or java not available");
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
        .arg(fixtures_dir().join("sibover_siblingoverride.scala"))
        .output()
        .expect("run scalac");
    assert!(
        run.status.success(),
        "scalac 2.13.16 rejected sibover_siblingoverride: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let theirs = run_java(&theirs_out, Some(jar.to_str().unwrap()));
    let ours_out = compile_fixture_with("sibover_siblingoverride", &["--no-scala-library"]);
    assert_eq!(
        theirs,
        run_java(&ours_out, None),
        "sibling override choice differs from scalac 2.13.16"
    );
    assert_eq!(theirs, expected_stdout("sibover_siblingoverride"));
    let _ = fs::remove_dir_all(&theirs_out);
    let _ = fs::remove_dir_all(&ours_out);
}

// ------------------------------------------------- negative: the restrictions

#[test]
fn fixtures_sibover_siblingoverride_bad_is_rejected() {
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
            "sibover_siblingoverride_bad",
            &mode,
            &[
                // Two sibling members with unrelated parameter types are two
                // members, whatever the linearization says about their owners.
                "ambiguous overload for h with arguments (null)",
                // The reduction runs on a set that was found; it invents none.
                "value nosuch is not a member of HBoth",
            ],
        );
    }
}

/// Real scalac 2.13.16 rejects the same two lines, at 35 and 36.
#[test]
fn scalac_rejects_sibover_bad_at_the_same_lines() {
    let Some(sc) = scalac() else {
        eprintln!("skip scalac_rejects_sibover_bad_at_the_same_lines: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("sibover_siblingoverride_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(!output.status.success(), "scalac accepted the bad fixture");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "sibover_siblingoverride_bad.scala:35: error: ambiguous reference to overloaded definition",
        "sibover_siblingoverride_bad.scala:36: error: value nosuch is not a member of HBoth",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac to report {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
