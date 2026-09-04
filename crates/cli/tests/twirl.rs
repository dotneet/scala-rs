//! E2E tests for the `agent/twirl` slice: what a Twirl-generated template
//! asks of a class read from a jar.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. The miniature library is `twlib`, modelled on
//! `play.twirl.api` — see `docs/gitbucket.md`.
//!
//! Every one of gitbucket's 140 generated templates is
//!
//! ```scala
//! object x extends BaseScalaTemplate[HtmlFormat.Appendable,
//!                                    Format[HtmlFormat.Appendable]](HtmlFormat)
//!     with Template4[…, HtmlFormat.Appendable] {
//!   def apply(…): HtmlFormat.Appendable = _display_ { … }
//! }
//! ```
//!
//! and 707 of the 3261 diagnostics in one gitbucket measurement came from
//! **two** gaps in that shape, not the one the survey guessed at:
//!
//! 1. **`p.T` reached the pickle only inside a parents clause.** The pickled
//!    alias `type Appendable = Output` on `Format[Output]` already comes back
//!    correctly substituted (`Html`), so `install_type_alias` was not the
//!    problem — but `Check::tree_to_type` only consulted
//!    `qualified_pickled_type_member` under `strict_type_names`, which is on
//!    for a parent and off for an ordinary signature. So `HtmlFormat
//!    .Appendable` resolved in the `extends` clause and stayed the
//!    placeholder `Type::Named("Appendable")` in the very next line's
//!    `def apply(…): HtmlFormat.Appendable`. Only the *diagnostic* is strict
//!    now; the lookup runs either way.
//!
//! 2. **An unqualified *overloaded* inherited member was not read
//!    as-seen-from.** `Check::bind_found`'s single-alternative path applies
//!    `subst_as_seen_from` through the enclosing class; the overload path
//!    built `Type::Overload` out of the raw symbol types, so
//!    `BaseScalaTemplate`'s six `_display_` alternatives all came back
//!    returning the bare `T` instead of `Html`
//!    (`type mismatch; found: T  required: Appendable`, and
//!    `ambiguous overload for _display_` where the un-substituted
//!    alternatives could not be told apart). The instantiated types have to
//!    be filed under `overload_member_types` as well, because
//!    `resolve_overload_with` rebuilds its candidate list from the symbols.
//!
//! The two are independent: `a_pickled_alias_and_an_inherited_overload`
//! fails on either one alone.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-twirl-{tag}-{}-{nanos}-{seq}",
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

fn jar_tool() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = PathBuf::from(home).join("bin/jar");
        if p.is_file() {
            return Some(p);
        }
    }
    let out = Command::new("which").arg("jar").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
    p.is_file().then_some(p)
}

