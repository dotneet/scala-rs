//! The collections library's infix cons operators and extractor objects.
//!
//! `case h :: t` had always worked -- `scala.::` is a case class, so the
//! pattern goes through the constructor-pattern path. Its three siblings are
//! plain objects with an `unapply` and none of them was in the symbol table:
//!
//! ```text
//! scala/collection/package$$plus$colon$.unapply:(Lscala/collection/SeqOps;)Lscala/Option;
//! scala/collection/package$$colon$plus$.unapply:(Lscala/collection/SeqOps;)Lscala/Option;
//! scala/package$$hash$colon$colon$.unapply:(Lscala/collection/immutable/LazyList;)Lscala/Option;
//! scala/package$$hash$colon$colon$.unapply:(Lscala/collection/immutable/Stream;)Lscala/Option;
//! ```
//!
//! so `case h +: t` / `case t :+ h` / `case h #:: t` all reported "not found:
//! extractor", and every name such a pattern bound cascaded into a second
//! error. The expression side (`a #:: xs`, `xs #::: ys`) was missing too: it
//! goes through an implicit conversion whose parameter is *by name*, and a
//! by-name conversion was invisible to the view search.
//!
//! Four things are checked here, each of which was independently broken:
//!
//!  * the extractors resolve, and the container they bind is the scrutinee's
//!    own (an `ArraySeq` stays an `ArraySeq`) -- `unapply[A, C <: Seq[A]]`,
//!    with `A` solved through `C`'s bound;
//!  * `scala.#::`'s two alternatives are told apart by the scrutinee, and a
//!    scrutinee that is neither a `LazyList` nor a `Stream` is *rejected*
//!    rather than bound at whichever alternative came first;
//!  * `a #:: xs` forces neither side (`Main.ones` in the fixture is infinite);
//!  * a class that declares a method named `::` does not thereby hide
//!    `scala.::` from the `case h :: t` patterns in its own body.
//!
//! Every runnable fixture is diffed against real scalac 2.13.16.

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
        "scala-rs-catstail-{tag}-{}-{nanos}-{seq}",
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

fn compile_fails(name: &str, extra: &[&str], needles: &[&str]) {
    let out = tmp_dir(name);
    let (ok, msgs) = compile(&out, name, extra);
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

/// `+:` / `:+` / `#::` in patterns, `#::` / `#:::` in expressions. Jar only:
/// none of these objects exists on the private runtime, which diagnoses the
/// patterns instead (`the_private_runtime_diagnoses_the_cons_extractors`).
#[test]
fn ct2_conscoll_scala_library() {
    jar_run("ct2_conscoll");
}

#[test]
fn ct2_conscoll_matches_real_scalac() {
    matches_real_scalac("ct2_conscoll");
}

#[test]
fn ct2_consshadow_scala_library() {
    jar_run("ct2_consshadow");
}

#[test]
fn ct2_consshadow_private_runtime() {
    private_run("ct2_consshadow");
}

#[test]
fn ct2_consshadow_matches_real_scalac() {
    matches_real_scalac("ct2_consshadow");
}

/// Real scalac rejects both patterns in this fixture: "scrutinee is
/// incompatible with pattern type" for the `Int`, "cannot resolve overloaded
/// unapply" for the `List`.
#[test]
fn ct2_conscoll_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip ct2_conscoll_bad: jar not present");
        return;
    };
    compile_fails(
        "ct2_conscoll_bad",
        &["--scala-library", jar.to_str().unwrap()],
        &["type mismatch; found: A  required: Int", "extractor #::"],
    );
}

// ------------------------------------------------------------------- details

/// The private runtime has no `scala.collection.+:` and no `scala.#::`, so the
/// pattern has to be diagnosed there rather than quietly compiled into a call
/// to a class that does not exist.
#[test]
fn the_private_runtime_diagnoses_the_cons_extractors() {
    let out = tmp_dir("cons-private");
    let path = out.join("Snippet.scala");
    fs::write(
        &path,
        "object Main {\n  def f(xs: List[Int]): Int = xs match {\n    case h +: _ => h\n    case _ => 0\n  }\n}\n",
    )
    .unwrap();
    let output = Command::new(bin())
        .arg("compile")
        .arg(&path)
        .args(["-d", out.to_str().unwrap()])
        .arg("--no-scala-library")
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !output.status.success() && msgs.contains("not found: extractor +:"),
        "expected the private runtime to reject `+:`, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The head sub-pattern's type comes from `C`'s *bound*: nothing in
/// `unapply[A, C <: Seq[A]](t: C)`'s parameter mentions `A`. Before the bound
/// was consulted `h` stayed an unresolved `A` and `h + 1` picked `String.+`.
#[test]
fn the_head_of_a_plus_colon_pattern_has_the_elements_type() {
    let src = r#"
object Main {
  def f(xs: Seq[Int]): Int = xs match {
    case h +: _ => h + 1
    case _      => 0
  }
  def main(a: Array[String]): Unit = println(f(Seq(1)))
}
"#;
    let (ok, msgs) = compile_src(src, "plus-colon-head");
    assert!(ok, "compile failed:\n{msgs}");
}

/// And the tail keeps the scrutinee's own container, which is what an
/// `unapply` declared to yield `Seq[A]` would have thrown away.
#[test]
fn the_tail_of_a_plus_colon_pattern_keeps_the_scrutinees_container() {
    let src = r#"
import scala.collection.immutable.ArraySeq
object Main {
  def f(xs: ArraySeq[Int]): ArraySeq[Int] = xs match {
    case _ +: rest => rest
    case _         => xs
  }
  def g(xs: LazyList[Int]): LazyList[Int] = xs match {
    case _ +: rest => rest
    case _         => xs
  }
}
"#;
    let (ok, msgs) = compile_src(src, "plus-colon-tail");
    assert!(ok, "compile failed:\n{msgs}");
}

/// `scala.#::` is overloaded. Taking the first alternative bound the tail of a
/// `Stream` pattern at `LazyList`, which type-checked as far as the next call
/// and then named a method the `Stream` does not have.
#[test]
fn a_hash_colon_colon_pattern_on_a_stream_binds_a_stream() {
    let src = r#"
object Main {
  def s(xs: Stream[Int]): Int = xs match {
    case v #:: t => v + s(t)
    case _       => 0
  }
  def l(xs: LazyList[Int]): Int = xs match {
    case v #:: t => v + l(t)
    case _       => 0
  }
}
"#;
    let (ok, msgs) = compile_src(src, "hash-cons-overload");
    assert!(ok, "compile failed:\n{msgs}");
}

/// A conversion whose parameter is by name is still a conversion. Without this
/// the view search skipped `LazyList.toDeferrer` outright and `a #:: xs` was
/// "value #:: is not a member of LazyList[Int]".
#[test]
fn a_by_name_implicit_conversion_is_found() {
    let src = r#"
object Main {
  class Wrap(val n: () => Int) extends AnyVal { def twice: Int = n() * 2 }
  implicit def toWrap(n: => Int): Wrap = new Wrap(() => n)
  def main(a: Array[String]): Unit = println(3.twice)
}
"#;
    let (ok, msgs) = compile_src(src, "byname-view");
    assert!(ok, "compile failed:\n{msgs}");
}
