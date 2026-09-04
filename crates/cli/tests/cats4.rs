//! E2E tests for the `agent/cats` slice: what stopped typelevel/cats' `kernel`
//! and `core` sources before any of them could be typed.
//!
//! 1. **`$` was not a letter.** nsc's `Chars.isIdentifierStart` accepts it, so
//!    `implicit ev$1: Defer[G]` -- which cats' checked-in simulacrum output
//!    writes -- is one identifier. The lexer rejected the character outright,
//!    and 13 core sources produced 47 `unexpected character '$'` errors.
//! 2. **Type parameters could not carry annotations.** nsc's grammar is
//!    `TypeParam ::= {Annotation} [`+' | `-'] ...`; cats-kernel writes
//!    `trait Eq[@sp A]` on 26 traits, and each one was a parse cascade.
//! 3. **`@tailrec` on a def nested in a method was rejected** as "neither
//!    private nor final so can be overridden". A local def is not a member of
//!    anything; cats writes one inside `tailRecM` 79 times.
//! 4. **A package written out in an expression had no members.**
//!    `cats.kernel.instances.int.catsKernelStdOrderForInt` selects through a
//!    package object; only the *import* path knew to look there, so the
//!    expression reported `value ... is not a member of <notype>` (161 times).
//! 5. **Expanding an abstract type member's alias could not terminate.** See
//!    `tests/fixtures/c4_alias.scala`: cats' `Representable#compose` made the
//!    whole run die with `fatal runtime error: stack overflow`, so 244 sources
//!    produced no diagnostics at all.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `c4` prefix.

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
        "scala-rs-cats4-{tag}-{}-{nanos}-{seq}",
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

/// `c4_lang.scala` imports `scala.annotation.tailrec`, which the private
/// runtime does not supply, so this fixture is `--scala-library` only (same as
/// `cats_lambda.scala`).
#[test]
fn fixtures_c4_lang() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let src = fixtures_dir().join("c4_lang.scala");
    let out = tmp_dir("lang");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile c4_lang failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar_s), "Main"),
        expected_stdout("c4_lang")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The expected output is nsc's, not this compiler's idea of it.
#[test]
fn scalac_agrees_c4_lang_output() {
    let (Some(sc), true) = (scalac(), java_available()) else {
        eprintln!("skip: scalac or java not available");
        return;
    };
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let out = tmp_dir("scalac-lang");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("c4_lang.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected c4_lang:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap()), "Main"),
        expected_stdout("c4_lang")
    );
    let _ = fs::remove_dir_all(&out);
}

/// Terminating is the whole point: a stack overflow kills the process before
/// any diagnostic is written, which is how 244 cats-core sources reported
/// nothing at all. This does not assert that the file compiles -- it does not
/// yet (see the fixture's header) -- only that the compiler answers.
#[test]
fn c4_alias_terminates_with_a_diagnostic() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let out = tmp_dir("alias");
    let output = Command::new(bin())
        .args([
            "compile",
            fixtures_dir().join("c4_alias.scala").to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !err.contains("overflowed its stack") && !err.contains("stack overflow"),
        "c4_alias overflowed the stack again:\n{err}"
    );
    assert!(
        output.status.code().is_some(),
        "c4_alias died on a signal rather than reporting: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Real scalac 2.13.16 accepts `c4_alias.scala`, so the gap it pins is ours
/// and not a property of the code.
#[test]
fn scalac_accepts_c4_alias() {
    let Some(sc) = scalac() else {
        eprintln!("skip: scalac not available");
        return;
    };
    let out = tmp_dir("scalac-alias");
    let output = Command::new(sc)
        .args(["-d", out.to_str().unwrap()])
        .arg(fixtures_dir().join("c4_alias.scala"))
        .output()
        .expect("run scalac");
    assert!(
        output.status.success(),
        "scalac rejected c4_alias:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&out);
}
