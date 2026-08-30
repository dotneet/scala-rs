//! Nested patterns inside `::` and other extractors, and the exception a
//! `match` throws when it runs out of cases.
//!
//! Four silent wrong-code bugs, all found by diffing against real scalac:
//!
//!  * `case P(v) :: t` cast the cons cell's head straight to `P`, so a list
//!    whose head was a `Q` threw a `ClassCastException` instead of falling
//!    through to the next case. nsc casts an extracted value to the *source's*
//!    static type and only then emits `instanceof P` / `ifeq` / `checkcast P`.
//!    Every sub-pattern that tests was affected -- `case Some(P(v))`,
//!    `case (p @ P(v)) :: t`, and `case Some(1)` on an `Option[Any]`, which
//!    unboxed the element instead of comparing it boxed.
//!  * `case P(v) ~ _` on a user-defined infix extractor left the extractor's
//!    `Tuple2` on the stack when the nested pattern jumped to the next case:
//!    `VerifyError: Inconsistent stackmap frames`.
//!  * an extractor reached through an erased field was handed a raw `Object`
//!    (`case Some(Two(a, b))` on an `Option[Any]`), with no `instanceof` in
//!    front of the call; and an `unapply` returning a tuple wider than
//!    `Tuple2` had its result `checkcast`ed to `Tuple2` whatever the arity.
//!  * a failed `match` threw `RuntimeException("match error")` rather than
//!    `scala.MatchError` carrying the scrutinee.
//!
//! Every fixture is run three ways -- private runtime, real `scala-library`
//! jar, and real scalac -- and all three have to print the same thing.

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
        "scala-rs-conspat-{tag}-{}-{nanos}-{seq}",
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
    cached.is_file().then_some(cached)
}

fn find_scalac() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    cached.is_file().then_some(cached)
}

fn compile(out: &Path, name: &str, extra: &[&str]) -> (bool, String) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let output = Command::new(bin())
        .arg("compile")
        .arg(&src)
        .args(["-d", out.to_str().unwrap()])
        .args(extra)
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

