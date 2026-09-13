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
//! 6. **A derivation candidate's own witness has an implicit scope.** Cats'
//!    `Isomorphisms.invariant[Option]` needs an `Invariant[Option]` that lives
//!    in `Option`'s companion. The immutable implicit search cannot discover
//!    that companion unless it is warmed before the candidate is tried.
//!
//! 7. **Reading scala-rs's own nested package-object modules.** Their
//!    classfiles contain JVM mixin forwarders as well as an enclosing Scala
//!    pickle. A raw forwarder such as `none[A]: Option[A]` can lose `A`, then
//!    shadow the precise inherited declaration and leave `none[Int]` at raw
//!    `Option`. `a_generic_syntax_module_round_trips_through_our_own_pickle`
//!    compiles both halves with scala-rs to keep that path covered.
//!
//! 3--6 are exercised together by `a_simulacrum_style_syntax_layer_crosses_a_jar`,
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

object Laws {
  trait Isomorphisms[F[_]]
  object Isomorphisms {
    implicit def invariant[F[_]](implicit F: Functor[F]): Isomorphisms[F] =
      new Isomorphisms[F] {}
  }
}

trait TinyEq[A]
object TinyEq {
  implicit val stringEq: TinyEq[String] = new TinyEq[String] {}
}

// Loading this signature first enters only a `TinyEq` class stub. A later
// explicit import of `TinyEq` must still load its same-named companion.
trait MentionsTinyEq { def eq: TinyEq[String] }

trait Numeric[A]

object Checks {
  def forAll[A](f: A => Unit)(implicit N: Numeric[A]): String = "checked"
}

trait Pick[A] { def label: String }
trait LowPriorityPick {
  implicit def anyPick[A]: Pick[A] = new Pick[A] { def label = "any" }
}
object Pick extends LowPriorityPick {
  implicit def numericPick[A](implicit N: Numeric[A]): Pick[A] =
    new Pick[A] { def label = "numeric" }
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
  object eq extends AllSyntax
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

/// `eq` is also a member of `AnyRef`. Cats exposes its equality syntax under
/// exactly this object name (`cats.syntax.eq`), so resolving the import must
/// keep following the package-object member rather than the inherited method.
const TINY_EQ_USER: &str = r#"
import tinycats.Box
import tinycats.syntax.eq._

object Main {
  def main(args: Array[String]): Unit =
    println(new Box(3).flatMap(n => new Box(n + 1)).a)
}
"#;

/// The result candidate is in the requested type's companion, while the
/// witness for its implicit clause is in the type constructor's companion.
/// No earlier expression in this compiler invocation warms `Box`'s scope.
const TINY_DERIVATION_USER: &str = r#"
import tinycats.{Box, Laws}

object Main {
  val isomorphisms: Laws.Isomorphisms[Box] =
    implicitly[Laws.Isomorphisms[Box]]

