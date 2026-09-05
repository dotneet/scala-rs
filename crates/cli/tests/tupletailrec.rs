//! E2E tests for the tuple / `@tailrec` slice: four roots behind 103 of the
//! 752 errors `tests/cats_measure.sh -Ykind-projector` reported on
//! typelevel/cats. See `docs/cats.md` for the write-up.
//!
//! 1. Every `TupleN` is a `case class`, which the prelude did not say, so
//!    `(a, b).copy(_1 = x)` was `value copy is not a member of (Any, Any)`.
//! 2. A *fully applied* type lambda is the type its body names; member lookup
//!    kept the unreduced application and found nothing on it.
//! 3. A higher-kinded parameter bounded by a proper type (`F[_, _] <: Product`)
//!    could never be instantiated at a type lambda: nsc eta-expands the bound
//!    alongside the constructor and compares the bodies.
//! 4. `@tailrec` eligibility is nsc's `isEffectivelyFinalOrNotOverridden`, not
//!    "private or final": an anonymous class's member, an un-overridden
//!    `sealed` class's member, a block-local class's member and a `def` in a
//!    `val`'s right-hand side are all accepted by scalac 2.13.16.
//!
//! Every fixture here is plain Scala 2.13 with no compiler plugin, so each has
//! a twin that runs it through real scalac 2.13.16: the positive ones compare
//! the program's output, the `_bad` ones compare which lines are rejected.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `tt` prefix.

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
        "scala-rs-tt-{tag}-{}-{nanos}-{seq}",
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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

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

/// Compile a fixture with scala-rs against the real scala-library and run it.
fn scala_rs_runs(name: &str, tag: &str) {
    let jar = scala_library_jar().expect("checked by the caller");
    let jar_s = jar.to_str().unwrap().to_string();
    let out = tmp_dir(tag);
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            &jar_s,
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile {name} failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(run_java(&out, &jar_s, "Main"), expected_stdout(name));
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture through real scalac: the expected output is nsc's, not
/// this compiler's idea of it.
fn scalac_runs(name: &str, tag: &str) {
    let sc = scalac().expect("checked by the caller");
    let jar = scala_library_jar().expect("checked by the caller");
    let out = tmp_dir(tag);
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join(format!("{name}.scala")))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected {name}:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, jar.to_str().unwrap(), "Main"),
        expected_stdout(name)
    );
    let _ = fs::remove_dir_all(&out);
}

/// The compiler's whole output for a fixture it must reject.
fn rejected(name: &str, tag: &str) -> String {
    let jar = scala_library_jar().expect("checked by the caller");
    let out = tmp_dir(tag);
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join(format!("{name}.scala"))
                .to_str()
                .unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "{name} was accepted:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    err
}

/// The source lines real scalac 2.13.16 reports an error on, in order.
fn scalac_error_lines(name: &str, tag: &str) -> Vec<u32> {
    let sc = scalac().expect("checked by the caller");
    let out = tmp_dir(tag);
    let path = fixtures_dir().join(format!("{name}.scala"));
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(&path)
        .output()
        .expect("run scalac");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
    let mut lines: Vec<u32> = text
        .lines()
        .filter(|l| l.contains(": error:"))
        .filter_map(|l| l.rsplit_once(".scala:").map(|(_, r)| r.to_string()))
        .filter_map(|r| r.split(':').next().and_then(|n| n.parse().ok()))
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

// ---------------------------------------------------------------------------
// Tuples, type lambdas, compound expected types.

#[test]
fn fixtures_tt_tuple() {
    if !java_available() || scala_library_jar().is_none() {
        eprintln!("skip: java or the scala-library jar is not present");
        return;
    }
    scala_rs_runs("tt_tuple", "tuple");
}

#[test]
fn scalac_agrees_tt_tuple_output() {
    if !java_available() || scalac().is_none() || scala_library_jar().is_none() {
        eprintln!("skip: java, scalac or the scala-library jar is not present");
        return;
    }
    scalac_runs("tt_tuple", "scalac-tuple");
}

#[test]
fn tt_tuple_bad_is_rejected() {
    if scala_library_jar().is_none() {
        eprintln!("skip: scala-library jar not present");
        return;
    }
    let err = rejected("tt_tuple_bad", "tuple-bad");
    assert!(
        err.contains("unknown parameter name: _3"),
        "a tuple's `copy` has exactly the tuple's own fields:\n{err}"
    );
    assert!(
        err.contains("found: Tuple2[String, String]"),
        "`copy` re-infers the type parameters, so the result widens:\n{err}"
    );
    assert!(
        err.contains("value _4 is not a member of (A0, Any, Any)"),
        "reducing the lambda offers its body's members and no others:\n{err}"
    );
    assert!(
        err.contains("do not conform to method bi's type parameter bounds"),
        "`Int` is not a `Product`, so the lambda misses the bound:\n{err}"
    );
}

/// Real scalac 2.13.16 rejects the same four lines, so the widening stops
/// where nsc stops.
#[test]
fn scalac_agrees_tt_tuple_bad_is_rejected() {
    if scalac().is_none() {
        eprintln!("skip: scalac not available");
        return;
    }
    assert_eq!(
        scalac_error_lines("tt_tuple_bad", "scalac-tuple-bad"),
        vec![11, 15, 24, 27]
    );
}

// ---------------------------------------------------------------------------
// `@tailrec` eligibility.

#[test]
fn fixtures_tt_tailrec() {
    if !java_available() || scala_library_jar().is_none() {
        eprintln!("skip: java or the scala-library jar is not present");
        return;
    }
    scala_rs_runs("tt_tailrec", "tailrec");
}

#[test]
fn scalac_agrees_tt_tailrec_output() {
    if !java_available() || scalac().is_none() || scala_library_jar().is_none() {
        eprintln!("skip: java, scalac or the scala-library jar is not present");
        return;
    }
    scalac_runs("tt_tailrec", "scalac-tailrec");
}

#[test]
fn tt_tailrec_bad_is_rejected() {
    if scala_library_jar().is_none() {
        eprintln!("skip: scala-library jar not present");
        return;
    }
    let err = rejected("tt_tailrec_bad", "tailrec-bad");
    assert_eq!(
        err.matches(
            "could not optimize @tailrec annotated method: it is neither private nor final"
        )
        .count(),
        5,
        "all five shapes are still rejected:\n{err}"
    );
}

/// The five lines nsc rejects are exactly the five lines rejected above.
#[test]
fn scalac_agrees_tt_tailrec_bad_is_rejected() {
    if scalac().is_none() {
        eprintln!("skip: scalac not available");
        return;
    }
    assert_eq!(
        scalac_error_lines("tt_tailrec_bad", "scalac-tailrec-bad"),
        vec![10, 15, 20, 29, 37]
    );
}
