//! E2E tests for the `agent/implicitfilter` slice: the cheap structural
//! pre-test in front of implicit search, and the applicability rule that goes
//! with it.
//!
//! Both changes are in `crates/typer/src/implicits.rs` and both are
//! **diagnostic-neutral**: `tests/gitbucket_measure.sh` stays at
//! `errors=895 files_with_errors=111`, `tests/cats_measure.sh` at
//! `errors=346 files_with_errors=81`, and `tests/slick_measure.sh` at
//! `errors=0 files_with_errors=0 classes=1490`. What they change is cost --
//! see `docs/gitbucket.md`.
//!
//! **`Typer::plausibly_inhabits`** is nsc's `isPlausiblyCompatible`: when a
//! candidate's declared result type and the wanted type are both `Type::Class`
//! and neither head symbol reaches the other, no substitution of either side's
//! type parameters can change that, so the conformance check at the end of the
//! fit is already going to fail. Rejecting there skips `Unify`, whose
//! `Class`/`Class` arm builds a substituted base type on *both* sides before
//! it can say no, and -- far more expensively -- skips the recursion into
//! `implicit_fit_open`, which searches for the candidate's own clauses.
//!
//! **The `pinned` rule** in `implicit_fit_open` is the other half. That
//! fallback documents itself as running "only when the wanted type pinned down
//! at least one of the candidate's parameters", and enforced it by counting
//! solved parameters -- but `Unify::unify_at` answers a `_` with `true`
//! without recording a constraint, so slick's `tupleNShape` rules counted
//! `Level := _ <: FlatShapeLevel` as pinned and every one of the 22 of them
//! searched all of its own `Shape` clauses at every level of the derivation.
//! A parameter solved only to a wildcard no longer counts.
//!
//! The fixture uses a `Tuple3` so that the rule family *branches* -- one
//! arity cannot show a search choosing between rules -- which is why there is
//! no `--no-scala-library` variant here: the private runtime has no `Tuple3`.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`.

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
        "scala-rs-implfilter-{tag}-{}-{nanos}-{seq}",
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

fn real_scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Compile one fixture. Answers (success, diagnostics, output directory).
fn compile(name: &str, extra: &[&str]) -> (bool, String, PathBuf) {
    let out = tmp_dir(name);
    let src = fixtures_dir().join(format!("{name}.scala"));
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        src.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), msgs, out)
}

fn run_main(cp: &str) -> String {
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn scalac_file(scalac: &Path, src: &Path, tag: &str) -> (bool, String) {
    let out = tmp_dir(tag);
    let output = Command::new(scalac)
        .args(["-d", out.to_str().unwrap()])
        .arg(src)
        .output()
        .expect("run scalac");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&out);
    (output.status.success(), msgs)
}

/// The two things the cheap test must not get wrong: a witness whose result
/// class is a strict *subclass* of the wanted one is still found, and a
/// derivation rule the wanted type pins anywhere other than through a `_` is
/// still tried.
#[test]
fn the_structural_pre_test_loses_no_witness() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip implfilter: scala-library jar not present");
        return;
    };
    let (ok, msgs, out) = compile("implfilter", &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "implfilter failed to compile:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    if java_available() {
        let cp = format!("{}:{}", out.display(), jar.display());
        assert_eq!(
            run_main(&cp),
            expected_stdout("implfilter"),
            "stdout mismatch for implfilter"
        );
    }
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_file(
            &scalac,
            &fixtures_dir().join("implfilter.scala"),
            "implfilter_scalac",
        );
        assert!(ok, "real scalac rejected implfilter:\n{msgs}");
    }
}

/// The pre-test only removes candidates the fit would have rejected anyway.
/// A receiver no rule answers is still an error, and a *supertype* in scope is
/// still not a witness for a subtype -- which is exactly the direction the
/// structural test is entitled to reject on its own. Real scalac reports the
/// same two.
#[test]
fn implfilter_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip implfilter_bad: scala-library jar not present");
        return;
    };
    let (ok, msgs, out) = compile(
        "implfilter_bad",
        &["--scala-library", jar.to_str().unwrap()],
    );
    assert!(!ok, "expected implfilter_bad to be rejected:\n{msgs}");
    assert_eq!(
        msgs.matches("could not find implicit value of type Shape")
            .count(),
        1,
        "expected the Shape search to fail exactly once:\n{msgs}"
    );
    assert_eq!(
        msgs.matches("could not find implicit value of type Hex")
            .count(),
        1,
        "expected a Digits in scope not to answer a wanted Hex:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
    if let Some(scalac) = real_scalac() {
        let (ok, msgs) = scalac_file(
            &scalac,
            &fixtures_dir().join("implfilter_bad.scala"),
            "implfilter_bad_scalac",
        );
        assert!(!ok, "real scalac accepted implfilter_bad:\n{msgs}");
    }
}
