//! E2E tests for the `agent/catsyntax` slice: the extension methods
//! `import cats.syntax.all._` is supposed to bring into scope.
//!
//! Five gaps, all of them on the road from `fa.flatMap(…)` to a call:
//!
//! 1. **The first type argument of a higher-kinded class is not an element.**
//!    `map` / `flatMap` / `foreach` took the receiver's first type argument for
//!    the lambda's parameter type, which is right for `List[A]` and wrong for
//!    cats' `Ops[F[_], A]`: `Ops[Box, Int].flatMap(n => …)` gave `n` the type
//!    `Box`. Reproduces with no implicit conversion in sight. `csyn_ops`,
//!    `csyn_ops_bad`.
//!
//! 2. **A pickled `REFINEDtpe` result type.** simulacrum gives every
//!    `toFooOps` the result type `Foo.Ops[F, A] { type TypeClassType =
//!    Foo[F] }`, which `PickleSupply::conv` could not express, so the member
//!    was not supplied at all and the whole syntax layer was invisible.
//!    Reading it needed three things: the conversion itself, the parents of a
//!    `Type::Refined` as a place `subst_as_seen_from` walks into (or
//!    `flatMap`'s `A` stays raw), and `elem_type` seeing through it.
//!
//! 3. **`import o._` imports what `o` *has*.** `cats.syntax.all` declares
//!    almost nothing; every conversion comes from one of the ~60 traits it
//!    mixes in. Codegen then needs the imported object as the receiver, or it
//!    loads `this` and casts it to the trait that declares the conversion.
//!
//! 4. **`InnerClasses` is not a list of declarations.**
//!    `cats/effect/kernel/MonadCancel.class` names `cats/syntax/package$all$`
//!    in it; adopting that entry installed `cats.syntax.all` as a member of
//!    `MonadCancel`, and the later `import cats.syntax.all._` found nothing.
//!
//! 5. **A companion that is present may still have no implicits**, and a
//!    conversion's own type parameter may be solvable only from its implicit
//!    clause (cats' `catsSyntaxApplicativeError[F[_], E, A]` gets `E` only
//!    from the `ApplicativeError[F, E]` it asks for).
//!
//! 3--5 are exercised together by `a_simulacrum_style_syntax_layer_crosses_a_jar`,
//! which builds a miniature cats with **real scalac** -- our own pickle writer
//! does not emit a `REFINEDtpe`, so the fixture has to come from scalac to be
//! worth anything.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `csyn` prefix.

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
        "scala-rs-catsyntax-{tag}-{}-{nanos}-{seq}",
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

fn expected_stdout(name: &str) -> String {
    fs::read_to_string(fixtures_dir().join("expected").join(format!("{name}.txt"))).unwrap()
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
        "compile {name} failed extra={extra:?}:\n{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    out
}

/// `-Xverify:all`, so a wrong receiver for an inherited conversion is a
/// verification failure here rather than a silent difference in the output.
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

/// `--no-scala-library`: the private runtime.
fn check_private(name: &str) {
    let out = compile_fixture_with(name, &["--no-scala-library"]);
    if java_available() {
        assert_eq!(
            run_java(&out, None),
            expected_stdout(name),
            "stdout mismatch for {name} (private runtime)"
        );
    }
    let _ = fs::remove_dir_all(&out);
}

/// `--scala-library`: linked against the real 2.13.16 ABI, then run.
fn dual_run_fixture(name: &str) {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with(name, &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout(name),
        "stdout mismatch for library dual-run {name}"
    );
    let _ = fs::remove_dir_all(&out);
}

fn compile_fails(name: &str, extra: &[&str], needle: &str) {
    let src = fixtures_dir().join(format!("{name}.scala"));
    let out = tmp_dir(&format!("{name}-bad"));
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
        "expected compile of {name} (extra={extra:?}) to fail"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        err.contains(needle),
        "expected {name} error to contain {needle:?}, got: {err}"
    );
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------ 1. `Ops[F[_], A]` is not a collection of `F`s

#[test]
fn fixtures_csyn_ops() {
    dual_run_fixture("csyn_ops");
}

/// The same fixture on the private runtime: nothing here needs the library.
#[test]
fn fixtures_csyn_ops_private() {
    check_private("csyn_ops");
}

/// No stubbing: giving the lambda its declared parameter type does not make a
/// call legal that has no witness for the method's implicit clause.
#[test]
fn fixtures_csyn_ops_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    compile_fails(
        "csyn_ops_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "could not find implicit value of type FlatMap[Bag]",
    );
}

// ------------------------- 2-5. simulacrum's syntax layer, read from a pickle

/// A miniature cats: the pieces that made the real one unreachable, and
/// nothing else. Compiled by **scalac**, because the refinement result type
/// (`Ops[F, A] { type TypeClassType = FlatMap[F] }`) only exists in a pickle
/// scalac wrote.
const TINY_LIB: &str = r#"
package tinycats

trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }

