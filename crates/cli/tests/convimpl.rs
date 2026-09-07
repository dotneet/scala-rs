//! E2E tests for the `agent/convimpl` slice: an implicit conversion is a
//! candidate only when its *whole* application succeeds, implicit clauses
//! included.
//!
//! `implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])` fits
//! every one-argument application by shape, so once partial unification
//! (`agent/hkunify`) started solving `F := Bag`, `A := Int` the way nsc does,
//! the view became applicable to every such receiver and `bag.flatMap(f)` was
//! reported as a missing `FlatMap[Bag]` -- twice, once for the view
//! `type_select` inserts and once for the one `rewrite_apply_extension`
//! inserts again.
//!
//! nsc does not report that. `inferView` types the application with its
//! implicit arguments; a failure makes the candidate *not applicable*, the
//! search goes on with the remaining views, and if none survives the
//! diagnostic that stands is the selection's own. The rule is narrow -- it
//! belongs to the search, not to the diagnostic -- so all four shapes below
//! are pinned against scalac 2.13.16:
//!
//! * `cimpl_view`: the witness is there, the view applies, the program runs.
//! * `cimpl_other_conv`: the witnessless view is discarded and the *other*
//!   conversion answers, rather than the two tying.
//! * `cimpl_view_bad`: no view survives -- `value flatMap is not a member of
//!   Bag[Int]`, at the selection's own position, reported once.
//! * `cimpl_explicit_bad`: written out by hand the conversion is an ordinary
//!   call, and the missing witness is still an error.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `cimpl` prefix.

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
        "scala-rs-convimpl-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
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
        "compile {name} failed extra={extra:?}:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// `-Xverify:all`, so a conversion applied with the wrong number of arguments
/// is a verification failure here rather than a silent difference.
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
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `--no-scala-library`: the private runtime.
fn check_private(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout(name),
            "stdout mismatch for {name} (private runtime)"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// `--scala-library`: linked against the real 2.13.16 ABI, then run.
fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_errors(name: &str, extra: &[&str]) -> String {
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
    let _ = fs::remove_dir_all(&out);
    err
}

// ------------------------------------------- 1. the witness is there: it runs

#[test]
fn fixtures_cimpl_view() {
    dual_run_fixture("cimpl_view");
}

#[test]
fn fixtures_cimpl_view_private() {
    check_private("cimpl_view");
}

// ---------------------------- 2. the other conversion answers, without a tie

#[test]
fn fixtures_cimpl_other_conv() {
    dual_run_fixture("cimpl_other_conv");
}

#[test]
fn fixtures_cimpl_other_conv_private() {
    check_private("cimpl_other_conv");
}

// -------------------------------- 3. no view survives: the member error alone

/// The diagnostic scalac reports, and only that one: no `FlatMap[Bag]` and no
/// second copy of it at the enclosing span.
#[test]
fn fixtures_cimpl_view_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let err = compile_errors(
        "cimpl_view_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(
        err.contains("value flatMap is not a member of Bag[Int]"),
        "expected the member error scalac reports, got:\n{err}"
    );
    assert!(
        !err.contains("could not find implicit value of type FlatMap[Bag]"),
        "the discarded view's own failure must not be reported:\n{err}"
    );
    assert_eq!(
        err.matches("value flatMap is not a member of Bag[Int]")
            .count(),
        1,
        "one selection, one diagnostic:\n{err}"
    );
}

#[test]
fn fixtures_cimpl_view_bad_is_rejected_private() {
    let err = compile_errors("cimpl_view_bad", &["--no-scala-library"]);
    assert!(
        err.contains("value flatMap is not a member of Bag[Int]"),
        "expected the member error scalac reports, got:\n{err}"
    );
    assert!(
        !err.contains("could not find implicit value of type FlatMap[Bag]"),
        "the discarded view's own failure must not be reported:\n{err}"
    );
}

// ------------------------- 4. spelled out by hand, the witness is still asked

/// Discarding a view is a rule of the *search*. An explicit call to the very
/// same conversion still needs its witness, and scalac says so.
#[test]
fn fixtures_cimpl_explicit_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let err = compile_errors(
        "cimpl_explicit_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(
        err.contains("could not find implicit value of type FlatMap[Bag]"),
        "expected the missing-witness error scalac reports, got:\n{err}"
    );
}

#[test]
fn fixtures_cimpl_explicit_bad_is_rejected_private() {
    let err = compile_errors("cimpl_explicit_bad", &["--no-scala-library"]);
    assert!(
        err.contains("could not find implicit value of type FlatMap[Bag]"),
        "expected the missing-witness error scalac reports, got:\n{err}"
    );
}