  def main(args: Array[String]): Unit = println("derived")
}
"#;

const TINY_IMPORTED_NESTED_USER: &str = r#"
import tinycats.Box
import tinycats.Laws.Isomorphisms

object Main {
  val isomorphisms: Isomorphisms[Box] = Isomorphisms.invariant[Box]
  def main(args: Array[String]): Unit = println("imported-nested")
}
"#;

const TINY_STUBBED_COMPANION_USER: &str = r#"
import tinycats.MentionsTinyEq
import tinycats.TinyEq

object Main {
  val eq: TinyEq[String] = implicitly[TinyEq[String]]
  def main(args: Array[String]): Unit = println("companion")
}
"#;

/// A declaration on the derived companion must beat a same-result implicit
/// inherited from its low-priority parent. An implicit-only clause is not a
/// conversion argument and must not cancel the declaration-owner advantage.
const TINY_LOW_PRIORITY_USER: &str = r#"
import tinycats.{Numeric, Pick}

object Main {
  implicit val numeric: Numeric[Int] = new Numeric[Int] {}
  val picked: Pick[Int] = implicitly[Pick[Int]]
  def main(args: Array[String]): Unit = println(picked.label)
}
"#;

/// A wildcard import from a binary object must recover Scala's parameter
/// clause boundary. The JVM descriptor flattens `f` and `N` into one list;
/// only the pickle says the second clause is implicit.
const TINY_TRAILING_IMPLICIT_USER: &str = r#"
import tinycats.Numeric
import tinycats.Checks._

object Main {
  implicit val numeric: Numeric[Int] = new Numeric[Int] {}
  val result: String = forAll((_: Int) => ())
  def main(args: Array[String]): Unit = println(result)
}
"#;

const SELF_PICKLE_LIB: &str = r#"
package selfcats

trait Show[A] { def render(a: A): String }
object Show {
  implicit val intShow: Show[Int] = new Show[Int] {
    def render(a: Int): String = a.toString
  }
  implicit def optionShow[A](implicit A: Show[A]): Show[Option[A]] =
    new Show[Option[A]] {
      def render(a: Option[A]): String = if (a.isEmpty) "none" else A.render(a.get)
    }
}

final class ShowOps[A](val value: A, val instance: Show[A]) {
  def show: String = instance.render(value)
}

trait OptionSyntax {
  def none[A]: Option[A] = Option.empty[A]
}
trait ShowSyntax {
  implicit def toShowOps[A](a: A)(implicit A: Show[A]): ShowOps[A] =
    new ShowOps[A](a, A)
}

package object syntax {
  object option extends OptionSyntax
  object show extends ShowSyntax
}
"#;

const SELF_PICKLE_USER: &str = r#"
import selfcats.Show
import selfcats.syntax.option._
import selfcats.syntax.show._

object Main {
  implicit val optionIntShow: Show[Option[Int]] = new Show[Option[Int]] {
    def render(a: Option[Int]): String = if (a.isEmpty) "none" else a.get.toString
  }
  def main(args: Array[String]): Unit = println(none[Int].show)
}
"#;

const SELF_PICKLE_JAVA_CASE_CLASS_LIB: &str = r#"
package selfdate

final case class Account(accountId: Long = 0L, registeredDate: java.util.Date, note: String)
"#;

const SELF_PICKLE_JAVA_CASE_CLASS_USER: &str = r#"
import selfdate.Account

object Main {
  def build(d: java.util.Date): Account =
    Account(note = "ok", registeredDate = d, accountId = 1L)

  def main(args: Array[String]): Unit = {
    val a = build(null)
    println(s"${a.accountId}:${a.note}:${a.registeredDate == null}")
  }
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
fn a_generic_syntax_module_round_trips_through_our_own_pickle() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("self-pickle");
    let lib = dir.join("lib.scala");
    let user = dir.join("user.scala");
    fs::write(&lib, SELF_PICKLE_LIB).unwrap();
    fs::write(&user, SELF_PICKLE_USER).unwrap();

    let lib_out = dir.join("libout");
    fs::create_dir_all(&lib_out).unwrap();
    let (ok, msgs) = compile_against(&lib_out, &jar, &lib, &[]);
    assert!(
        ok,
        "scala-rs failed to build the miniature library:\n{msgs}"
    );

