//! E2E tests for the `agent/tail1` slice: three small, independent slick
//! error clusters.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `t1` prefix.
//!
//! # (1) `value X is not a member of Y$` through a package-object `val`
//!
//! `agent/companionkind` split every jar module's companion from its class,
//! but left one adjacent hole (documented in README's "コンパニオンとクラスは
//! 別のシンボル" section): a companion's *nested* class or object could not
//! be found when the companion itself was reached through a package object's
//! `val` alias -- exactly the shape `cats.effect`'s `package object effect`
//! uses to re-export `Resource` and `Outcome` from `cats.effect.kernel`.
//! `import cats.effect.Resource` (or `Outcome`) picks up that `val`, and
//! `Outcome.Succeeded(_)` / `Resource.ExitCase.Errored(e)` both go through it.
//!
//! `Box.of` (a *direct* member) worked; `Box.Const` (a nested case class)
//! failed with `value Const is not a member of Box$`. Root cause, found by
//! tracing `type_select`'s member-lookup fallback in `crates/typer/src/
//! check.rs`: the fallback used `qual.sym` -- the *val* symbol, whose
//! `jvm_name` is empty -- to look up on-demand classpath candidates, instead
//! of the val's own *type*, which already names the real module. Fixing that
//! alone surfaced three further gaps this test also pins down:
//!
//! * `complete_binary_member`'s candidate loop returned on the *first* JVM
//!   name that existed, so a case class with a companion (`Const` /
//!   `Const$`) only ever got the class half installed, and `Const(5)` read
//!   as "value apply is not a member of Const".
//! * A generic-signature parent naming `scala/runtime/Nothing$` (a case
//!   object's `extends Trait[Nothing]`) built an ordinary class stub named
//!   `Nothing$` instead of `Type::Nothing`, so `Trait[Nothing] <: Trait[Int]`
//!   failed for a *covariant* `Trait[+A]` -- and separately, a jar-read
//!   class's type parameters never got variance at all (only the pickle
//!   carries it; a JVM generic signature cannot).
//! * A package object's `val` compiles to a zero-arg *method* indistinguishable
//!   from a `def` at the class-file level; only `pflags::STABLE` in the
//!   pickle says which is which, and nothing read it, so `Resource.ExitCase`
//!   (a `p.T` path-dependent type, which SLS 3.2.3 requires a *stable* `p`
//!   for) failed with "stable identifier required, but Resource found" even
//!   after the member lookup itself was fixed.
//!
//! `a_nested_member_through_a_package_object_val` reproduces the whole chain
//! with a real scalac-built jar: a case class with a companion (`Box.Const`)
//! and a case object extending a covariant trait applied to `Nothing`
//! (`Outcome.Canceled`), both reached only through a package object's `val`
//! alias -- never through a direct import, which worked throughout.
//!
//! # (2) `value getOrElse is not a member of Product`
//!
//! Minimized from slick's `PositionedResult.scala` (`nextBlobOption()
//! getOrElse …`, an `if (cond) None else Some(r)` block with no declared
//! return type). A from-scratch reproduction of the same shape --
//! including the exact `abstract class … extends Closeable`, a
//! forward-referenced companion-object exception, and `java.sql.{Blob,
//! Bytes, Clob, Object}`-shaped return values -- type-checks correctly on
//! its own (`f.getOrElse(...)` resolves against `Option[Blob]` etc., not
//! `Product`), both directly and through `-Xsource:3`. The failure did not
//! reproduce outside slick's full ~184-file compilation, so it depends on
//! cross-file state this slice did not track down; **not fixed here**. See
//! "Remaining" in README.md.
//!
//! # (3) `not found: value fromInt`
//!
//! `import integral._` (an implicit `Integral[T]`) followed by a bare
//! `zero` / `one` / `fromInt(n)`: ordinary inherited methods a standard
//! library trait's *pickle* declares, with no classfile of their own.
//! `expose_unqualified`'s wildcard-import fallback (`crates/typer/src/
//! check.rs`) asked `complete_binary_member` for them -- the same
//! nested-classfile-candidate search task (1) had to move away from for
//! `Box.Const` -- which can only ever find a *class*, never an ordinary
//! method. Fixed by falling back to `PickleSupply::complete` (the same
//! pickle path an ordinary member selection like `x.zero` already uses)
//! once the classfile-only search comes up empty.
//!
//! `fixtures_t1_wildcard_inherited` pins the fix with a scala-library
//! dual-run fixture (`tests/fixtures/t1_wildcard_inherited.scala`); real
//! scalac's acceptance of the same source is checked directly against
//! `tests/fixtures/expected/t1_wildcard_inherited.txt`.

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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "scala-rs-tail1-{tag}-{}-{nanos}-{seq}",
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

