//! The two rules whose whole job is to *reject* — variance (SLS 4.5) and
//! self-type conformance — turned out to reject shapes scalac 2.13.16 accepts.
//! slick's `BasicProfile`/`SqlProfile` (7 variance errors) and its four
//! `Database` cake classes (4 `illegal inheritance` errors) were all false
//! positives, and both had a single cause of their own:
//!
//!   * variance: the position a type *argument* sits in was read off classes
//!     only. An abstract type member (`type ResultAction[+R, +S, -E]`) and a
//!     higher-kinded type parameter (`F[+X]`) declare variances just the same,
//!     and nsc reads all three off `sym.typeParams`. Treating their arguments
//!     as invariant rejected `def head: ResultAction[T, NoStream, E]`.
//!
//!   * self type: the class being checked was compared as a bare `C`, with its
//!     type arguments dropped, against the parent's self type read *raw* — in
//!     the declaring trait's own vocabulary. slick's `BasicDatabaseDef[F]` says
//!     `this: Database[F] =>`, and `Database` is `BasicBackend`'s abstract
//!     member; from inside `JdbcBackend` it means `JdbcDatabaseDef[F]`.
//!     Nothing conforms to `BasicBackend.Database[F]`, so every cake class
//!     failed.
//!
//! A third, adjacent one showed up while writing the accepting fixture:
//! members reached *through* a parameterized self type kept the self type's
//! own parameters (`trait P[A] { self: Q[A] => def p: A = q }` reported "type
//! mismatch; found: A required: A"), because `subst_as_seen_from` walked a
//! class's parents but not its self type.
//!
//! Every expected string here was read off real scalac 2.13.16 first; see the
//! header of each fixture. The rejecting fixture is kept because loosening a
//! rejection rule is exactly how a rule stops working: `rej_bad.scala` holds
//! the six shapes nsc still refuses.

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
        "scala-rs-rej-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn java_available() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success() || !o.stderr.is_empty() || !o.stdout.is_empty())
        .unwrap_or(false)
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    if cached.is_file() {
        return Some(cached);
    }
    None
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

fn run_java(out: &Path, cp_extra: Option<&Path>) -> String {
    let cp = match cp_extra {
        Some(j) => format!("{}:{}", out.display(), j.display()),
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

fn check_private(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout(name),
            "stdout mismatch for private-runtime {name}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

fn check_library(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not present");
        return;
    };
    let out = compile_fixture_with(name, &["--scala-library", jar.to_str().unwrap()]);
    assert_eq!(
        run_java(&out, Some(&jar)),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn diagnostics(name: &str, extra: &[&str]) -> String {
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
        !output.status.success(),
        "expected compile of {name} to fail ({extra:?})"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let wrote_classes = fs::read_dir(&out)
        .map(|d| {
            d.flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "class"))
        })
        .unwrap_or(false);
    assert!(
        !wrote_classes,
        "{name} was rejected but still wrote classes"
    );
    let _ = fs::remove_dir_all(&out);
    err
}

/// Both modes must reject it: a rule that only fires with the jar on the
/// classpath is not a rule.
fn compile_fails_both(name: &str, needles: &[&str]) {
    let mut modes: Vec<Vec<String>> = vec![vec!["--no-scala-library".to_string()]];
    if let Some(jar) = scala_library_jar() {
        modes.push(vec![
            "--scala-library".to_string(),
            jar.to_str().unwrap().to_string(),
        ]);
    }
    for m in &modes {
        let args: Vec<&str> = m.iter().map(|s| s.as_str()).collect();
        let err = diagnostics(name, &args);
        for needle in needles {
            assert!(
                err.contains(needle),
                "expected {needle:?} in diagnostics for {name} ({args:?}), got {err:?}"
            );
        }
    }
}

/// The accepting side: everything in here compiles and runs under real scalac
/// 2.13.16, and used to draw eleven diagnostics from us.
#[test]
fn fixtures_rej_ok_private_runtime() {
    check_private("rej_ok");
}

#[test]
fn fixtures_rej_ok_scala_library() {
    check_library("rej_ok");
}

/// The rejecting side. Six shapes nsc refuses, one per cause the fix touched.
#[test]
fn fixtures_rej_bad_is_error() {
    compile_fails_both(
        "rej_bad",
        &[
            // `type M[X]` is invariant however the enclosing class is declared.
            "covariant type A occurs in invariant position in type A of return type of f",
            // `type N[-X]` flips the position rather than erasing it.
            "covariant type A occurs in contravariant position in type A of return type of f",
            // Reading the variances off a higher-kinded parameter must not
            // grant `F[X]` a variance it never declared.
            "trait HkInv[F[X], +A]",
            "trait HkFlip[G[-Y], +A]",
            // A self type that now carries type arguments still has to fail
            // when the arguments do not match.
            "illegal inheritance: self-type Miss[A] does not conform to Q[A]",
            // ... and when the cake's alias resolves to a different class.
            "illegal inheritance: self-type Fake[F] does not conform to Real[F]",
        ],
    );
}

/// Exactly six, not five: the checks must not swallow one another.
#[test]
fn rej_bad_reports_every_case() {
    let err = diagnostics("rej_bad", &["--no-scala-library"]);
    assert!(
        err.contains("6 error(s)"),
        "expected six diagnostics for rej_bad, got {err:?}"
    );
}
