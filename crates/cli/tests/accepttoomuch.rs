//! E2E tests for the `agent/accepttoomuch` slice: **programs scalac rejects
//! and scala-rs compiled without a word.**
//!
//! Three holes, all found by scoring `tests/scala_corpus.sh`'s `neg` against
//! the `.check` text (see `docs/scala-corpus.md`):
//!
//! * a written type annotation naming nothing. `def f(x: Zork)`, `val x: Zork`
//!   and `def f(x: Int): Zork` all compiled — only a template's parents, its
//!   self type and `new` resolved strictly. `type_val_sig` and `type_def_sig`
//!   now do too.
//! * a local `type` alias had no symbol at all: a block ran the namer over its
//!   `class`/`object` statements only. Harmless while an unresolved name in a
//!   signature was tolerated, fatal once it is not.
//! * two overloads erasure merges into one descriptor, nsc's
//!   `RefChecks.checkNoDoubleDefs`.
//!
//! Each rejection here was probed against real scalac 2.13.16 first, and so
//! was each **acceptance**: this is a rejection rule, and the acceptances are
//! the half that breaks working code when it is wrong. The four project
//! measurements (slick, cats, gitbucket, scala/library) are the other half.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `accepttoomuch` prefix.

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
        "scala-rs-accepttoomuch-{tag}-{}-{nanos}-{seq}",
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

/// Compile one file against the real scala-library jar. Returns whether the
/// compile succeeded and everything it printed.
fn compile_file(tag: &str, file: &Path) -> (bool, String) {
    let Some(jar) = scala_library_jar() else {
        return (true, String::new());
    };
    let dir = tmp_dir(tag);
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

fn compile_src(tag: &str, src: &str) -> (bool, String) {
    let dir = tmp_dir(tag);
    let file = dir.join(format!("{tag}.scala"));
    fs::write(&file, src).unwrap();
    let r = compile_file(tag, &file);
    let _ = fs::remove_dir_all(&dir);
    r
}

/// What real scalac 2.13.16 says about `src`, or `None` when it is not
/// installed. Every rule below is pinned against this rather than guessed.
fn scalac_says(tag: &str, src: &str) -> Option<(bool, String)> {
    let scalac = scalac()?;
    let dir = tmp_dir(tag);
    let file = dir.join(format!("{tag}.scala"));
    fs::write(&file, src).unwrap();
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(&file)
        .output()
        .expect("run scalac");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let ok = out.status.success();
    let _ = fs::remove_dir_all(&dir);
    Some((ok, text))
}

// ------------------------------------------- an unresolved name in a signature

#[test]
fn an_unknown_type_in_a_signature_is_reported() {
    let (ok, err) = compile_file("bad", &fixtures_dir().join("accepttoomuch_alias_bad.scala"));
    if err.is_empty() {
        return; // no jar
    }
    assert!(!ok, "the fixture must be rejected:\n{err}");
    assert_eq!(
        err.matches("not found: type Zork").count(),
        4,
        "one diagnostic per annotation, as scalac reports:\n{err}"
    );
}

/// The one thing the rule must not do: report a name it simply cannot see.
/// A wildcard import whose members this compiler cannot enumerate leaves the
/// scope open, and gitbucket writes 259 signatures under one
/// (`import ...Profile.profile.blockingApi._`, then `implicit s: Session`).
#[test]
fn a_wildcard_import_this_compiler_cannot_expand_switches_the_rule_off() {
    let (ok, err) = compile_src(
        "opaque",
        r#"
object O {
  val b = new StringBuilder
  import b._
  def f(x: SomethingOnlyThatImportCouldHaveBrought): Int = 3
}
"#,
    );
    if err.is_empty() {
        return;
    }
    assert!(
        ok,
        "a name under an unexpandable wildcard import proves nothing:\n{err}"
    );
}

/// An existential quantifies its own names, and they stay `Type::Named`
/// placeholders until `subst_quantified` binds them. `pos/exbound`,
/// `pos/depexists`, `pos/t0905`, `pos/t1048`, `pos/t1560` and `pos/t5022` are
/// all this shape and all regressed when the rule first went in.
#[test]
fn an_existential_binds_its_own_names() {
    let (ok, err) = compile_src(
        "exist",
        r#"
class A[T <: A[T]]
object O {
  val x: A[X] forSome { type X } = null
  val y: Option[(a, b)] forSome { type a <: Number; type b <: (a, a) } = null
}
"#,
    );
    if err.is_empty() {
        return;
    }
    assert!(ok, "`forSome` names are bound, not missing:\n{err}");
}

// ------------------------------------------------------ local `type` aliases

#[test]
fn a_local_type_alias_is_in_scope_for_the_whole_block() {
    let (ok, err) = compile_file("alias", &fixtures_dir().join("accepttoomuch_alias.scala"));
    if err.is_empty() {
        return;
    }
    assert!(ok, "local `type` aliases must resolve:\n{err}");
}

#[test]
fn e2e_accepttoomuch_alias_runs() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        return;
    };
    let dir = tmp_dir("alias-run");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let status = Command::new(bin())
        .args([
            "compile",
            fixtures_dir()
                .join("accepttoomuch_alias.scala")
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
        status.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert_eq!(
        run_java(&out, jar.to_str().unwrap()),
        expected_stdout("accepttoomuch_alias"),
    );
    let _ = fs::remove_dir_all(&dir);
}

/// The same fixture through real scalac, run the same way: the two overloads
/// that differ only in their erased *result* have to dispatch identically.
#[test]
fn real_scalac_dual_run_accepttoomuch_alias() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip real-scalac dual-run: jar or scalac not obtainable");
        return;
    };
    let dir = tmp_dir("alias-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("accepttoomuch_alias.scala"))
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac rejected accepttoomuch_alias:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        run_java(&dir, jar.to_str().unwrap()),
        expected_stdout("accepttoomuch_alias"),
    );
    let _ = fs::remove_dir_all(&dir);
}

