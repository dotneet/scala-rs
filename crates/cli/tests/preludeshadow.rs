//! E2E tests for the `agent/preludeshadow` slice: **a source definition of a
//! name the prelude also supplies replaces the prelude's symbol.**
//!
//! This typer knows the standard library as the hand-written
//! `crates/typer/src/prelude*.rs` signature tables, so compiling
//! scala/scala's own `src/library` asks it to typecheck source definitions of
//! the very names it already believes it knows. Before this slice the
//! prelude's symbol won every lookup — it is allocated first, so it comes
//! first in the package's member list — and the file that *defines*
//! `iterator` reported `value iterator is not a member of IterableOnce[A]`.
//! Nine lines reproduced it; `source_definition_replaces_prelude_trait` is
//! those nine lines.
//!
//! Three things had to change, and the tests below are one per change:
//!
//! 1. `SymbolTable::shadow_supplied_by_source` takes the prelude symbol out
//!    of its owner's members and out of every open scope — including the
//!    prelude's own scope, which never pops and would otherwise outlive any
//!    shadowing — and `find_class_by_jvm` skips it.
//! 2. `Typer::auto_import_scala_member`: `scala._` is open around every unit,
//!    and the prelude models that by *copying* the package's members into its
//!    scope at install time. A source `Tuple9.scala` compiled in the same run
//!    arrives after that snapshot, so it was invisible three packages away.
//! 3. `class_sym_of` looks a tuple's `TupleN` up in the **type** namespace. A
//!    term of that name in a nearer scope (`object Equiv` declares `implicit
//!    def Tuple2[T1, T2]`) used to swallow the lookup.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `pshadow` prefix.

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
        "scala-rs-preludeshadow-{tag}-{}-{nanos}-{seq}",
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

/// Compile `src` on its own, in `--no-scala-library` mode. Returns the
/// combined diagnostics, whether or not the compile succeeded: the tests that
/// redefine `scala.*` names cannot be run against the jar (the jar already
/// has those classes), and `--no-scala-library` is the arrangement that would
/// actually retire it.
fn compile_private(tag: &str, src: &str) -> (bool, String) {
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
            "--no-scala-library",
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

// ------------------------------ 1. a source trait replaces the prelude's

/// The nine-line reproduction from `docs/scala-library.md`. `IterableOnce` is
/// a prelude class; this file defines it, and the member it declares has to
/// be the one a call through that type finds.
#[test]
fn source_definition_replaces_prelude_trait() {
    let (ok, err) = compile_private(
        "iterableonce",
        r#"
package scala.collection

trait IterableOnce[+A] {
  def myOwnMember: Int
}

object P {
  def f(it: IterableOnce[Int]): Int = it.myOwnMember
}
"#,
    );
    assert!(
        ok,
        "source IterableOnce should replace the prelude's:\n{err}"
    );
}

/// The other half of the same collision, and the reason it is not enough to
/// enter the source symbol *alongside* the prelude's: the prelude enters
/// names like `IterableOnce` into a scope of its own that stays open for the
/// whole run, so a source definition reached from a **different** package
/// still met the prelude's symbol.
#[test]
fn the_replacement_is_visible_from_another_package() {
    let (ok, err) = compile_private(
        "crosspkg",
        r#"
package scala.collection

trait IterableOnce[+A] {
  def myOwnMember: Int
}

package object other {
  def g(it: scala.collection.IterableOnce[Int]): Int = it.myOwnMember
}
"#,
    );
    assert!(
        ok,
        "the source trait should be what other packages see:\n{err}"
    );
}

/// A source `object` replaces the prelude's module, not its class, and a
/// source `class` the other way round: Scala's two namespaces stay apart.
#[test]
fn source_object_replaces_prelude_module() {
    let (ok, err) = compile_private(
        "console",
        r#"
package scala

object Console {
  def myOwnMember: Int = 3
}

object P {
  def f(): Int = Console.myOwnMember
}
"#,
    );
    assert!(
        ok,
        "source object Console should replace the prelude's:\n{err}"
    );
}

/// Nothing is replaced when the names merely look alike: a class in a package
/// of its own, and the prelude's `scala.Iterable` still answers for
/// `Iterable`.
///
/// `Option` would not do as the name here, and the reason is worth knowing:
/// `tree_to_type` maps an applied type tree whose name is `Option`, `List` or
/// `Some` straight onto the prelude's symbol *whatever prefix it is written
/// with*, so even `mine.Option[Int]` comes out `scala.Option[Int]`. That is a
/// pre-existing bug — it reproduces unchanged on the branch point — and it is
/// written up in `docs/scala-library.md`.
#[test]
fn an_unrelated_package_does_not_shadow_the_prelude() {
    let (ok, err) = compile_private(
        "unrelated",
        r#"
package mine

class Iterable[A](val a: A)

object P {
  def f(o: scala.Option[Int]): Int = o.getOrElse(0)
  def g(o: mine.Iterable[Int]): Int = o.a
}
"#,
    );
    assert!(
        ok,
        "an unrelated `Iterable` must not disturb the prelude's:\n{err}"
    );
}

// -------------------------- 2. `scala._` is open, snapshot or no snapshot

/// A class defined in package `scala` in this run is in scope everywhere, the
/// way `scala._` being auto-imported says it is. In `--no-scala-library` mode
/// the prelude builds no `Tuple9` at all, so before this the source one was
/// unreachable from any other package and `(a, …, i)` had no class behind it.
#[test]
fn a_source_class_in_package_scala_is_auto_imported() {
    let (ok, err) = compile_private(
        "autoimport",
        r#"
package scala

class MyOwnMarker(val n: Int)

package object elsewhere {
  def f(): Int = new MyOwnMarker(7).n
}
"#,
    );
    assert!(ok, "package scala members should be auto-imported:\n{err}");
}

// ------------------- 3. `TupleN` is looked up in the type namespace

/// `object Equiv` declares `implicit def Tuple2[T1, T2](…)` and then writes
/// `x._1` on a `(T1, T2)` a few lines below. The tuple's class was looked up
/// with the namespace-blind `lookup`, which stopped at the nearest scope that
/// binds the name at all — the method — found nothing class-like in it and
/// gave up. 176 of `Ordering.scala`'s and `Equiv.scala`'s errors were that.
#[test]
fn fixtures_pshadow_tuplename() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let src = fixtures_dir().join("pshadow_tuplename.scala");
    let out = tmp_dir("pshadow_tuplename");
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
        "compile pshadow_tuplename failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("pshadow_tuplename")
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source through **real scalac**, so the fixture is pinned to what
/// Scala 2.13.16 actually prints and not just to our own output.
#[test]
fn real_scalac_dual_run_pshadow_tuplename() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip real-scalac dual-run: jar or scalac not obtainable");
        return;
    };
    let dir = tmp_dir("pshadow_tuplename-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("pshadow_tuplename.scala"))
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac rejected pshadow_tuplename:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        run_java(&dir, Some(jar.to_str().unwrap())),
        expected_stdout("pshadow_tuplename"),
    );
    let _ = fs::remove_dir_all(&dir);
}
