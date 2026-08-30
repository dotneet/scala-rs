//! E2E tests for the `agent/companionkind` slice: a companion object and its
//! class are two symbols.
//!
//! `find_or_stub_java_class` is the entry point for every JVM name a parent
//! list, a descriptor or an `InnerClasses` table mentions. Handed
//! `cats/effect/kernel/Ref$` it used to strip the trailing `$`, allocate a
//! `SymKind::Class` called `Ref`, and store the *companion's* name in its
//! `jvm_name`. One symbol then stood for two things:
//!
//! * the trait `Ref` could not get a symbol of its own -- `ensure_class`
//!   answers "there is a `Ref`, but its `jvm_name` is not the key I asked
//!   for" and declines -- so `Ref#update`'s type came from the class file's
//!   generic signature. A JVM signature cannot write `F[Unit]`; it writes
//!   `TF;`. Hence `value >> is not a member of F` and
//!   `no matching overload for (Function0[A])F`.
//! * the object's own members were installed on that same symbol, so
//!   `Ref.of` / `Ref.const` looked like members of the trait.
//!
//! A `$`-suffixed name now stubs a `ModuleClass` plus its `Module`, exactly
//! as `install_java_module` does for a class file it has really read.
//!
//! The same confusion has a second half, which shows up with **no cats in
//! sight**: `scala.concurrent.Future` is not a prelude class, and
//! `adopt_binary_class` refused every `scala/` name, so `Future`'s members
//! could only ever come from the class file. `Future.apply` takes its body
//! by name, a JVM signature says `Function0`, and `Future(21)` did not
//! typecheck. The refusal now applies only to what the prelude actually
//! built (`SymbolTable::prelude_end`).
//!
//! Kept out of `crates/cli/tests/e2e.rs` to avoid merge conflicts; see
//! `.agent-brief.md`. All fixtures use the `ckind` prefix.

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
        "scala-rs-companionkind-{tag}-{}-{nanos}-{seq}",
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

/// `-Xverify:all`, so a signature read from the wrong place shows up as a
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

// ------------------------------------------- 1. `Future`: a by-name member
//                                                of a jar-only companion

/// `Future(21)`: the body is `=> T` in the pickle and `Function0[T]` in the
/// class file. Runs the emitted classes under `-Xverify:all`.
#[test]
fn fixtures_ckind_future() {
    if !java_available() {
        return;
    }
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip scala-library dual-run: jar not obtainable");
        return;
    };
    let jar_s = jar.to_str().unwrap();
    let out = compile_fixture_with("ckind_future", &["--scala-library", jar_s]);
    assert_eq!(
        run_java(&out, Some(jar_s)),
        expected_stdout("ckind_future"),
        "stdout mismatch for library dual-run ckind_future"
    );
    let _ = fs::remove_dir_all(&out);
}

/// The same source through **real scalac**, so the fixture is pinned to what
/// Scala 2.13.16 actually prints and not just to our own output.
#[test]
fn real_scalac_dual_run_ckind_future() {
    if !java_available() {
        return;
    }
    let (Some(jar), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip real-scalac dual-run: jar or scalac not obtainable");
        return;
    };
    let dir = tmp_dir("ckind_future-scalac");
    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&dir)
        .arg(fixtures_dir().join("ckind_future.scala"))
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac rejected ckind_future:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        run_java(&dir, Some(jar.to_str().unwrap())),
        expected_stdout("ckind_future"),
    );
    let _ = fs::remove_dir_all(&dir);
}

/// No stubbing: the companion's real signature brings its real implicit
/// clause with it, and without an `ExecutionContext` the call is rejected --
/// as scalac rejects it.
#[test]
fn fixtures_ckind_future_bad_is_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not obtainable");
        return;
    };
    compile_fails(
        "ckind_future_bad",
        &["--scala-library", jar.to_str().unwrap()],
        "could not find implicit value of type ExecutionContext",
    );
}

// -------------------------------- 2. the companion/class split, from a jar

/// cats' shape in miniature, and the only shape that reproduces the bug: a
/// higher-kinded trait, a companion object, and a *package object* whose
/// `val Ref = tinyeff.Ref` puts `L/tinyeff/Ref$;` into a descriptor. Reading
/// that descriptor is what used to enter the trait's name pointing at the
/// companion's class file.
///
/// `update`'s result type is the point: `F[Unit]` in the pickle, plain `TF;`
/// in the class file.
const TINY_LIB: &str = r#"
package tinyeff

trait Ref[F[_], A] {
  def get: F[A]
  def update(f: A => A): F[Unit]
}

object Ref {
  private final class Const[F[_], A](g: F[A], u: F[Unit]) extends Ref[F, A] {
    def get: F[A] = g
    def update(f: A => A): F[Unit] = u
  }

  def const[F[_], A](g: F[A], u: F[Unit]): Ref[F, A] = new Const(g, u)
}
"#;

/// `val Ref = tinyeff.Ref` is the descriptor that names `tinyeff/Ref$`, and
/// `type Ref[F[_], A] = tinyeff.Ref[F, A]` is the alias that then has to find
/// the *trait*. cats.effect's package object is exactly this.
const TINY_PKG: &str = r#"
package tinyeff

package object alias {
  type Ref[F[_], A] = tinyeff.Ref[F, A]
  val Ref = tinyeff.Ref
}
"#;

const TINY_USER: &str = r#"
import tinyeff.alias.Ref

object Main {
  def bump[F[_]](r: Ref[F, Int]): F[Unit] = r.update(_ + 1)

  def main(args: Array[String]): Unit = {
    val r = Ref.const[Option, Int](Some(3), Some(()))
    println(bump(r))
    println(r.get)
  }
}
"#;

/// The trait has no `bogus`, and neither does the object: entering the two as
/// separate symbols must not invent a member on either.
const TINY_USER_BAD: &str = r#"
import tinyeff.alias.Ref

object Main {
  def main(args: Array[String]): Unit =
    println(Ref.const[Option, Int](Some(3), Some(())).bogus)
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
fn a_companion_and_its_class_are_separate_symbols() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip: scala-library jar not present");
        return;
    };
    let Some(scalac) = scalac() else {
        eprintln!("skip: no scalac to write the pickle with");
        return;
    };
    if jar_tool().is_none() {
        eprintln!("skip: no `jar` tool");
        return;
    }
    let dir = tmp_dir("tinyeff");
    let lib = dir.join("lib.scala");
    let pkg = dir.join("pkg.scala");
    let user = dir.join("user.scala");
    let bad = dir.join("bad.scala");
    fs::write(&lib, TINY_LIB).unwrap();
    fs::write(&pkg, TINY_PKG).unwrap();
    fs::write(&user, TINY_USER).unwrap();
    fs::write(&bad, TINY_USER_BAD).unwrap();
    let lib_out = dir.join("libout");
    fs::create_dir_all(&lib_out).unwrap();

    let out = Command::new(&scalac)
        .arg("-d")
        .arg(&lib_out)
        .args([&lib, &pkg])
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
    assert!(!ok, "expected the bogus member to be rejected:\n{msgs}");
    assert!(
        msgs.contains("bogus"),
        "expected the error to name the missing member, got:\n{msgs}"
    );

    if java_available() {
        let cp = format!("{}:{}", jar.display(), lib_jar.display());
        assert_eq!(run_java(&user_out, Some(&cp)), "Some(())\nSome(3)\n");
    }
    let _ = fs::remove_dir_all(&dir);
}