fn compile_against(out: &Path, jar: &Path, src: &Path, extra_cp: &[PathBuf]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile").arg(src);
    cmd.args(["-d", out.to_str().unwrap()]);
    if !extra_cp.is_empty() {
        let joined = extra_cp
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(":");
        cmd.args(["-cp", &joined]);
    }
    let output = cmd
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

// ---------------------------------------------------------------------
// (1) a companion's nested class/object, reached through a package-object
//     val alias.
// ---------------------------------------------------------------------

/// `object Box { final case class Const[A](get: A); def of[A](a: A) }`: a
/// direct member (`of`) and a nested case class with its own companion
/// (`Const` / `Const$`).
///
/// `object Outcome { ... case object Canceled extends Outcome[Nothing] }`
/// is cats-effect's real shape in miniature: a *covariant* trait applied to
/// `Nothing` in a case object's `extends` clause, which only a jar's generic
/// signature (not its pickle) can describe -- the pickle carries the
/// variance, the classfile Signature attribute carries `Nothing` written as
/// `scala.runtime.Nothing$`.
const T1_LIB: &str = r#"
package t1lib

trait Box[A] {
  def get: A
}

object Box {
  final case class Const[A](get: A) extends Box[A]

  def of[A](a: A): Box[A] = Const(a)
}

sealed trait Outcome[+A]

object Outcome {
  final case class Succeeded[A](a: A) extends Outcome[A]
  case object Canceled extends Outcome[Nothing]
}
"#;

/// `val Box = t1lib.Box` is exactly `cats.effect`'s `package object effect`
/// shape for `Resource` / `Outcome`: a re-exporting `val` alongside a `type`
/// alias of the same name, so `import t1lib.alias.Box` picks up a name that
/// is *both* a term and a type.
const T1_PKG: &str = r#"
package t1lib

package object alias {
  type Box[A] = t1lib.Box[A]
  val Box = t1lib.Box
  type Outcome[A] = t1lib.Outcome[A]
  val Outcome = t1lib.Outcome
}
"#;

const T1_USER: &str = r#"
import t1lib.alias.{Box, Outcome}

object Main {
  def describe(o: Outcome[Int]): String = o match {
    case Outcome.Succeeded(a) => s"ok $a"
    case Outcome.Canceled     => "cancel"
  }

  def main(args: Array[String]): Unit = {
    val b = Box.of(3)
    println(b)
    val c = Box.Const(5)
    println(c)
    println(describe(Outcome.Succeeded(7)))
    println(describe(Outcome.Canceled))
  }
}
"#;

/// Neither the trait nor the object has `bogus`; entering the companion's
/// nested members through the package-object val must not invent one.
const T1_USER_BAD: &str = r#"
import t1lib.alias.Box

object Main {
  def main(args: Array[String]): Unit =
    println(Box.Const(5).bogus)
}
"#;

fn write_and_pack(dir: &Path, files: &[(&str, &str)]) -> PathBuf {
    let mut paths = Vec::new();
    for (name, src) in files {
        let p = dir.join(name);
        fs::write(&p, src).unwrap();
        paths.push(p);
    }
    let scalac = self::scalac().expect("checked by caller");
    let lib_out = dir.join("libout");
    fs::create_dir_all(&lib_out).unwrap();
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&lib_out)
        .args(&paths)
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac failed on the miniature library:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lib_jar = dir.join("t1lib.jar");
    pack_jar(&lib_out, &lib_jar);
    lib_jar
}

