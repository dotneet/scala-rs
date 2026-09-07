//! E2E tests for the `agent/catsinfer` slice: a type parameter that reaches
//! inference only through an expected type still carrying an *undecided*
//! position.
//!
//! An undetermined variable in an argument's expected type is opened to
//! `Type::Wildcard` (`check_apply`'s `relaxed`, `open_to_bounds`) -- "the
//! argument decides this". `expected_solution` refused a *bare* one and read a
//! solution out of every nested one, so the inner calls of the argument took
//! their own parameters from a position that names nothing:
//!
//! ```text
//! found: P.F[T[_]]  required: P.F[T[B]]
//! no matching overload for (F[Either[A, B]])EitherT[F, A, B] with arguments (F[_])
//! ```
//!
//! The other half is the opposite mistake: an expected type that *does* say
//! something, inside a type lambda, which `collect_expected` could not walk
//! into -- `Applicative[Kleisli[F, A, *]]` names `A` nowhere else, so it was
//! minimised to `Nothing`.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `wci` prefix.

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
        "scala-rs-wcinfer-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`: an inference that produced a type the backend then erased
/// inconsistently would show up here rather than as a wrong value.
fn run_java(out: &Path, cp_extra: Option<&str>, main: &str) -> String {
    let cp = match cp_extra {
        Some(extra) => format!("{}:{}", out.display(), extra),
        None => out.display().to_string(),
    };
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
        run_java(&out, Some(jar_s), "Main"),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
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

/// The expected output is what real scalac 2.13.16 prints for the same source.
/// Every line depends on a solved variable: `mkString` needs `B = String`,
/// `v + 1` needs `B = Int`, and the label is built from the `Show[A]` the
/// lambda's `A` had to find.
#[test]
fn fixtures_wci_open() {
    dual_run_fixture("wci_open");
}

/// The fixture is only worth its runtime if real scalac accepts it too -- and
/// prints exactly what `expected/wci_open.txt` records.
#[test]
fn scalac_agrees_wci_open_runs() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-wci-open");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("wci_open.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected wci_open.scala: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap()), "Main"),
        expected_stdout("wci_open"),
        "scalac's own run does not match the recorded output"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The lambda parameter of `map` on a two-parameter covariant class. Each line
/// calls a method only the *right* element type has, so a parameter that
/// merely type-checks cannot pass.
#[test]
fn fixtures_wci_elem() {
    dual_run_fixture("wci_elem");
}

#[test]
fn scalac_agrees_wci_elem_runs() {
    let (Some(sc), Some(jar)) = (scalac(), scala_library_jar()) else {
        eprintln!("skip: scalac or scala-library not available");
        return;
    };
    if !java_available() {
        return;
    }
    let out = tmp_dir("scalac-wci-elem");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("wci_elem.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected wci_elem.scala: {}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap()), "Main"),
        expected_stdout("wci_elem"),
        "scalac's own run does not match the recorded output"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------- negative: the solutions bind

/// A declared type the call does not produce is still a mismatch, and the type
/// lambda that fixes `A` fixes it for the implicit search too. On the pre-fix
/// binary the first two are rejected for the *wrong* reason -- `P.F[T[_]]`
/// rather than `P.F[T[B]]`, and a missing `Apl2[F]` rather than the missing
/// `Show[Boolean]` -- which is what these needles pin.
#[test]
fn fixtures_wci_open_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not obtainable");
        return;
    };
    compile_fails(
        "wci_open_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "type mismatch; found: P.F[T[B]]  required: P.F[T[Int]]",
            "could not find implicit value of type Show[Boolean]",
            "type mismatch; found: Apl2[[x]Kle[Bx, Int, x]]  required: Apl2[[x]Kle[Bx, Boolean, x]]",
            "wci_open_bad.scala:33",
            "wci_open_bad.scala:50",
            "wci_open_bad.scala:52",
        ],
    );
}

/// The parameter type read off the signature still has to be checked. The
/// second of these is the one the pre-fix binary *accepted*: the guess made
/// `E => Int` the expected type, so a lambda written at `E` conformed.
#[test]
fn fixtures_wci_elem_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library not obtainable");
        return;
    };
    compile_fails(
        "wci_elem_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &[
            "type mismatch; found: (Int) => Int  required: (String) => Int",
            "type mismatch; found: (E) => Int  required: (A) => Int",
            "wci_elem_bad.scala:25",
            "wci_elem_bad.scala:27",
        ],
    );
}

#[test]
fn scalac_agrees_wci_elem_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-wci-elem-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("wci_elem_bad.scala"))
        .output()
        .expect("run scalac");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "scalac accepted it: {err}");
    for needle in [
        "wci_elem_bad.scala:25: error: type mismatch",
        "found   : Int => Int",
        "required: String => Int",
        "wci_elem_bad.scala:27: error: type mismatch",
        "found   : E => Int",
        "required: A => Int",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac output to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// The same three, straight from scalac, at the same lines, so the fixture
/// cannot drift into asserting a rejection nsc does not make.
#[test]
fn scalac_agrees_wci_open_bad_is_rejected() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-wci-open-bad");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("wci_open_bad.scala"))
        .output()
        .expect("run scalac");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "scalac accepted it: {err}");
    for needle in [
        "wci_open_bad.scala:33: error: type mismatch",
        "found   : P.F[T[B]]",
        "required: P.F[T[Int]]",
        "wci_open_bad.scala:50: error: could not find implicit value for parameter A: Show[Boolean]",
        "wci_open_bad.scala:52: error: type mismatch",
        "Kle[Bx,Int,",
        "Kle[Bx,Boolean,",
    ] {
        assert!(
            err.contains(needle),
            "expected scalac output to contain {needle:?}, got: {err}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}
