//! E2E tests for the `agent/declvsdef` slice: an overloaded reference in
//! value position, and a declaration standing beside the definition that
//! implements it.
//!
//! Two roots, both reproduced from the scalatra shapes gitbucket compiles
//! against, rewritten as source so nothing but scala-library is needed:
//!
//! * `def params(implicit r: Req): M` next to `def params(key: String)
//!   (implicit r: Req): S`. In value position nsc's `isAsSpecific` looks
//!   through the implicit clause, so the first alternative is strictly more
//!   specific; scala-rs kept the whole set and reported `value get is not a
//!   member of <overload …>`.
//! * an implicit `request` *declared* in one trait and *defined* in another,
//!   neither a base of the other, reached through a `self:` annotation or
//!   from an anonymous class nested in the class that mixes both in. Both
//!   candidates survived and every use was `ambiguous implicit: request,
//!   request`.
//!
//! Kept separate from `crates/cli/tests/e2e.rs` to avoid merge conflicts with
//! other agents working the same file; see `.agent-brief.md`. All fixtures use
//! the `dd_` prefix.

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
        "scala-rs-declvsdef-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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

fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
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

/// Compile against the real jar and check what `Main` prints.
fn run_fixture_lib(name: &str) -> Option<String> {
    if !java_available() {
        return None;
    }
    let jar = scala_library_jar()?;
    let jar_s = jar.to_str().unwrap();
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed");
    let got = run_java(&out, jar_s);
    assert_eq!(got, expected_stdout(name), "stdout mismatch for {name}");
    let _ = fs::remove_dir_all(&out);
    Some(got)
}

fn compile_fails(name: &str, extra: &[&str], needle: &str) {
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
    assert!(
        err.contains(needle),
        "expected {name} error to contain {needle:?}, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Both roots at once: the value-position overload through `extends`, through
/// a `self:` annotation, and from an anonymous class, all reading the same
/// implicit that one trait declares and another defines.
#[test]
fn fixtures_dd_implicitovl_lib() {
    run_fixture_lib("dd_implicitovl");
}

/// The same program under real scalac 2.13.16: identical stdout. The
/// alternative value position picks decides what is printed -- `params.get`
/// answers `Some(d1)` off the map, not off the `(key: String)` alternative --
/// so this is what says the pick matches nsc's and not merely that something
/// compiled.
#[test]
fn dd_implicitovl_matches_scalac() {
    if !java_available() {
        return;
    }
    let (Some(scalac), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip dd_implicitovl scalac dual-run: scalac or scala-library not available");
        return;
    };
    let src = fixtures_dir().join("dd_implicitovl.scala");
    let ref_out = tmp_dir("dd_implicitovl-scalac");
    let status = Command::new(&scalac)
        .args(["-d", ref_out.to_str().unwrap(), src.to_str().unwrap()])
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac failed to compile dd_implicitovl");
    let expected = run_java(&ref_out, jar.to_str().unwrap());
    assert_eq!(
        expected,
        expected_stdout("dd_implicitovl"),
        "scalac disagrees with the recorded expectation"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

/// Settling the reference in value position must not hide the set from
/// application position, and must not swallow a real "not a member". nsc
/// rejects both lines too (a type mismatch on the first, since it reports
/// against the one alternative of that arity).
#[test]
fn dd_implicitovl_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip dd_implicitovl_bad: scala-library not available");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    compile_fails(
        "dd_implicitovl_bad",
        &["--scala-library", jar_s],
        "no matching overload",
    );
    compile_fails(
        "dd_implicitovl_bad",
        &["--scala-library", jar_s],
        "value nope is not a member of Map[String, String]",
    );
}
