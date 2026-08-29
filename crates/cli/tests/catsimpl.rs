//! E2E tests for the `agent/catsimpl` slice.
//!
//! Three gaps, all of them things cats and slick run into:
//!
//! 1. **A lambda that reads the enclosing `this` did not capture it.** The free
//!    variable scan behind `$outer` only looked for free *terms*, so a `this`
//!    written out (`this.f`) or -- far more often -- left implicit in a call to
//!    a method of the enclosing class marked nothing. The lambda class got no
//!    `$outer` field and codegen's `load_this` read slot 0, which inside
//!    `apply` is the *lambda*: `M3$$anonfun$0 cannot be cast to M3` at run
//!    time, with the type checker perfectly happy. `cats_lambda` /
//!    `cats_lambda2`.
//!
//! 2. **cats' syntax layer.** `implicit def toFlatMapOps[F[_], A](fa: F[A])
//!    (implicit F: FlatMap[F])` needs its `F` solved from the receiver's type
//!    *constructor* (it fell through to `AnyRef`) and its own implicit clause
//!    applied (it was dropped, so the call went out one argument short of its
//!    descriptor). `cats_syntax`, `cats_syntax_bad`.
//!
//! 3. **A companion object is in the implicit scope of its class (SLS 7.2),
//!    including for a higher-kinded type.** `Async[IO]` is
//!    `cats.effect.IO.asyncForIO` and nothing else; two things kept it out of
//!    reach across a `-cp` boundary -- the pickle reader declined a *wholly
//!    unapplied* class reference (`IO` as the argument of `Async[F[_]]`) as an
//!    arity error, and a jar-supplied `implicit def` never got the `IMPLICIT`
//!    flag, which only the pickle records. `a_higher_kinded_companion_implicit_
//!    crosses_a_jar`.
//!
//! 4. **A call that omits a defaulted parameter is typed twice.** The
//!    `name$default$n` getter takes the parameters that precede the default, so
//!    the arguments already given are handed to it and re-typed -- and a
//!    by-name argument has by then been wrapped into its `Function0` thunk,
//!    which the second pass called `() => <notype>` and matched to nothing.
//!    slick's `w2.orElse(where)` inside a `copy(…)`. `cats_byname`.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `cats` prefix.

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
        "scala-rs-catsimpl-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn scala_library_jar() -> Option<PathBuf> {
    let cached = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    cached.is_file().then_some(cached)
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
        "java Main failed: {}",
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

// ------------------------------------------------- 1. lambdas capturing `this`

#[test]
fn fixtures_cats_lambda() {
    dual_run_fixture("cats_lambda");
}

/// The same capture without any library collection, so the private runtime
/// covers it too.
#[test]
fn fixtures_cats_lambda2() {
    check_private("cats_lambda2");
}

#[test]
fn fixtures_cats_lambda2_lib() {
    dual_run_fixture("cats_lambda2");
}

// ------------------------------------------------------------ 2. cats syntax

#[test]
fn fixtures_cats_syntax() {
    dual_run_fixture("cats_syntax");
}

/// No stubbing: a receiver with no witness for the conversion's own implicit
/// clause keeps the member error scalac reports, rather than having the
/// conversion inserted and then failing on the implicit.
#[test]
fn fixtures_cats_syntax_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    compile_fails(
        "cats_syntax_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "value flatMap is not a member of Bag[Int]",
    );
}

// ------------------------------------- 3. companion implicit scope over a jar

const HK_LIB: &str = "\
package tinycats

trait Functor[F[_]] {
  def map[A, B](fa: F[A])(f: A => B): F[B]
}

trait FlatMap[F[_]] extends Functor[F] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

trait Async[F[_]] extends FlatMap[F] {
  def pure[A](a: A): F[A]
}

final class Box[A](val a: A)

object Box {
  implicit def asyncForBox: Async[Box] = new Async[Box] {
    def map[A, B](fa: Box[A])(f: A => B): Box[B] = new Box(f(fa.a))
    def flatMap[A, B](fa: Box[A])(f: A => Box[B]): Box[B] = f(fa.a)
    def pure[A](a: A): Box[A] = new Box(a)
  }
}
";

const HK_USER: &str = "\
import tinycats.{Async, Box}

object Main {
  // The only witness for `Async[Box]` lives on `Box`'s companion, which is a
  // class file of its own that nothing else in this source asks for.
  def need[F[_]](implicit F: Async[F]): F[Int] = F.pure(7)
  def main(args: Array[String]): Unit = println(need[Box].a)
}
";

const HK_USER_BAD: &str = "\
import tinycats.{Async, Box}

object Main {
  final class Crate[A](val a: A)
  def need[F[_]](implicit F: Async[F]): F[Int] = F.pure(7)
  // `Crate` has no companion and no instance anywhere: still an error.
  def main(args: Array[String]): Unit = println(need[Crate].a)
}
";

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

/// Compile the library, pack it into a jar, then compile a program that reaches
/// the witness only through the jar's `ScalaSignature`. This is the shape of
/// `Async[IO]` / `cats.effect.IO.asyncForIO`.
#[test]
fn a_higher_kinded_companion_implicit_crosses_a_jar() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip jar round trip: scala-library jar not present");
        return;
    };
    if jar_tool().is_none() {
        eprintln!("skip jar round trip: no `jar` tool");
        return;
    }
    let dir = tmp_dir("hkcompanion");
    let lib_src = dir.join("lib.scala");
    let user_src = dir.join("user.scala");
    let bad_src = dir.join("bad.scala");
    fs::write(&lib_src, HK_LIB).unwrap();
    fs::write(&user_src, HK_USER).unwrap();
    fs::write(&bad_src, HK_USER_BAD).unwrap();
    let lib_out = dir.join("libout");
    let user_out = dir.join("userout");
    let bad_out = dir.join("badout");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&user_out).unwrap();
    fs::create_dir_all(&bad_out).unwrap();

    let (ok, msgs) = compile_against(&lib_out, &jar, &lib_src, &[]);
    assert!(ok, "library failed to compile:\n{msgs}");
    let lib_jar = dir.join("tinycats.jar");
    pack_jar(&lib_out, &lib_jar);

    let (ok, msgs) = compile_against(&user_out, &jar, &user_src, std::slice::from_ref(&lib_jar));
    assert!(ok, "user failed to compile against the jar:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    // No stubbing: a type with no witness is still a hard error.
    let (ok, msgs) = compile_against(&bad_out, &jar, &bad_src, std::slice::from_ref(&lib_jar));
    assert!(!ok, "expected the witness-less program to fail:\n{msgs}");
    assert!(
        msgs.contains("could not find implicit value of type Async[Crate]"),
        "expected a missing-implicit error, got:\n{msgs}"
    );

    if java_available() {
        let cp = format!("{}:{}", jar.display(), lib_jar.display());
        assert_eq!(run_java(&user_out, Some(&cp)), "7\n");
    }
    let _ = fs::remove_dir_all(&dir);
}

// -------------------------- 4. by-name argument in a call that omits a default

#[test]
fn fixtures_cats_byname() {
    dual_run_fixture("cats_byname");
}