trait FlatMap[F[_]] extends Functor[F] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

object FlatMap {
  trait Ops[F[_], A] extends Serializable {
    type TypeClassType <: FlatMap[F]
    def typeClassInstance: TypeClassType
    def self: F[A]
    def flatMap[B](f: A => F[B]): F[B] = typeClassInstance.flatMap[A, B](self)(f)
    def >>[B](fb: F[B]): F[B] = typeClassInstance.flatMap[A, B](self)(_ => fb)
  }
  trait ToFlatMapOps extends Serializable {
    implicit def toFlatMapOps[F[_], A](target: F[A])(implicit tc: FlatMap[F]):
        Ops[F, A] { type TypeClassType = FlatMap[F] } =
      new Ops[F, A] {
        type TypeClassType = FlatMap[F]
        val self = target
        val typeClassInstance = tc
      }
  }
}

final class Box[A](val a: A)

object Box {
  implicit val flatMapForBox: FlatMap[Box] = new FlatMap[Box] {
    def map[A, B](fa: Box[A])(f: A => B): Box[B] = new Box(f(fa.a))
    def flatMap[A, B](fa: Box[A])(f: A => Box[B]): Box[B] = f(fa.a)
  }
}
"#;

/// `all` is a nested object of the package object, exactly as `cats.syntax.all`
/// is: its class file is `tinycats/syntax/package$all$`.
const TINY_SYNTAX: &str = r#"
package tinycats

trait FlatMapSyntax extends FlatMap.ToFlatMapOps

trait AllSyntax extends FlatMapSyntax

package object syntax {
  object all extends AllSyntax
}
"#;

/// Nothing here is used by the program below except the name `Uses`. It exists
/// so that `other/Uses.class` mentions `tinycats/syntax/package$all$` in its
/// `InnerClasses` table -- and so that `tinycats.Box` is reached as a
/// placeholder before anything imports it.
const TINY_OTHER: &str = r#"
package other

import tinycats.Box
import tinycats.syntax.all._

trait Uses {
  def go(b: Box[Int]): Box[Int] = b.flatMap(n => new Box(n + 1))
}
"#;

const TINY_USER: &str = r#"
import other.Uses
import tinycats.Box
import tinycats.syntax.all._

object Main {
  def main(args: Array[String]): Unit = {
    println(new Box(3).flatMap(n => new Box(n + 1)).a)
    println((new Box(1) >> new Box(9)).a)
  }
}
"#;

/// `Crate` has no `FlatMap` instance anywhere: the conversion has no witness,
/// so the member error stands rather than the conversion being inserted.
const TINY_USER_BAD: &str = r#"
import tinycats.syntax.all._

final class Crate[A](val a: A)

object Main {
  def main(args: Array[String]): Unit =
    println(new Crate(3).flatMap(n => new Crate(n + 1)).a)
}
"#;

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

#[test]
fn a_simulacrum_style_syntax_layer_crosses_a_jar() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let Some(scalac) = scalac() else {
        eprintln!("skip: no scalac to write the refinement pickle with");
        return;
    };
    if jar_tool().is_none() {
        eprintln!("skip: no `jar` tool");
        return;
    }
    let dir = tmp_dir("tinycats");
    let lib = dir.join("lib.scala");
    let syn = dir.join("syntax.scala");
    let other = dir.join("other.scala");
    let user = dir.join("user.scala");
    let bad = dir.join("bad.scala");
    fs::write(&lib, TINY_LIB).unwrap();
    fs::write(&syn, TINY_SYNTAX).unwrap();
    fs::write(&other, TINY_OTHER).unwrap();
    fs::write(&user, TINY_USER).unwrap();
    fs::write(&bad, TINY_USER_BAD).unwrap();
    let lib_out = dir.join("libout");
    fs::create_dir_all(&lib_out).unwrap();

    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&lib_out)
        .args([&lib, &syn, &other])
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac failed on the miniature library:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lib_jar = dir.join("tinycats.jar");
    pack_jar(&lib_out, &lib_jar);

    let user_out = dir.join("userout");
    fs::create_dir_all(&user_out).unwrap();
    let (ok, msgs) = compile_against(&user_out, &jar, &user, std::slice::from_ref(&lib_jar));
    assert!(ok, "user failed to compile against the jar:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    let bad_out = dir.join("badout");
    fs::create_dir_all(&bad_out).unwrap();
    let (ok, msgs) = compile_against(&bad_out, &jar, &bad, std::slice::from_ref(&lib_jar));
    assert!(!ok, "expected the witness-less program to fail:\n{msgs}");
    assert!(
        msgs.contains("value flatMap is not a member of Crate[Int]"),
        "expected the member error scalac reports, got:\n{msgs}"
    );

    if java_available() {
        let cp = format!("{}:{}", jar.display(), lib_jar.display());
        assert_eq!(run_java(&user_out, Some(&cp)), "4\n9\n");
    }
    let _ = fs::remove_dir_all(&dir);
}
