//! E2E tests for the `agent/tuplelit` slice: **a tuple literal is
//! `scala.TupleN`, not a name to be resolved.**
//!
//! The parser lowers `(a, b)` to `Apply(Ident("Tuple2"), …)`, and the typer
//! then resolved `Tuple2` like any other name. nsc never does that:
//! `gen.mkTuple` builds a fully qualified `scala.TupleN` tree, so a *term* of
//! that name in scope cannot capture the literal. `scala.math.Ordering` and
//! `scala.math.Equiv` each declare `implicit def Tuple2[T1, T2](…)` and write
//! tuple literals in their own bodies, which is why this shows up compiling
//! scala/scala's `src/library`.
//!
//! The marker is `Tree::scala_ref`, set by the parser on the `Ident` it makes
//! up (and by the two places in `check.rs` that synthesize one: auto-tupling
//! of an argument list, and a `for` generator's destructuring pattern). A
//! marked `Ident` is resolved by `SymbolTable::lookup_scala`, a member lookup
//! in package `scala` rather than a lexical one.
//!
//! It is deliberately narrow. Not every synthesized name is qualified in nsc:
//! string interpolation really does emit a bare `StringContext`, and scalac
//! 2.13.16 reports `value s is not a member of String` for `s"…"` written
//! where a `def StringContext` is in scope. `real_scalac_*` below pins the
//! part that is qualified.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `tuplelit` prefix.

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
        "scala-rs-tuplelit-{tag}-{}-{nanos}-{seq}",
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
        "java -Xverify:all Main failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile a snippet against the real scala-library jar, which is the mode
/// every other benchmark uses. Returns the diagnostics whether or not the
/// compile succeeded.
fn compile_jar(tag: &str, src: &str) -> (bool, String) {
    let Some(jar) = scala_library_jar() else {
        return (true, String::new());
    };
    let dir = tmp_dir(tag);
    let file = dir.join(format!("{tag}.scala"));
    fs::write(&file, src).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let output = Command::new(bin())
        .args([
            "compile",
            file.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let ok = output.status.success();
    let _ = fs::remove_dir_all(&dir);
    (ok, text)
}

// ---------------------------------------------- the literal is not captured

/// The reproduction from `docs/scala-library.md`, which real scalac 2.13.16
/// accepts: a `def Tuple2` in scope, and a tuple literal in the same body.
#[test]
fn a_term_named_tuple2_does_not_capture_a_tuple_literal() {
    let (ok, err) = compile_jar(
        "expr",
        r#"
object Fake {
  def Tuple2(n: Int): String = "" + n
  def f[A, B](a: A, b: B): (A, B) = (a, b)
}
"#,
    );
    assert!(ok, "`(a, b)` should still be scala.Tuple2:\n{err}");
}

/// The same in *pattern* position. `Equiv.scala:251` is `(x, y) match`, and
/// it reported `not found: extractor Tuple2` — `type_pattern` looks the
/// pattern's class up with the namespace-blind `lookup`, which stops at the
/// first scope binding the name at all.
#[test]
fn a_term_named_tuple2_does_not_capture_a_tuple_pattern() {
    let (ok, err) = compile_jar(
        "pattern",
        r#"
object Fake {
  def Tuple2(n: Int): String = "" + n
  def g(x: Int, y: Int): Int = (x, y) match { case (a, b) => a + b }
}
"#,
    );
    assert!(ok, "`case (a, b)` should still be scala.Tuple2:\n{err}");
}

/// A `for` generator over a tuple pattern is lowered to a pattern-matching
/// anonymous function whose selector `check.rs` synthesizes itself, so it
/// needs the marker too.
#[test]
fn a_term_named_tuple2_does_not_capture_a_for_generator() {
    let (ok, err) = compile_jar(
        "forcomp",
        r#"
object Fake {
  def Tuple2(n: Int): String = "" + n
  def h(xs: List[(Int, Int)]): List[Int] = for ((a, b) <- xs) yield a + b
}
"#,
    );
    assert!(ok, "`for ((a, b) <- xs)` should still work:\n{err}");
}

/// Higher arities go through the same path, and the fix must not depend on
/// the prelude having built a companion for that arity by hand.
#[test]
fn the_rule_holds_for_higher_arities() {
    let (ok, err) = compile_jar(
        "arity",
        r#"
object Fake {
  def Tuple3(n: Int): String = "" + n
  def Tuple5(n: Int): String = "" + n
  def t3[A, B, C](a: A, b: B, c: C): (A, B, C) = (a, b, c)
  def t5(a: Int, b: Int, c: Int, d: Int, e: Int): Int =
    (a, b, c, d, e) match { case (p, q, r, s, t) => p + q + r + s + t }
}
"#,
    );
    assert!(ok, "arities other than 2 need the same rule:\n{err}");
}

/// The narrow half of the rule: an `Ident` the *source* writes is resolved the
/// ordinary way, so an explicit `Tuple2(1)` still calls the method. Losing
/// this would be a silent miscompile, not a diagnostic, which is why
/// `tuplelit_shadow` below checks it by running the program.
#[test]
fn an_explicit_tuple2_call_still_names_the_method() {
    let (ok, err) = compile_jar(
        "explicit",
        r#"
object Fake {
  def Tuple2(n: Int): String = "" + n
  // Only the method takes one `Int`; `scala.Tuple2.apply` takes two.
  def f: String = Tuple2(1)
}
"#,
    );
    assert!(ok, "an explicit call is an ordinary name:\n{err}");
}

/// The same family, found while surveying the parser's synthesized names: a
/// repeated parameter's `Seq[T]` was looked up with `lookup` as well, so a
/// `def Seq` in scope left the parameter as the bare `T*` and every member
/// selection on it failed.
#[test]
fn a_term_named_seq_does_not_capture_a_repeated_parameter() {
    let (ok, err) = compile_jar(
        "repeated",
        r#"
object Fake {
  def Seq(n: Int): String = "" + n
  def f(xs: Int*): Int = xs.length
}
"#,
    );
    assert!(ok, "`xs: Int*` should still widen to Seq[Int]:\n{err}");
}

// ------------------------------------------------------- run the whole thing

/// Compile and run `tests/fixtures/tuplelit_shadow.scala`. Nothing else here
/// executes code, and the `explicit` case above can only be told apart from a
/// wrong fix at run time.
#[test]
fn fixtures_tuplelit_shadow() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let src = fixtures_dir().join("tuplelit_shadow.scala");
    let out = tmp_dir("tuplelit_shadow");
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
        "compile tuplelit_shadow failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("tuplelit_shadow")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source through **real scalac**, so the expected output is pinned
/// to what Scala 2.13.16 actually prints.
#[test]
fn real_scalac_dual_run_tuplelit_shadow() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip real-scalac dual-run: jar or scalac not obtainable");
        return;
    };
    let dir = tmp_dir("tuplelit_shadow-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("tuplelit_shadow.scala"))
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac rejected tuplelit_shadow:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        run_java(&dir, Some(jar.to_str().unwrap())),
        expected_stdout("tuplelit_shadow"),
    );
    let _ = fs::remove_dir_all(&dir);
}
