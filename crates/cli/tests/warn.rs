//! The warnings scalac 2.13.16 issues by default, printed the way its
//! console reporter prints them (`--diagnostics=scalac`): message, position
//! and caret, the order the phases report in, the deprecation summary, and
//! the closing `N warnings` count.
//!
//! Each fixture's expected text is scalac's own output for it, and a
//! scalac-agreement test checks that it still is when scalac is present.
//!
//! Fixture prefix: `warn_`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn tmp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-warn-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

/// Compile `name.scala` from the fixtures directory (as a relative path, so
/// the reported file name is the bare one scalac prints for it).
fn compile(compiler: &Path, scala_rs: bool, name: &str, flags: &[&str]) -> (bool, String) {
    let dir = tmp_dir(name);
    let mut cmd = Command::new(compiler);
    cmd.current_dir(fixtures_dir());
    if scala_rs {
        let jar = scala_library_jar().expect("jar");
        cmd.arg("compile")
            .arg("--diagnostics=scalac")
            .arg("--scala-library")
            .arg(jar);
    }
    cmd.arg("-d").arg(&dir).args(flags).arg(format!("{name}.scala"));
    let o = cmd.output().expect("run compiler");
    let _ = fs::remove_dir_all(&dir);
    (
        o.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
    )
}

fn expected(file: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{file}.txt"))).unwrap()
}

fn check_ours(name: &str, flags: &[&str], expected_file: &str) {
    if scala_library_jar().is_none() {
        eprintln!("skip: scala-library jar not present");
        return;
    }
    let (ok, out) = compile(&bin(), true, name, flags);
    assert!(ok, "{name} should compile with warnings only:\n{out}");
    assert_eq!(out, expected(expected_file), "scala-rs output for {name} {flags:?}");
}

fn check_scalac(name: &str, flags: &[&str], expected_file: &str) {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not present");
        return;
    };
    let (ok, out) = compile(&sc, false, name, flags);
    assert!(ok, "scalac rejected {name}:\n{out}");
    assert_eq!(out, expected(expected_file), "scalac output for {name} {flags:?}");
}

#[test]
fn patmat_warnings_match_scalac() {
    check_ours("warn_patmat", &[], "warn_patmat");
}

#[test]
fn patmat_expected_is_scalacs() {
    check_scalac("warn_patmat", &[], "warn_patmat");
}

#[test]
fn refchecks_warnings_match_scalac() {
    check_ours("warn_pure", &[], "warn_pure");
}

#[test]
fn refchecks_expected_is_scalacs() {
    check_scalac("warn_pure", &[], "warn_pure");
}

#[test]
fn deprecation_summary_matches_scalac() {
    check_ours("warn_depr", &[], "warn_depr");
}

#[test]
fn deprecation_summary_expected_is_scalacs() {
    check_scalac("warn_depr", &[], "warn_depr");
}

#[test]
fn deprecations_listed_under_the_flag_match_scalac() {
    check_ours("warn_depr", &["-deprecation"], "warn_depr_flag");
}

#[test]
fn deprecations_listed_expected_is_scalacs() {
    check_scalac("warn_depr", &["-deprecation"], "warn_depr_flag");
}

#[test]
fn later_phase_warnings_match_scalac() {
    check_ours("warn_later", &[], "warn_later");
}

#[test]
fn later_phase_expected_is_scalacs() {
    check_scalac("warn_later", &[], "warn_later");
}

#[test]
fn later_phase_deprecations_listed_match_scalac() {
    check_ours("warn_later", &["-deprecation"], "warn_later_flag");
}

#[test]
fn later_phase_deprecations_listed_expected_is_scalacs() {
    check_scalac("warn_later", &["-deprecation"], "warn_later_flag");
}

/// `-Werror` keeps the warnings as warnings and fails the run with one
/// position-less error after the summaries, as nsc does.
#[test]
fn werror_fails_after_the_warnings() {
    if scala_library_jar().is_none() {
        return;
    }
    let (ok, out) = compile(&bin(), true, "warn_depr", &["-Werror"]);
    assert!(!ok, "-Werror must fail the compilation:\n{out}");
    // The error comes after the summaries, and its count after the
    // warnings' count.
    let plain = expected("warn_depr");
    let (body, count) = plain
        .trim_end_matches('\n')
        .rsplit_once('\n')
        .expect("a warning count line");
    let want = format!("{body}\nerror: No warnings can be incurred under -Werror.\n{count}\n1 error\n");
    assert_eq!(out, want);
}

/// `-nowarn` silences every warning, summaries included.
#[test]
fn nowarn_prints_nothing() {
    if scala_library_jar().is_none() {
        return;
    }
    let (ok, out) = compile(&bin(), true, "warn_patmat", &["-nowarn"]);
    assert!(ok);
    assert_eq!(out, "");
}

/// The warnings never change what a valid program does.
#[test]
fn warned_programs_still_run() {
    let Some(jar) = scala_library_jar() else {
        return;
    };
    for name in ["warn_patmat", "warn_pure", "warn_depr", "warn_later"] {
        let dir = tmp_dir(name);
        let o = Command::new(bin())
            .current_dir(fixtures_dir())
            .args(["compile", "--scala-library"])
            .arg(&jar)
            .arg("-d")
            .arg(&dir)
            .arg(format!("{name}.scala"))
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let main = match name {
            "warn_patmat" => "WarnPatmat",
            "warn_pure" => "WarnPure",
            "warn_later" => "WarnLater",
            _ => "WarnDepr",
        };
        let cp = format!("{}:{}", dir.display(), jar.display());
        let r = Command::new("java")
            .args(["-Xverify:all", "-cp", &cp, main])
            .output()
            .unwrap();
        assert!(r.status.success(), "{name}: {}", String::from_utf8_lossy(&r.stderr));
        let _ = fs::remove_dir_all(&dir);
    }
}