fn pack_jar(classes: &Path, dest: &Path) {
    let tool = jar_tool().expect("jar tool");
    let out = Command::new(tool)
        .args([
            "cf",
            dest.to_str().unwrap(),
            "-C",
            classes.to_str().unwrap(),
        ])
        .arg(".")
        .output()
        .expect("run jar");
    assert!(
        out.status.success(),
        "jar failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run_java(out: &Path, cp_extra: &str) -> String {
    let cp = format!("{}:{}", out.display(), cp_extra);
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

fn compile_against(out: &Path, jar: &Path, src: &Path, cp: &Path) -> (bool, String) {
    let output = Command::new(bin())
        .arg("compile")
        .arg(src)
        .args(["-d", out.to_str().unwrap()])
        .args(["-cp", cp.to_str().unwrap()])
        .args(["--scala-library", jar.to_str().unwrap()])
        .output()
        .expect("run scala-rs compile");
    let msgs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    (output.status.success(), msgs)
}

/// `play.twirl.api` in miniature, with the three features the templates need:
/// a **nullary type alias whose right-hand side is a type parameter**
/// (`Format[Output] { type Appendable = Output }`, made concrete by
/// `object HtmlFormat extends Format[Html]`), a base class whose members
/// return that parameter, and an **overloaded** one of them
/// (`BaseTemplate._display_`) that a subclass calls unqualified.
///
/// It only reproduces from a class file: a `type` alias leaves no trace in
/// the bytecode, so this all rests on the `ScalaSignature` pickle.
const TW_LIB: &str = r#"
package twlib

trait Appendable[T] {
  def add(other: T): T
}

trait Format[Output <: Appendable[Output]] {
  type Appendable = Output
  def raw(text: String): Output
  def empty: Output
}

class Html(val body: String) extends Appendable[Html] {
  def add(other: Html): Html = new Html(body + other.body)
}

object HtmlFormat extends Format[Html] {
  def raw(text: String): Html = new Html(text)
  def empty: Html = new Html("")
}

class BaseTemplate[T <: Appendable[T], F <: Format[T]](val format: F) {
  def _display_(o: Any): T = format.raw(String.valueOf(o))
  def _display_(s: String): T = format.raw(s)
  def _display_(n: Int): T = format.raw(n.toString)
}

trait Template1[A, Result] {
  def render(a: A): Result
}
"#;

/// The shape sbt-twirl generates, minus the position comments: the alias in
/// the parents clause *and* in every signature, and `_display_` called
/// unqualified from inside the subclass.
const TW_USER: &str = r#"
object mytpl
    extends twlib.BaseTemplate[
      twlib.HtmlFormat.Appendable,
      twlib.Format[twlib.HtmlFormat.Appendable]
    ](twlib.HtmlFormat)
    with twlib.Template1[String, twlib.HtmlFormat.Appendable] {

  def apply(x: String): twlib.HtmlFormat.Appendable = _display_(x)

  def render(x: String): twlib.HtmlFormat.Appendable = apply(x)

  def number(n: Int): twlib.HtmlFormat.Appendable = _display_(n)
}

object Main {
  def main(args: Array[String]): Unit = {
    println(mytpl("hello").body)
    println(mytpl.render("rendered").body)
    println(mytpl.number(7).body)
    println(mytpl.format.empty.add(mytpl("tail")).body)
  }
}
"#;

/// Resolving the alias is not the same as accepting anything under its name:
/// `HtmlFormat.Appendable` is `Html`, so a `String` does not conform. Real
/// scalac reports the same, spelling it "(which expands to) twlib.Html".
const TW_USER_BAD: &str = r#"
object badtpl
    extends twlib.BaseTemplate[
      twlib.HtmlFormat.Appendable,
      twlib.Format[twlib.HtmlFormat.Appendable]
    ](twlib.HtmlFormat) {

  def apply(x: String): twlib.HtmlFormat.Appendable = x
}
"#;

const TW_EXPECTED: &str = "hello\nrendered\n7\ntail\n";

fn build_lib_jar(dir: &Path) -> PathBuf {
    let src = dir.join("lib.scala");
    fs::write(&src, TW_LIB).unwrap();
    let lib_out = dir.join("libout");
    fs::create_dir_all(&lib_out).unwrap();
    let scalac = self::scalac().expect("checked by caller");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&lib_out)
        .arg(&src)
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac failed on the miniature library:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lib_jar = dir.join("twlib.jar");
    pack_jar(&lib_out, &lib_jar);
    lib_jar
}

/// A pickled nullary alias used outside a parents clause, and an overloaded
/// inherited member called unqualified, both read through the class's own
/// type arguments.
#[test]
fn a_pickled_alias_and_an_inherited_overload() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    if scalac().is_none() {
        eprintln!("skip: no scalac to write the pickle with");
        return;
    }
    if jar_tool().is_none() {
        eprintln!("skip: no `jar` tool");
        return;
    }
    let dir = tmp_dir("alias");
    let lib_jar = build_lib_jar(&dir);

    let user = dir.join("user.scala");
    fs::write(&user, TW_USER).unwrap();
    let user_out = dir.join("userout");
    fs::create_dir_all(&user_out).unwrap();
    let (ok, msgs) = compile_against(&user_out, &jar, &user, &lib_jar);
    assert!(
        ok,
        "the template failed to compile against the jar:\n{msgs}"
    );
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    let bad = dir.join("bad.scala");
    fs::write(&bad, TW_USER_BAD).unwrap();
    let bad_out = dir.join("badout");
    fs::create_dir_all(&bad_out).unwrap();
    let (ok, msgs) = compile_against(&bad_out, &jar, &bad, &lib_jar);
    assert!(
        !ok,
        "expected `String` not to conform to the alias's right-hand side:\n{msgs}"
    );
    assert!(
        msgs.contains("required: Html"),
        "expected the alias to be reported as the `Html` it expands to, got:\n{msgs}"
    );

    if java_available() {
        let cp = format!("{}:{}", jar.display(), lib_jar.display());
        assert_eq!(
            run_java(&user_out, &cp),
            TW_EXPECTED,
            "stdout mismatch for a_pickled_alias_and_an_inherited_overload"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The fixture is ordinary Scala, not a quirk of ours: real scalac compiles
/// the same two files and prints the same thing.
#[test]
fn real_scalac_accepts_the_same_template() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let Some(scalac) = scalac() else {
        eprintln!("skip: no scalac");
        return;
    };
    if !java_available() {
        eprintln!("skip: no java");
        return;
    }
    let dir = tmp_dir("alias-scalac");
    let lib = dir.join("lib.scala");
    let user = dir.join("user.scala");
    fs::write(&lib, TW_LIB).unwrap();
    fs::write(&user, TW_USER).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let result = Command::new(&scalac)
        .arg("-d")
        .arg(&out)
        .args([&lib, &user])
        .output()
        .expect("run scalac");
    assert!(
        result.status.success(),
        "scalac rejected the fixture:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(run_java(&out, jar.to_str().unwrap()), TW_EXPECTED);
    let _ = fs::remove_dir_all(&dir);
}
