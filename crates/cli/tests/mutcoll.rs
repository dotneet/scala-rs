//! E2E tests for the `agent/mutcoll` slice: the `scala.collection.mutable`
//! gaps — the `f(i) = v` → `f.update(i, v)` desugar (SLS 6.15), the factory
//! `apply` that used to come back as the *immutable* collection of the same
//! simple name, and the `Queue` / `Stack` / `TreeMap` / `TreeSet` /
//! `PriorityQueue` / `ArraySeq` companions the prelude never declared.
//!
//! Kept in its own file to avoid merge conflicts with other agents working
//! `crates/cli/tests/e2e.rs`; see `.agent-brief.md`. All fixtures use the
//! `mc` prefix.

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
        "scala-rs-mc-{tag}-{}-{nanos}-{seq}",
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

/// Private runtime (`--no-scala-library`): nothing library-backed here.
fn check_private(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout(name),
            "stdout mismatch for {name} (--no-scala-library)"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// library-ABI run (`--scala-library <jar>`) against the real 2.13.16 jar.
fn check_library(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout(name),
        "stdout mismatch for library run {name}"
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

/// `f(args) = v` → `f.update(args, v)` for any receiver with an `update`
/// member: arrays, user classes, multi-index `update`, a selected receiver,
/// a generic one and a non-`Unit` one. Nothing here is library-backed, so it
/// has to run on the private runtime as well as against the jar.
#[test]
fn fixtures_mc_update_private() {
    check_private("mc_update");
}

#[test]
fn fixtures_mc_update_library() {
    check_library("mc_update");
}

/// The mutable maps, sets and buffers: `m(k) = v`, the companion factories
/// and the `Growable` / `Shrinkable` operators. `mutable.Set(1, 2, 3)` used
/// to be inferred as `scala.collection.immutable.Set`, so `+=` / `-=` /
/// `++=` / `--=` / `add` were all "not a member of Set[Int]".
#[test]
fn fixtures_mc_maps_library() {
    check_library("mc_maps");
}

/// `Queue` / `Stack` / `ArrayDeque` / `PriorityQueue` / `TreeSet` /
/// `TreeMap` / `ArraySeq` / `StringBuilder`: companion `apply` (with and
/// without arguments) and `empty`, `new X[T]()`, and the destructive members.
#[test]
fn fixtures_mc_queue_library() {
    check_library("mc_queue");
}

/// The private runtime has no `scala.collection.mutable` classfiles to call,
/// so these must keep being *diagnosed* there rather than silently accepted.
#[test]
fn fixtures_mc_maps_without_library_is_error() {
    let err = compile_errors("mc_maps", &["--no-scala-library"]);
    assert!(
        err.contains("is not a member"),
        "expected a not-a-member diagnostic without the jar, got: {err}"
    );
}

#[test]
fn fixtures_mc_queue_without_library_is_error() {
    let err = compile_errors("mc_queue", &["--no-scala-library"]);
    assert!(
        err.contains("is not a member"),
        "expected a not-a-member diagnostic without the jar, got: {err}"
    );
}

/// `m(k) = v` still type-checks the desugared `update` call, and a receiver
/// with no `update` member is rejected outright.
#[test]
fn fixtures_mc_maps_bad_is_error() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: jar not obtainable");
        return;
    };
    let err = compile_errors("mc_maps_bad", &["--scala-library", jar.to_str().unwrap()]);
    // `m("a") = "wrong type"` and `m(1) = 2`: the `update(String, Int)` call
    // the sugar produced, not a silent accept.
    assert!(
        err.contains(r#"(String, Int)Unit with arguments ("a", "wrong type")"#),
        "expected the wrong-value diagnostic, got: {err}"
    );
    assert!(
        err.contains("(String, Int)Unit with arguments (1, 2)"),
        "expected the wrong-key diagnostic, got: {err}"
    );
    assert!(
        err.contains("value update is not a member of NoUpdate"),
        "expected the no-`update` diagnostic, got: {err}"
    );
    assert!(
        err.contains(r#"with arguments ("not an Int")"#),
        "expected the `enqueue` element-type diagnostic, got: {err}"
    );
}

/// nsc reports a missing `op=` on an unassignable receiver as *one* error
/// with an explanatory second line. Raising it as two made the follow-up read
/// as an independent failure of whatever assignment came before it.
#[test]
fn fixtures_mc_queue_bad_is_one_error() {
    let err = compile_errors("mc_queue_bad", &["--no-scala-library"]);
    assert!(
        err.contains("value -= is not a member of Plain"),
        "expected the not-a-member diagnostic, got: {err}"
    );
    assert!(
        err.contains(
            "Expression does not convert to assignment because receiver is not assignable."
        ),
        "expected the explanatory line, got: {err}"
    );
    assert!(
        err.contains("1 error(s)"),
        "expected exactly one error, got: {err}"
    );
}