    let user_out = dir.join("userout");
    fs::create_dir_all(&user_out).unwrap();
    let (ok, msgs) = compile_against(&user_out, &jar, &user, std::slice::from_ref(&lib_out));
    assert!(
        ok,
        "scala-rs failed to read its own nested syntax module:\n{msgs}"
    );
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    if java_available() {
        let cp = format!("{}:{}", jar.display(), lib_out.display());
        assert_eq!(run_java(&user_out, Some(&cp)), "none\n");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_case_class_apply_with_a_jdk_type_keeps_named_parameters_in_our_own_pickle() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let dir = tmp_dir("self-pickle-java-case-class");
    let lib = dir.join("lib.scala");
    let user = dir.join("user.scala");
    fs::write(&lib, SELF_PICKLE_JAVA_CASE_CLASS_LIB).unwrap();
    fs::write(&user, SELF_PICKLE_JAVA_CASE_CLASS_USER).unwrap();

    let lib_out = dir.join("libout");
    fs::create_dir_all(&lib_out).unwrap();
    let (ok, msgs) = compile_against(&lib_out, &jar, &lib, &[]);
    assert!(ok, "scala-rs failed to build the case class:\n{msgs}");

    let user_out = dir.join("userout");
    fs::create_dir_all(&user_out).unwrap();
    let (ok, msgs) = compile_against(&user_out, &jar, &user, std::slice::from_ref(&lib_out));
    assert!(
        ok,
        "scala-rs failed to read named parameters whose signature contains a JDK type:\n{msgs}"
    );
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    if java_available() {
        let cp = format!("{}:{}", jar.display(), lib_out.display());
        assert_eq!(run_java(&user_out, Some(&cp)), "1:ok:true\n");
    }
    let _ = fs::remove_dir_all(&dir);
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
    let eq_user = dir.join("eq-user.scala");
    let derivation_user = dir.join("derivation-user.scala");
    let imported_nested_user = dir.join("imported-nested-user.scala");
    let stubbed_companion_user = dir.join("stubbed-companion-user.scala");
    let low_priority_user = dir.join("low-priority-user.scala");
    let trailing_implicit_user = dir.join("trailing-implicit-user.scala");
    let bad = dir.join("bad.scala");
    fs::write(&lib, TINY_LIB).unwrap();
    fs::write(&syn, TINY_SYNTAX).unwrap();
    fs::write(&other, TINY_OTHER).unwrap();
    fs::write(&user, TINY_USER).unwrap();
    fs::write(&eq_user, TINY_EQ_USER).unwrap();
    fs::write(&derivation_user, TINY_DERIVATION_USER).unwrap();
    fs::write(&imported_nested_user, TINY_IMPORTED_NESTED_USER).unwrap();
    fs::write(&stubbed_companion_user, TINY_STUBBED_COMPANION_USER).unwrap();
    fs::write(&low_priority_user, TINY_LOW_PRIORITY_USER).unwrap();
    fs::write(&trailing_implicit_user, TINY_TRAILING_IMPLICIT_USER).unwrap();
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

    let trailing_implicit_user_out = dir.join("trailing-implicit-userout");
    fs::create_dir_all(&trailing_implicit_user_out).unwrap();
    let (ok, msgs) = compile_against(
        &trailing_implicit_user_out,
        &jar,
        &trailing_implicit_user,
        std::slice::from_ref(&lib_jar),
    );
    assert!(
        ok,
        "trailing implicit user failed to compile against the jar:\n{msgs}"
    );
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    let imported_nested_user_out = dir.join("imported-nested-userout");
    fs::create_dir_all(&imported_nested_user_out).unwrap();
    let (ok, msgs) = compile_against(
        &imported_nested_user_out,
        &jar,
        &imported_nested_user,
        std::slice::from_ref(&lib_jar),
    );
    assert!(
        ok,
        "imported nested type user failed to compile against the jar:\n{msgs}"
    );
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    let low_priority_user_out = dir.join("low-priority-userout");
    fs::create_dir_all(&low_priority_user_out).unwrap();
    let (ok, msgs) = compile_against(
        &low_priority_user_out,
        &jar,
        &low_priority_user,
        std::slice::from_ref(&lib_jar),
    );
    assert!(
        ok,
        "low-priority implicit user failed to compile against the jar:\n{msgs}"
    );
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    let stubbed_companion_user_out = dir.join("stubbed-companion-userout");
    fs::create_dir_all(&stubbed_companion_user_out).unwrap();
    let (ok, msgs) = compile_against(
        &stubbed_companion_user_out,
        &jar,
        &stubbed_companion_user,
        std::slice::from_ref(&lib_jar),
    );
    assert!(
        ok,
        "stubbed companion user failed to compile against the jar:\n{msgs}"
    );
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    let derivation_user_out = dir.join("derivation-userout");
    fs::create_dir_all(&derivation_user_out).unwrap();
    let (ok, msgs) = compile_against(
        &derivation_user_out,
        &jar,
        &derivation_user,
        std::slice::from_ref(&lib_jar),
    );
    assert!(
        ok,
        "derived implicit user failed to compile against the jar:\n{msgs}"
    );
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    let eq_user_out = dir.join("eq-userout");
    fs::create_dir_all(&eq_user_out).unwrap();
    let (ok, msgs) = compile_against(&eq_user_out, &jar, &eq_user, std::slice::from_ref(&lib_jar));
    assert!(
        ok,
        "eq syntax user failed to compile against the jar:\n{msgs}"
    );
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
        assert_eq!(run_java(&eq_user_out, Some(&cp)), "4\n");
        assert_eq!(run_java(&derivation_user_out, Some(&cp)), "derived\n");
        assert_eq!(
            run_java(&imported_nested_user_out, Some(&cp)),
            "imported-nested\n"
        );
        assert_eq!(run_java(&low_priority_user_out, Some(&cp)), "numeric\n");
        assert_eq!(
            run_java(&trailing_implicit_user_out, Some(&cp)),
            "checked\n"
        );
        assert_eq!(
            run_java(&stubbed_companion_user_out, Some(&cp)),
            "companion\n"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}