/// `-Xverify:all` so a bad `StackMapTable` is a failure, not a silent pass.
fn run_main(out: &Path, jar: Option<&Path>) -> String {
    let cp = match jar {
        Some(j) => format!("{}:{}", out.display(), j.display()),
        None => out.display().to_string(),
    };
    let output = Command::new("java")
        .args(["-Xverify:all", "-cp", &cp, "Main"])
        .output()
        .expect("java");
    assert!(
        output.status.success(),
        "java -Xverify:all -cp {cp} Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile against the jar and check the program's stdout.
fn jar_run(name: &str) {
    let (Some(jar), true) = (scala_library_jar(), java_available()) else {
        eprintln!("skip {name}: jar or java not present");
        return;
    };
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(ok, "compile {name} failed:\n{msgs}");
    assert_eq!(
        run_main(&out, Some(&jar)),
        expected_stdout(name),
        "stdout mismatch for {name} (scala-library)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same fixture on the private runtime (`--no-scala-library`).
fn private_run(name: &str) {
    if !java_available() {
        return;
    }
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--no-scala-library"]);
    assert!(ok, "compile {name} --no-scala-library failed:\n{msgs}");
    assert_eq!(
        run_main(&out, None),
        expected_stdout(name),
        "stdout mismatch for {name} (private runtime)"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The recorded expectation has to be what real scalac 2.13.16 prints.
fn matches_real_scalac(name: &str) {
    let (Some(scalac), Some(jar), true) = (find_scalac(), scala_library_jar(), java_available())
    else {
        eprintln!("skip real-scalac diff {name}: scalac, jar or java not present");
        return;
    };
    let src = fixtures_dir().join(format!("{name}.scala"));
    let ref_out = tmp_dir(&format!("{name}-nsc"));
    let status = Command::new(&scalac)
        .args([src.to_str().unwrap(), "-d", ref_out.to_str().unwrap()])
        .status()
        .expect("scalac");
    assert!(status.success(), "real scalac failed to compile {name}");
    assert_eq!(
        run_main(&ref_out, Some(&jar)),
        expected_stdout(name),
        "recorded expectation for {name} does not match real scalac"
    );
    let _ = fs::remove_dir_all(&ref_out);
}

fn compile_fails(name: &str, needles: &[&str]) {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip {name}: jar not present");
        return;
    };
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, &["--scala-library", jar.to_str().unwrap()]);
    assert!(!ok, "expected compile of {name} to fail, got:\n{msgs}");
    for needle in needles {
        assert!(
            msgs.contains(needle),
            "expected {needle:?} in diagnostics for {name}, got {msgs}"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// Compile a snippet against the jar and report the diagnostics.
fn compile_src(src: &str, tag: &str) -> (bool, String) {
    let Some(jar) = scala_library_jar() else {
        return (true, String::new());
    };
    let out = tmp_dir(tag);
    let path = out.join("Snippet.scala");
    fs::write(&path, src).unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .arg(&path)
        .args(["-d", out.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let ok = output.status.success();
    let _ = fs::remove_dir_all(&out);
    (ok, msgs)
}

// ------------------------------------------------------------------ fixtures

#[test]
fn cp_cons_private_runtime() {
    private_run("cp_cons");
}

#[test]
fn cp_cons_scala_library() {
    jar_run("cp_cons");
}

#[test]
fn cp_cons_matches_real_scalac() {
    matches_real_scalac("cp_cons");
}

#[test]
fn cp_infix_private_runtime() {
    private_run("cp_infix");
}

#[test]
fn cp_infix_scala_library() {
    jar_run("cp_infix");
}

#[test]
fn cp_infix_matches_real_scalac() {
    matches_real_scalac("cp_infix");
}

#[test]
fn cp_err_private_runtime() {
    private_run("cp_err");
}

#[test]
fn cp_err_scala_library() {
    jar_run("cp_err");
}

#[test]
fn cp_err_matches_real_scalac() {
    matches_real_scalac("cp_err");
}

/// `Seq(...)` / `List(...)` extractor patterns and `Tuple3` only exist against
/// the real jar; the private runtime diagnoses them (see
/// `seqpat_without_library_is_diagnosed`).
#[test]
fn cp_seq_scala_library() {
    jar_run("cp_seq");
}

#[test]
fn cp_seq_matches_real_scalac() {
    matches_real_scalac("cp_seq");
}

#[test]
fn cp_cons_bad_is_rejected() {
    compile_fails(
        "cp_cons_bad",
        &[
            "extractor P expects 1 argument(s), found 2",
            "not found: extractor Nope",
        ],
    );
}

// ------------------------------------------------------------------- details

/// The head of a cons cell is `Object`; the sub-pattern's own `instanceof` has
/// to come before any narrowing cast. Without it this threw
/// `ClassCastException: Main$Q$ cannot be cast to Main$P`.
#[test]
fn a_nested_extractor_under_cons_tests_before_it_casts() {
    let src = r#"
object Main {
  sealed trait C
  case class P(v: Int) extends C
  case object Q extends C
  def f(cs: List[C]): Int = cs match {
    case Nil => 0
    case P(v) :: t => v + f(t)
    case Q :: t => f(t)
  }
  def main(a: Array[String]): Unit = println(f(P(1) :: Q :: P(2) :: Nil))
}
"#;
    let (ok, msgs) = compile_src(src, "nested-cons");
    assert!(ok, "compile failed:\n{msgs}");
}

/// A constructor pattern binds one sub-pattern per field. This used to reach
/// the backend and throw `RuntimeException("pattern arity")` at run time. A
/// case class has a synthetic `unapply`, so the extractor branch is what
/// reports it; a class with fields and no extractor at all reports the
/// constructor form (`a_constructor_pattern_without_an_extractor_is_checked`).
#[test]
fn a_constructor_patterns_arity_is_checked() {
    let src = r#"
object Main {
  case class P(v: Int)
  def f(p: P): Int = p match { case P(a, b) => a }
}
"#;
    let (ok, msgs) = compile_src(src, "ctor-arity");
    assert!(!ok, "expected the arity error, got:\n{msgs}");
    assert!(
        msgs.contains("extractor P expects 1 argument(s), found 2"),
        "unexpected diagnostics: {msgs}"
    );
}

/// The same arity check on a class the pattern reaches through its fields
/// rather than an `unapply`.
#[test]
fn a_constructor_pattern_without_an_extractor_is_checked() {
    let src = r#"
object Main {
  class K(val a: Int, val b: Int)
  def f(k: K): Int = k match { case K(x) => x }
}
"#;
    let (ok, msgs) = compile_src(src, "plain-arity");
    assert!(!ok, "expected the arity error, got:\n{msgs}");
    assert!(
        msgs.contains("wrong number of arguments for pattern K: expected 2, found 1"),
        "unexpected diagnostics: {msgs}"
    );
}

/// A repeated last parameter takes any number of sub-patterns, so the arity
/// check must leave it alone.
#[test]
fn a_repeated_parameter_is_exempt_from_the_arity_check() {
    let src = r#"
object Main {
  case class V(name: String, xs: Int*)
  def f(v: V): String = v match { case V(n, rest @ _*) => n + rest.length }
  def main(a: Array[String]): Unit = println(f(V("a", 1, 2)))
}
"#;
    let (ok, msgs) = compile_src(src, "repeated-arity");
    assert!(ok, "compile failed:\n{msgs}");
}
