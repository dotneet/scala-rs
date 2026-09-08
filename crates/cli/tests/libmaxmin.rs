//! E2E tests for the `agent/libmaxmin` slice: **`Predef._` is an import, not a
//! snapshot.**
//!
//! The brief handed this slice 55 errors in `tests/scalalib_measure.sh` --
//! `value max is not a member of Int` (34) and `value min` (21) -- with the
//! hypothesis that a source-defined `intWrapper` was losing to the
//! `--scala-library` jar's copy at the pickle seam.
//!
//! It is neither. Three measured facts settle it:
//!
//! 1. The measurement does not link the jar. `tests/scalalib_measure.sh` runs
//!    `--no-scala-library` by default, and in that mode the prelude builds no
//!    `RichInt` and no `intWrapper` **at all** (`prelude.rs` gates both on
//!    `library_abi`). There is no jar copy to win.
//! 2. The source declaration is a perfectly ordinary implicit and nothing
//!    shadows it: writing `import scala.Predef._` by hand in the same file
//!    makes both the conversion and an explicit `scala.Predef.intWrapper(a)`
//!    resolve.
//! 3. It reproduces in fifteen lines with no library source anywhere --
//!    `source_predef_conversion_is_in_scope_everywhere` below -- so nothing
//!    about the seam between source and pickle is involved.
//!
//! The cause is that `Predef._` was modelled by *copying* the prelude's
//! `Predef` members into the base scope at prelude-install time, before any
//! source is read. `docs/scala-library.md` records the identical defect for
//! the `scala._` half of the same auto-import ("`scala._` was a snapshot, not
//! an import"), fixed by `Typer::auto_import_scala_member`; that fix enters a
//! source class or object landing in package `scala` and says nothing about
//! the members of a source `Predef`. `crate::predef_reimport` is the missing
//! half.
//!
//! Measured on `tests/scalalib_measure.sh -no-specialization`:
//! `files=538 errors=1420 files_with_errors=166` before,
//! `files=538 errors=1226 files_with_errors=157` after.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. Fixture prefix: `libmaxmin_`.

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
        "scala-rs-libmaxmin-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
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

/// Compile a source string on its own. Returns whether it succeeded and the
/// combined diagnostics either way.
fn compile_src(tag: &str, src: &str, extra: &[&str]) -> (bool, String) {
    let dir = tmp_dir(tag);
    let file = dir.join(format!("{tag}.scala"));
    fs::write(&file, src).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let mut cmd = Command::new(bin());
    cmd.args([
        "compile",
        file.to_str().unwrap(),
        "-d",
        out.to_str().unwrap(),
    ]);
    cmd.args(extra);
    let output = cmd.output().expect("run scala-rs compile");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let ok = output.status.success();
    let _ = fs::remove_dir_all(&dir);
    (ok, text)
}

/// `--no-scala-library`: the arrangement `tests/scalalib_measure.sh` measures,
/// and the only one a program redefining `scala.Predef` can be compiled in
/// without the jar's own `Predef` also being present.
fn compile_private(tag: &str, src: &str) -> (bool, String) {
    compile_src(tag, src, &["--no-scala-library"])
}

// ---------------------------------------------------------------- the defect

/// The reproduction, with no library source anywhere.
///
/// A conversion `Predef` *inherits* -- as the library inherits `intWrapper`
/// from `LowPriorityImplicits` -- has to be in scope in a file that neither
/// writes `package scala` nor imports anything, because nsc opens `Predef._`
/// around every unit.
#[test]
fn source_predef_conversion_is_in_scope_everywhere() {
    let (ok, err) = compile_private(
        "inherited",
        r#"
package scala

final class RichIntX(val self: Int) {
  def xmax(that: Int): Int = if (self > that) self else that
}

private[scala] abstract class LowPriorityImplicitsX {
  implicit def xintWrapper(x: Int): RichIntX = new RichIntX(x)
}

object Predef extends LowPriorityImplicitsX

package other {
  object User {
    def f(a: Int, b: Int): Int = a xmax b
  }
}
"#,
    );
    assert!(
        ok,
        "a conversion inherited by a source scala.Predef should be in scope:\n{err}"
    );
}

/// The same thing with the conversion declared *directly* in the source
/// `Predef`. Kept separate because it rules out the first explanation anyone
/// reaches for -- "inherited members are not enumerated" -- which is not what
/// was wrong: neither shape worked.
#[test]
fn source_predef_conversion_declared_directly_is_in_scope() {
    let (ok, err) = compile_private(
        "direct",
        r#"
package scala

final class RichIntY(val self: Int) {
  def ymax(that: Int): Int = if (self > that) self else that
}

object Predef {
  implicit def yintWrapper(x: Int): RichIntY = new RichIntY(x)
}

package other {
  object User {
    def f(a: Int, b: Int): Int = a ymax b
  }
}
"#,
    );
    assert!(
        ok,
        "a conversion declared by a source scala.Predef should be in scope:\n{err}"
    );
}

