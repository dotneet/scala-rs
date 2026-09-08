//! E2E tests for the `agent/hkbound` slice: an **applied** higher-kinded type
//! parameter is at least its own bound, and `@uncheckedVariance` is a spelling
//! rather than a type on either side of `<:`.
//!
//! `BuildFrom.scala` declares `CC[X, Y] <: Map[X, Y] with MapOps[X, Y, CC, _]`
//! and then writes `(from: MapOps[K0, V0, CC, _])` -- an ascription of a value
//! to exactly the parameter's own bound. `SymbolTable::is_sub_type` read the
//! bound of an applied abstract type *member* and not of an applied type
//! *parameter*, so `CC[K0, V0]` conformed to nothing at all. nsc has no such
//! split: `isSubType2`'s abstract-type case reads `sym.info.bounds.hi` for any
//! abstract symbol.
//!
//! The second family in the same cluster is a different root. `Factory.scala`'s
//! `fill`/`tabulate` ladder relates `CC[CC[A]]` to `CC[CC[A] @uncheckedVariance]`
//! in both directions and at several depths. The two arms that erase an
//! annotation sat *below* the `Applied` arms, which match on one side and every
//! `other`, so they were never reached for an applied constructor.
//! `agent/basetypemeet` established that the annotation is a spelling when
//! merging base type arguments; this is the conformance half of the same fact.
//!
//! Correctness, not compilation: `hkbound_appliedbound.scala` prints six lines
//! whose values a wrong conformance answer changes. `choose` picks between two
//! overloads, and the applicable one is applicable *only* through the bound, so
//! a rule that answered the conformance question loosely would rewrite that
//! line rather than reject it. [`scalac_agrees_hkbound_runs`] compiles the same
//! source with real scalac and compares the two programs' output, so the
//! expected values are the oracle's and not a transcription of ours.
//!
//! On the pre-fix binary the first three groups do not compile
//! (`type mismatch; found: CC[K0, V0]  required: MapOps[K0, V0, CC, _]` and two
//! `no matching overload`) and two of the last three do not either
//! (`found: CC[CC[A]]  required: CC[CC[A] @uncheckedVariance]`). The negative
//! fixture is rejected identically on both binaries, at the same four lines --
//! it is what stops the new arms from over-reaching, not evidence that they
//! exist.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. Both fixtures use the `hkbound` prefix and are checked in
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
        "scala-rs-hkbound-{tag}-{}-{nanos}-{seq}",
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

/// The private runtime. Six lines, each checked by its printed value.
#[test]
fn fixtures_hkbound_appliedbound_runs() {
    if !java_available() {
        eprintln!("skip fixtures_hkbound_appliedbound_runs: no java");
        return;
    }
    let out = compile_fixture_with("hkbound_appliedbound", &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, None),
        expected_stdout("hkbound_appliedbound")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source linked against the real 2.13.16 ABI.
#[test]
fn fixtures_hkbound_appliedbound_runs_against_the_library() {
    let (true, Some(jar)) = (java_available(), scala_library_jar()) else {
        eprintln!("skip fixtures_hkbound_appliedbound_runs_against_the_library: no java or jar");
        return;
    };
    let out = compile_fixture_with(
        "hkbound_appliedbound",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap())),
        expected_stdout("hkbound_appliedbound")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The oracle. `expected/hkbound_appliedbound.txt` is real scalac 2.13.16's own
/// run of the same file, not a transcription of ours -- which matters because
/// the third line is an overload selection this compiler used to get wrong
/// while still compiling.
#[test]
fn scalac_agrees_hkbound_runs() {
    let (true, Some(sc), Some(jar)) = (java_available(), scalac(), scala_library_jar()) else {
        eprintln!("skip scalac_agrees_hkbound_runs: scalac, jar or java not available");
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
        .arg(fixtures_dir().join("hkbound_appliedbound.scala"))
        .output()
        .expect("run scalac");
    assert!(
        run.status.success(),
        "scalac 2.13.16 rejected hkbound_appliedbound: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let theirs = run_java(&theirs_out, Some(jar.to_str().unwrap()));
    let ours_out = compile_fixture_with("hkbound_appliedbound", &["--no-scala-library"]);
    assert_eq!(
        theirs,
        run_java(&ours_out, None),
        "applied higher-kinded bound differs from scalac 2.13.16"
    );
    assert_eq!(theirs, expected_stdout("hkbound_appliedbound"));
    let _ = fs::remove_dir_all(&theirs_out);
    let _ = fs::remove_dir_all(&ours_out);
}

// ------------------------------------------------- negative: the restrictions

/// Reading a bound must not make unrelated types conform, must not relate two
/// F-bounded parameters to each other, must not run backwards, and erasing an
/// annotation must not erase the type under it.
#[test]
fn fixtures_hkbound_appliedbound_bad_is_rejected() {
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
            "hkbound_appliedbound_bad",
            &mode,
            &[
                // The bound is `MapOps`, not everything.
                "type mismatch; found: CC[K0, V0]  required: Sink[K0, V0]",
                // Two F-bounded parameters are two constructors.
                "type mismatch; found: CC[Int, String]  required: DD[Int, String]",
                // The reduction is one-way.
                "type mismatch; found: MapOps[K0, V0, CC, _]  required: CC[K0, V0]",
                // The annotation is erased; the type under it is not.
                "type mismatch; found: Box[A] @uncheckedVariance  required: Box[String]",
            ],
        );
    }
}

/// Real scalac 2.13.16 rejects the same four lines, at 24, 31, 37 and 41.
#[test]
fn scalac_rejects_hkbound_bad_at_the_same_lines() {
    let Some(sc) = scalac() else {
        eprintln!("skip scalac_rejects_hkbound_bad_at_the_same_lines: scalac not available");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scalac_rejects_hkbound_bad_at_the_same_lines: jar not available");
        return;
    };
    let out = tmp_dir("scalac-bad");
    let output = Command::new(sc)
        .args([
            "-classpath",
            jar.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .arg(fixtures_dir().join("hkbound_appliedbound_bad.scala"))
        .output()
        .expect("run scalac");
    assert!(!output.status.success(), "scalac accepted the bad fixture");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "hkbound_appliedbound_bad.scala:24: error: type mismatch",
        "required: Sink[K0,V0]",
        "hkbound_appliedbound_bad.scala:31: error: type mismatch",
        "required: DD[Int,String]",
        "hkbound_appliedbound_bad.scala:37: error: type mismatch",
        "required: CC[K0,V0]",
        "hkbound_appliedbound_bad.scala:41: error: type mismatch",
        "required: Box[String]",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac to report {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
