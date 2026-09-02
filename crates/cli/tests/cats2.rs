//! E2E tests for the `agent/cats2` slice: the cats / `F[_]` cluster in
//! slick's `BasicBackend.scala` / `ConcurrencyControl.scala`.
//!
//! Two gaps, neither of them the type-projection root the brief guessed at:
//!
//! 1. **A summoner whose result type is one of its own parameters.**
//!    Every cats-effect type class writes
//!    `def apply[F[_]](implicit F: Async[F]): F.type = F`. That `F.type` is a
//!    `SINGLEtype` naming the method's *implicit parameter*, which
//!    `PickleSupply::conv` could not express, so the whole member was declined
//!    and `Async$.apply` kept the class file's reading:
//!    `apply(x$0: Async[F]): Async[F]`, whose parameter carries no `implicit`
//!    flag (the JVM has no such notion). `Async[F]` then stayed a bare method
//!    type and `Async[F].flatMap(fa)(f)` was
//!    "value flatMap is not a member of (Async[F])Async[F]" -- or, reached
//!    through the `cats.effect` package object's `val Async =
//!    cats.effect.kernel.Async`, "value flatMap is not a member of `Async$`",
//!    because the *module class* a package-object re-export stubs is never
//!    adopted from a pickle at all.
//!
//!    cats-core writes the same summoners as `: Applicative[F]`, which is why
//!    `Applicative[F]` worked and `Async[F]` did not.
//!
//!    Exercised by `a_summoner_returning_its_own_parameters_type_crosses_a_jar`,
//!    which builds a miniature library with **real scalac** -- our own pickle
//!    writer emits no `SINGLEtype` for a parameter, so the fixture has to come
//!    from scalac to be worth anything.
//!
//! 2. **`$this` in a string interpolation.** `this` is a keyword, not an
//!    identifier; read as an `Ident` it was looked up as a term and slick's
//!    `s"No type for symbol $sym found in $this"` failed with
//!    "not found: value this". `c2_thisinterp`, `c2_thisinterp_bad`.
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `c2` prefix.

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
        "scala-rs-cats2-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`, so a `this` loaded from the wrong slot is a verification
/// failure here rather than a silent difference in the output.
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

// ------------------------------------------------ 2. `$this` in `s"..."`

#[test]
fn fixtures_c2_thisinterp() {
    dual_run_fixture("c2_thisinterp");
}

/// The same fixture on the private runtime: nothing here needs the library.
#[test]
fn fixtures_c2_thisinterp_private() {
    check_private("c2_thisinterp");
}

/// No stubbing: reading `$this` as `this` does not make every `$name` resolve.
#[test]
fn fixtures_c2_thisinterp_bad_is_rejected() {
    compile_fails(
        "c2_thisinterp_bad",
        &["--no-scala-library"],
        "not found: value nosuchvalue",
    );
}

// ------------------------------- 1. a summoner whose result is `F.type`

/// A miniature cats-effect: a type class whose companion summons the witness
/// the way every cats-effect one does, `apply[F[_]](implicit F: TC[F]): F.type`.
/// Compiled by **scalac**, because that `SINGLEtype` result only exists in a
/// pickle scalac wrote.
const TINY_LIB: &str = r#"
package tinyeff

trait TC[F[_]] {
  def pure[A](a: A): F[A]
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

object TC {
  // The shape that mattered: the result type names the method's own
  // *implicit parameter*, not the class.
  def apply[F[_]](implicit F: TC[F]): F.type = F
}

final class Box[A](val a: A)

object Box {
  implicit val tcForBox: TC[Box] = new TC[Box] {
    def pure[A](a: A): Box[A] = new Box(a)
    def flatMap[A, B](fa: Box[A])(f: A => Box[B]): Box[B] = f(fa.a)
  }
}
"#;

/// The re-export shape `import cats.effect.Async` really goes through: a
/// package object with a `val` aliasing another package's module. The module
/// *class* it stubs is the one that was never adopted from a pickle.
const TINY_ALIAS: &str = r#"
package tinyeff

package object api {
  type TC[F[_]] = tinyeff.TC[F]
  val TC = tinyeff.TC
}
"#;

const TINY_USER: &str = r#"
import tinyeff.Box
import tinyeff.api.TC

object Main {
  // through the package object's `val TC = tinyeff.TC` re-export, which is
  // how `import cats.effect.Async` reaches `cats.effect.kernel.Async`
  def twice[G[_]: TC](fa: G[Int]): G[Int] =
    TC[G].flatMap(fa)(n => TC[G].pure(n * 2))

  // straight at the module
  def wrap[G[_]: tinyeff.TC](n: Int): G[Int] = tinyeff.TC[G].pure(n)

  def main(args: Array[String]): Unit = {
    println(twice(new Box(21)).a)
    println(wrap[Box](7).a)
  }
}
"#;

/// `Crate` has no `TC` instance anywhere: the summoner's implicit clause has
/// no witness, so the search error stands rather than the call being accepted.
const TINY_USER_BAD: &str = r#"
import tinyeff.api.TC

final class Crate[A](val a: A)

object Main {
  def main(args: Array[String]): Unit =
    println(TC[Crate].pure(3))
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
fn a_summoner_returning_its_own_parameters_type_crosses_a_jar() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let Some(scalac) = scalac() else {
        eprintln!("skip: no scalac to write the SINGLEtype pickle with");
        return;
    };
    if jar_tool().is_none() {
        eprintln!("skip: no `jar` tool");
        return;
    }
    let dir = tmp_dir("tinyeff");
    let lib = dir.join("lib.scala");
    let alias = dir.join("alias.scala");
    let user = dir.join("user.scala");
    let bad = dir.join("bad.scala");
    fs::write(&lib, TINY_LIB).unwrap();
    fs::write(&alias, TINY_ALIAS).unwrap();
    fs::write(&user, TINY_USER).unwrap();
    fs::write(&bad, TINY_USER_BAD).unwrap();
    let lib_out = dir.join("libout");
    fs::create_dir_all(&lib_out).unwrap();

    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&lib_out)
        .args([&lib, &alias])
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac failed on the miniature library:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lib_jar = dir.join("tinyeff.jar");
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
        msgs.contains("could not find implicit value of type TC[Crate]"),
        "expected the implicit-search error, got:\n{msgs}"
    );

    if java_available() {
        let cp = format!("{}:{}", jar.display(), lib_jar.display());
        assert_eq!(run_java(&user_out, Some(&cp)), "42\n7\n");
    }
    let _ = fs::remove_dir_all(&dir);
}