/// The library's own shape, spelled with its own names: `intWrapper`,
/// `runtime.RichInt`, `max` and `min`, used from `scala.collection` the way
/// `BitSet.scala` writes `coll.nwords max otherBitset.nwords`.
///
/// This is the 55 errors the brief named, in twenty lines.
#[test]
fn int_max_and_min_reach_a_source_rich_int() {
    let (ok, err) = compile_private(
        "intwrapper",
        r#"
package scala {
  package runtime {
    final class RichInt(val self: Int) {
      def max(that: Int): Int = if (self > that) self else that
      def min(that: Int): Int = if (self < that) self else that
    }
  }

  private[scala] abstract class LowPriorityImplicits {
    implicit def intWrapper(x: Int): runtime.RichInt = new runtime.RichInt(x)
  }

  object Predef extends LowPriorityImplicits
}

package scala.collection {
  object BitSetLike {
    def unify(a: Int, b: Int): Int = a max b
    def clip(a: Int, b: Int): Int = a.min(b)
  }
}
"#,
    );
    assert!(ok, "source intWrapper should give Int max and min:\n{err}");
}

// --------------------------------------------------------------- the limits

/// The import is widened, not made permissive. A member `RichIntProbe` does
/// not declare is still `not a member`, and so is a name no conversion in
/// scope reaches at all.
///
/// Real scalac 2.13.16 rejects `tests/fixtures/libmaxmin_predef_bad.scala`
/// with `value hugest is not a member of Int` and `value absent is not a
/// member of Int`. This compiler names the same two members; it renders the
/// receiver as the literal's own type (`3`, `4`) rather than `Int`, which is
/// a pre-existing difference in how singleton types are displayed and is why
/// the assertion is on the member name.
#[test]
fn a_member_that_does_not_exist_is_still_diagnosed() {
    let name = "libmaxmin_predef_bad";
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--no-scala-library",
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        !output.status.success(),
        "expected {name} to be rejected, but it compiled"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains("value hugest is not a member"),
        "expected `hugest` to be diagnosed:\n{err}"
    );
    assert!(
        err.contains("value absent is not a member"),
        "expected `absent` to be diagnosed:\n{err}"
    );
    let _ = fs::remove_dir_all(&out);
}

/// A source object named `Predef` that is **not** `scala.Predef` supplies
/// nothing implicitly. Only the real one is auto-imported, and a slice that
/// widened this by name rather than by owner would pass every test above.
#[test]
fn a_predef_in_another_package_is_not_auto_imported() {
    let (ok, err) = compile_private(
        "otherpkg",
        r#"
package mine

final class RichIntZ(val self: Int) {
  def zmax(that: Int): Int = if (self > that) self else that
}

object Predef {
  implicit def zintWrapper(x: Int): RichIntZ = new RichIntZ(x)
}

object User {
  def f(a: Int, b: Int): Int = a zmax b
}
"#,
    );
    assert!(
        !ok,
        "mine.Predef must not be auto-imported, but the program compiled"
    );
    assert!(
        err.contains("zmax is not a member"),
        "expected `zmax` to be diagnosed:\n{err}"
    );
}

// ------------------------------------------------------------- it also runs

/// Compiling is not the same as being right.
///
/// The conversion `Predef` inherits is owned by a plain class, so the call has
/// to be emitted on `scala.Predef$` and not on `this`. Entering the members
/// without also recording the import that carried them produced a program that
/// type-checked and then died at run time with `class Main$ cannot be cast to
/// class scala.LowPriorityProbe` -- the JVM's verifier does not catch it, and
/// no compile-only check in this repository would have.
///
/// Expected output is real scalac 2.13.16's, compiling the same file against
/// the same jar.
#[test]
fn the_conversion_runs_and_prints() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let name = "libmaxmin_predef";
    let jar_s = jar.to_str().unwrap();
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(name);
    let status = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar_s,
        ])
        .status()
        .expect("run scala-rs compile");
    assert!(status.success(), "compile {name} failed");
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout(name),
        "stdout mismatch for {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------- what must not have moved

/// Every ordinary program -- one that does not define `scala.Predef` itself --
/// must be untouched: the pass is a no-op unless the run's own sources supply
/// that object. `max` still reaches `Int` through the prelude's `intWrapper`,
/// and a name that is not a member is still refused.
#[test]
fn an_ordinary_program_is_unaffected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap().to_string();
    let (ok, err) = compile_src(
        "ordinary",
        r#"
object M {
  def f(a: Int, b: Int): Int = a max b
  def g(a: Int, b: Int): Int = a.min(b)
}
"#,
        &["--scala-library", &jar_s],
    );
    assert!(ok, "prelude intWrapper should still supply max/min:\n{err}");

    let (bad, err) = compile_src(
        "ordinarybad",
        r#"
object M {
  def f(a: Int, b: Int): Int = a nosuchmember b
}
"#,
        &["--scala-library", &jar_s],
    );
    assert!(!bad, "a member RichInt does not have must still be refused");
    assert!(
        err.contains("nosuchmember is not a member"),
        "expected `nosuchmember` to be diagnosed:\n{err}"
    );
}
