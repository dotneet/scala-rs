//! E2E tests for the `agent/neglit` slice: a negated constant literal is not
//! folded before the narrowing check.
//!
//! ```scala
//! val b: Byte = -3     // was: type mismatch; found: Int  required: Byte
//! ```
//!
//! `-3` parses as `Apply(Select(Literal(3), "unary_-"), Nil)` -- there is no
//! negative-literal token in this parser. `unary_-` is a real method whose
//! declared return type is the widened `Int`, so by the time
//! `Typer::adapt`'s SLS 6.26.1 narrowing check runs, the tree's type is
//! `Type::Int`, not the `Type::Constant(Lit::Int(v))` a plain literal like
//! `3` already carries. SLS 6.24 defines a constant expression to include a
//! unary `-` applied to a literal, so `-3` should narrow exactly as `3`
//! does; `crates/typer/src/check_infer.rs`'s `negated_int_literal` recovers
//! that constant-ness from the tree shape and the existing narrowing check
//! (unchanged, not duplicated) does the rest.
//!
//! This costs `tests/scalalib_measure.sh` zero errors -- the library never
//! writes a negated literal against a `Byte`/`Short`/`Char` target. It is a
//! correctness fix, not a yield one.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. Fixtures use the `negl_` prefix.

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
        "scala-rs-neglit-{tag}-{}-{nanos}-{seq}",
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
    if let Ok(p) = Command::new("scalac").arg("-version").output() {
        if p.status.success() || !p.stderr.is_empty() || !p.stdout.is_empty() {
            return Some(PathBuf::from("scalac"));
        }
    }
    None
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

/// `-Xverify:all`: a fold that produced the wrong bit pattern (e.g. handing
/// the JVM `253` for a `Byte` meant to be `-3`) would show up here even
/// though it would still type-check.
fn run_java(out: &Path, cp_extra: &str, main: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, main])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all {main} failed: {}",
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

/// Every value is printed, not just type-checked, so a fold that lands on
/// the right *type* but the wrong *value* (a truncation, a sign flip) cannot
/// pass. Runs against the private runtime.
#[test]
fn fixtures_negl_run_no_library() {
    if !java_available() {
        return;
    }
    let out = compile_fixture_with("negl_run", &["--no-scala-library"]);
    assert_eq!(
        run_java(&out, "", "Main"),
        expected_stdout("negl_run"),
        "stdout mismatch for private-runtime run negl_run"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Same fixture, linked against the real scala-library jar -- the two modes
/// gate library-backed members differently, and this narrowing/folding path
/// does not touch either gate, but both are exercised anyway since the brief
/// asks for both when the private runtime supports them.
#[test]
fn fixtures_negl_run_with_library() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("negl_run", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, jar_s, "Main"),
        expected_stdout("negl_run"),
        "stdout mismatch for library dual-run negl_run"
    );
    let _ = fs::remove_dir_all(&out);
}

/// `expected/negl_run.txt` is real scalac 2.13.16's own run of the same
/// source, so the fixture cannot drift into recording this compiler's answer
/// instead of scalac's.
#[test]
fn scalac_agrees_negl_run() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-negl-run");
    let output = Command::new(sc)
        .args([
            "-classpath",
            jar.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
        ])
        .arg(fixtures_dir().join("negl_run.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected negl_run.scala: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar.to_str().unwrap(), "Main"),
        expected_stdout("negl_run"),
        "scalac's own run does not match the recorded output"
    );
    let _ = fs::remove_dir_all(&out);
}

// --------------------------------------- negative: the range still decides

/// `-300` does not fit a `Byte` any more than `300` does. Checked against
/// both modes since the narrowing check this touches runs before either
/// mode's library gating.
#[test]
fn fixtures_negl_bad_range_is_rejected_no_library() {
    compile_fails(
        "negl_bad_range",
        &["--no-scala-library"],
        &["type mismatch", "required: Byte", "negl_bad_range.scala:5"],
    );
}

#[test]
fn fixtures_negl_bad_range_is_rejected_with_library() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not available");
        return;
    };
    compile_fails(
        "negl_bad_range",
        &["--scala-library", jar.to_str().unwrap()],
        &["type mismatch", "required: Byte", "negl_bad_range.scala:5"],
    );
}

/// scalac, straight, at the same line -- so this cannot drift into a
/// restriction scalac itself does not have.
#[test]
fn scalac_agrees_negl_bad_range_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-negl-bad-range");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("negl_bad_range.scala"))
        .output()
        .expect("run scalac");
    assert!(
        !output.status.success(),
        "scalac accepted negl_bad_range.scala"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "negl_bad_range.scala:5: error: type mismatch",
        "found   : Int(-300)",
        "required: Byte",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac's error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------- negative: a non-constant still refuses

/// The fold is narrow on purpose: only `-<literal>` recovers a constant.
/// `-n` for a plain `val n = 3` is not a constant expression (SLS 6.24), and
/// must not narrow -- if it did, the fold would have gone from "recognizes a
/// negated literal" to "treats any negated `Int` as a `Byte`", which is
/// wrong.
#[test]
fn fixtures_negl_bad_nonconst_is_rejected_no_library() {
    compile_fails(
        "negl_bad_nonconst",
        &["--no-scala-library"],
        &[
            "type mismatch",
            "required: Byte",
            "negl_bad_nonconst.scala:8",
        ],
    );
}

#[test]
fn fixtures_negl_bad_nonconst_is_rejected_with_library() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not available");
        return;
    };
    compile_fails(
        "negl_bad_nonconst",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "type mismatch",
            "required: Byte",
            "negl_bad_nonconst.scala:8",
        ],
    );
}

#[test]
fn scalac_agrees_negl_bad_nonconst_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-negl-bad-nonconst");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("negl_bad_nonconst.scala"))
        .output()
        .expect("run scalac");
    assert!(
        !output.status.success(),
        "scalac accepted negl_bad_nonconst.scala"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "negl_bad_nonconst.scala:8: error: type mismatch",
        "found   : Int",
        "required: Byte",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac's error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------ negative: the boundary in both directions

/// `128` (positive, pre-existing plain-literal path) and `-129` (negated,
/// this slice's path) are each one past the value `negl_run.scala` accepts
/// on that side, and both must still be rejected.
#[test]
fn fixtures_negl_bad_boundary_is_rejected_no_library() {
    compile_fails(
        "negl_bad_boundary",
        &["--no-scala-library"],
        &[
            "negl_bad_boundary.scala:5",
            "required: Byte",
            "negl_bad_boundary.scala:6",
        ],
    );
}

#[test]
fn fixtures_negl_bad_boundary_is_rejected_with_library() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not available");
        return;
    };
    compile_fails(
        "negl_bad_boundary",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "negl_bad_boundary.scala:5",
            "required: Byte",
            "negl_bad_boundary.scala:6",
        ],
    );
}

#[test]
fn scalac_agrees_negl_bad_boundary_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-negl-bad-boundary");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("negl_bad_boundary.scala"))
        .output()
        .expect("run scalac");
    assert!(
        !output.status.success(),
        "scalac accepted negl_bad_boundary.scala"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    for needle in [
        "negl_bad_boundary.scala:5: error: type mismatch",
        "found   : Int(128)",
        "negl_bad_boundary.scala:6: error: type mismatch",
        "found   : Int(-129)",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac's error to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