/// An `import` inside a block takes effect where it stands, so the alias
/// pre-pass stops there. `pos/t5305` is the reproduction.
#[test]
fn an_import_inside_a_block_still_orders_the_aliases_after_it() {
    let (ok, err) = compile_src(
        "aliasimport",
        r#"
object T {
  def in(a: Any): Unit = {}
  object O { type F = Int; val v = "" }
  in {
    import O.{F, v}
    type x = { type l = (F, v.type) }
  }
}
"#,
    );
    if err.is_empty() {
        return;
    }
    assert!(ok, "an alias after an import keeps its order:\n{err}");
}

// ----------------------------------------------------------- double definition

const CLASH: &str = r#"
class D {
  def g(x: List[Int]): Int = 1
  def g(x: List[String]): Int = 2
}
"#;

#[test]
fn two_overloads_that_erase_to_one_descriptor_are_rejected() {
    let (ok, err) = compile_src("clash", CLASH);
    if err.is_empty() {
        return;
    }
    assert!(!ok, "`(List): Int` twice must be rejected:\n{err}");
    assert!(
        err.contains("double definition:"),
        "nsc's own headline:\n{err}"
    );
    if let Some((sok, stext)) = scalac_says("clash-scalac", CLASH) {
        assert!(!sok, "scalac must agree:\n{stext}");
        assert!(stext.contains("double definition:"), "{stext}");
    }
}

/// Parameter clauses are flattened, because the descriptor is
/// (`neg/t6443c`), and a repeated parameter is compared as the `Seq` it
/// becomes (`neg/t0259`).
#[test]
fn the_comparison_is_over_the_descriptor_not_the_clause_structure() {
    let src = r#"
class T0 { def visit(f: Int => Unit): Boolean = true
           def visit(f: Int => String): Boolean = true }
class T1() { def this(g: (String, Int)*) = this()
             def this(g: String*) = this() }
"#;
    let (ok, err) = compile_src("flatten", src);
    if err.is_empty() {
        return;
    }
    assert!(!ok, "both pairs must be rejected:\n{err}");
    assert_eq!(
        err.matches("double definition:").count(),
        2,
        "one per template:\n{err}"
    );
}

/// The JVM descriptor carries the result type and Scala uses that:
/// `scala.Function.uncurried` is five overloads taking one `Function1`.
/// Leaving the result out of the key cost twelve false diagnostics on
/// `src/library/scala/Function.scala` alone.
#[test]
fn overloads_that_differ_only_in_their_erased_result_are_accepted() {
    let src = r#"
object E {
  def uncurried[T1, T2, R](f: T1 => T2 => R): (T1, T2) => R = null
  def uncurried[T1, T2, T3, R](f: T1 => T2 => T3 => R): (T1, T2, T3) => R = null
}
"#;
    let (ok, err) = compile_src("result", src);
    if err.is_empty() {
        return;
    }
    assert!(ok, "the result type separates them:\n{err}");
    if let Some((sok, stext)) = scalac_says("result-scalac", src) {
        assert!(sok, "scalac accepts this:\n{stext}");
    }
}

/// A macro def has no bytecode, so two of them cannot collide. `pos/t7776`
/// is exactly this, and real scalac rejects the same pair written as ordinary
/// methods (`dd4` in the slice's notes).
///
/// Needs scala-reflect and scala-compiler for `blackbox.Context`, the way
/// `tests/scala_corpus.sh` runs the corpus; skipped when they are not there.
#[test]
fn macro_defs_are_exempt() {
    let reflect = PathBuf::from("/tmp/scala-2.13.16/lib/scala-reflect.jar");
    let compiler = PathBuf::from("/tmp/scala-2.13.16/lib/scala-compiler.jar");
    let (Some(jar), true) = (scala_library_jar(), reflect.is_file() && compiler.is_file()) else {
        return;
    };
    let dir = tmp_dir("macrodef");
    let file = dir.join("macrodef.scala");
    fs::write(
        &file,
        r#"
import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

class MacroErasure {
  def app(f: Any => Any, x: Any): Any = macro MacroErasure.appMacro
  def app[A](f: A => Any, x: Any): Any = macro MacroErasure.appMacroA[A]
}

object MacroErasure {
  def appMacro(c: Context)(f: c.Expr[Any => Any], x: c.Expr[Any]): c.Expr[Any] = {
    import c.universe._
    c.Expr(q"$f($x)")
  }
  def appMacroA[A](c: Context)(f: c.Expr[A => Any], x: c.Expr[Any])(
    implicit tt: c.WeakTypeTag[A]): c.Expr[Any] = {
    import c.universe._
    c.Expr(q"$f[${tt.tpe}]($x)")
  }
}
"#,
    )
    .unwrap();
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
            "-cp",
            &format!("{}:{}", reflect.display(), compiler.display()),
        ])
        .output()
        .expect("run scala-rs compile");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let _ = fs::remove_dir_all(&dir);
    assert!(
        !err.contains("double definition:"),
        "a macro def emits no method:\n{err}"
    );
}