#[test]
fn a_nested_member_through_a_package_object_val() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let Some(_scalac) = scalac() else {
        eprintln!("skip: no scalac to write the pickle with");
        return;
    };
    if jar_tool().is_none() {
        eprintln!("skip: no `jar` tool");
        return;
    }
    let dir = tmp_dir("pkgval");
    let lib_jar = write_and_pack(&dir, &[("lib.scala", T1_LIB), ("pkg.scala", T1_PKG)]);

    let user = dir.join("user.scala");
    fs::write(&user, T1_USER).unwrap();
    let user_out = dir.join("userout");
    fs::create_dir_all(&user_out).unwrap();
    let (ok, msgs) = compile_against(&user_out, &jar, &user, std::slice::from_ref(&lib_jar));
    assert!(ok, "user failed to compile against the jar:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    let bad = dir.join("bad.scala");
    fs::write(&bad, T1_USER_BAD).unwrap();
    let bad_out = dir.join("badout");
    fs::create_dir_all(&bad_out).unwrap();
    let (ok, msgs) = compile_against(&bad_out, &jar, &bad, std::slice::from_ref(&lib_jar));
    assert!(!ok, "expected the bogus member to be rejected:\n{msgs}");
    assert!(
        msgs.contains("bogus"),
        "expected the error to name the missing member, got:\n{msgs}"
    );

    if java_available() {
        let cp = format!("{}:{}", jar.display(), lib_jar.display());
        assert_eq!(
            run_java(&user_out, Some(&cp)),
            "Const(3)\nConst(5)\nok 7\ncancel\n",
            "stdout mismatch for a_nested_member_through_a_package_object_val"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Real scalac accepts `T1_LIB` / `T1_PKG` / `T1_USER` as ordinary, valid
/// Scala (the fixture is not testing our own quirks): compile the whole
/// three-file program with scalac alone and run it, matching the same
/// expected stdout the jar-based test above pins for our compiler.
#[test]
fn real_scalac_accepts_the_same_program() {
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
    let dir = tmp_dir("pkgval-scalac");
    let lib = dir.join("lib.scala");
    let pkg = dir.join("pkg.scala");
    let user = dir.join("user.scala");
    fs::write(&lib, T1_LIB).unwrap();
    fs::write(&pkg, T1_PKG).unwrap();
    fs::write(&user, T1_USER).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let result = Command::new(&scalac)
        .arg("-d")
        .arg(&out)
        .args([&lib, &pkg, &user])
        .output()
        .expect("run scalac");
    assert!(
        result.status.success(),
        "scalac rejected the fixture:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap())),
        "Const(3)\nConst(5)\nok 7\ncancel\n",
    );
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------
// (3) `import integral._` must reach ordinary inherited methods
//     (`zero` / `one` / `fromInt`), not only nested classfiles.
// ---------------------------------------------------------------------

#[test]
fn fixtures_t1_wildcard_inherited() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let src = fixtures_dir().join("t1_wildcard_inherited.scala");
    let out = tmp_dir("wildcard_inherited");
    let output = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out.to_str().unwrap(),
            "--scala-library",
            jar.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    assert!(
        output.status.success(),
        "compile t1_wildcard_inherited failed:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap())),
        expected_stdout("t1_wildcard_inherited"),
        "stdout mismatch for t1_wildcard_inherited"
    );
    let _ = fs::remove_dir_all(&out);
}

/// Real scalac accepts the fixture as ordinary, valid Scala.
#[test]
fn real_scalac_accepts_t1_wildcard_inherited() {
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
    let src = fixtures_dir().join("t1_wildcard_inherited.scala");
    let out = tmp_dir("wildcard_inherited-scalac");
    let result = Command::new(&scalac)
        .arg("-d")
        .arg(&out)
        .arg(&src)
        .output()
        .expect("run scalac");
    assert!(
        result.status.success(),
        "scalac rejected t1_wildcard_inherited:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        run_java(&out, Some(jar.to_str().unwrap())),
        expected_stdout("t1_wildcard_inherited"),
    );
    let _ = fs::remove_dir_all(&out);
}
