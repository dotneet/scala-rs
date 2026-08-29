//! Reading a `-cp` **jar**'s Scala classes from their `ScalaSignature` pickle.
//!
//! `load_classpath` only walks directories, so a class that lives in a jar used
//! to reach the typer through `install_java_class` — that is, through its JVM
//! *generic signature*. That format cannot write a higher kind: `trait
//! Monad[F[_]]` arrives as `Monad[F]` with `F` a proper type, and `def
//! pure[A](a: A): F[A]` arrives as `(A)F`. Every `Monad[F]` was then a kind
//! error and every `F.pure(v)` a `found: F required: F[Int]`.
//!
//! The pickle has the real signature, and `PickleSupply::adopt_binary_class`
//! reads it — lazily, one class at a time, as the classfile is loaded. These
//! tests pin that down three ways: the source-level meaning (`jarpk` fixture),
//! the same shapes crossing a jar boundary, and a real cats/cats-effect jar
//! from the classpath when one is cached locally.

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
    let p = std::env::temp_dir().join(format!(
        "scala-rs-jarpickle-{tag}-{}-{nanos}",
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

/// The JDK's `jar`, next to the `java` on `JAVA_HOME` or on `PATH`.
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

/// Pack a directory of classfiles into a jar, so the compiler has to read them
/// out of a zip rather than off the filesystem.
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

fn compile(out: &Path, jar: &Path, srcs: &[PathBuf], extra_cp: &[PathBuf]) -> (bool, String) {
    let mut cmd = Command::new(bin());
    cmd.arg("compile");
    for s in srcs {
        cmd.arg(s);
    }
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

fn run_main(out: &Path, jar: &Path, extra_cp: &[PathBuf]) -> String {
    let mut cp = format!("{}:{}", out.display(), jar.display());
    for p in extra_cp {
        cp.push(':');
        cp.push_str(&p.display().to_string());
    }
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

// ------------------------------------------------------------------ fixtures

#[test]
fn jarpk_fixture_dual_run() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip jarpk: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("jarpk.scala");
    let out = tmp_dir("jarpk");
    let (ok, msgs) = compile(&out, &jar, &[src], &[]);
    assert!(ok, "compile jarpk failed:\n{msgs}");
    if java_available() {
        let expected = fs::read_to_string(fixtures_dir().join("expected/jarpk.txt")).unwrap();
        assert_eq!(run_main(&out, &jar, &[]), expected, "stdout mismatch");
    }
    let _ = fs::remove_dir_all(&out);
}

/// Recovering a kind from a pickle is only worth something if a wrong one is
/// still an error; nsc 2.13.16 rejects both of these too.
#[test]
fn jarpk_bad_is_still_rejected() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip jarpk_bad: scala-library jar not present");
        return;
    };
    let src = fixtures_dir().join("jarpk_bad.scala");
    let out = tmp_dir("jarpk_bad");
    let (ok, msgs) = compile(&out, &jar, &[src], &[]);
    assert!(!ok, "expected jarpk_bad to fail, got:\n{msgs}");
    for needle in [
        "type constructor takes type parameters, but Int does not",
        "type mismatch; found: F[Int]  required: F[String]",
    ] {
        assert!(msgs.contains(needle), "expected {needle:?} in:\n{msgs}");
    }
    let _ = fs::remove_dir_all(&out);
}

// ------------------------------------------------------------ jar round trip

const HK_LIB: &str = "\
package hklib

trait Functor[F[_]] {
  def fmap[A, B](fa: F[A], f: A => B): F[B]
}

trait Monadic[F[_]] extends Functor[F] {
  def pure[A](a: A): F[A]
  def bind[A, B](fa: F[A], f: A => F[B]): F[B]
}

object Instances {
  val optionMonadic: Monadic[Option] = new Monadic[Option] {
    def pure[A](a: A): Option[A] = Some(a)
    def bind[A, B](fa: Option[A], f: A => Option[B]): Option[B] = fa.flatMap(f)
    def fmap[A, B](fa: Option[A], f: A => B): Option[B] = fa.map(f)
  }
}
";

const HK_USER: &str = "\
import hklib.{Functor, Monadic}

object OptM extends Monadic[Option] {
  def pure[A](a: A): Option[A] = Some(a)
  def bind[A, B](fa: Option[A], f: A => Option[B]): Option[B] = fa.flatMap(f)
  def fmap[A, B](fa: Option[A], f: A => B): Option[B] = fa.map(f)
}

object Main {
  def liftInt[F[_]](n: Int, F: Monadic[F]): F[Int] = F.pure(n)
  def chain[F[_]](fa: F[Int], F: Monadic[F]): F[Int] =
    F.bind(fa, (n: Int) => F.pure(n * 10))
  def describe[F[_]](fa: F[Int], F: Functor[F]): F[String] =
    F.fmap(fa, (n: Int) => \"n=\" + n.toString)

  def main(args: Array[String]): Unit = {
    val m: Monadic[Option] = OptM
    println(liftInt[Option](7, m))
    println(chain[Option](Option(4), m))
    println(describe[Option](Option(3), m))
    println(hklib.Instances.optionMonadic.pure(5))
  }
}
";

/// Compile the library, pack it into a jar, then compile and run a program that
/// only ever sees the jar. Nothing but the `ScalaSignature` crosses.
#[test]
fn a_higher_kinded_trait_survives_a_jar_round_trip() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip jar round trip: scala-library jar not present");
        return;
    };
    if jar_tool().is_none() {
        eprintln!("skip jar round trip: no `jar` tool");
        return;
    }
    let dir = tmp_dir("roundtrip");
    let lib_src = dir.join("lib.scala");
    let user_src = dir.join("user.scala");
    fs::write(&lib_src, HK_LIB).unwrap();
    fs::write(&user_src, HK_USER).unwrap();
    let lib_out = dir.join("libout");
    let user_out = dir.join("userout");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&user_out).unwrap();

    let (ok, msgs) = compile(&lib_out, &jar, &[lib_src], &[]);
    assert!(ok, "library failed to compile:\n{msgs}");
    let lib_jar = dir.join("hklib.jar");
    pack_jar(&lib_out, &lib_jar);

    let (ok, msgs) = compile(&user_out, &jar, &[user_src], std::slice::from_ref(&lib_jar));
    assert!(ok, "user failed to compile against the jar:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");

    if java_available() {
        assert_eq!(
            run_main(&user_out, &jar, &[lib_jar]),
            "Some(7)\nSome(40)\nSome(n=3)\nSome(5)\n"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------ real cats

/// cats-core / cats-kernel / cats-effect-kernel from the local Coursier cache,
/// if they happen to be there. Nothing is downloaded.
fn cats_jars() -> Option<Vec<PathBuf>> {
    let home = std::env::var("HOME").ok()?;
    let roots = [
        PathBuf::from(&home).join("Library/Caches/Coursier/v1/https/repo1.maven.org/maven2"),
        PathBuf::from(&home).join(".cache/coursier/v1/https/repo1.maven.org/maven2"),
    ];
    let wanted = [
        ("org/typelevel/cats-core_2.13", "cats-core_2.13"),
        ("org/typelevel/cats-kernel_2.13", "cats-kernel_2.13"),
    ];
    let mut out = Vec::new();
    for (rel, prefix) in wanted {
        let mut found = None;
        for root in &roots {
            let base = root.join(rel);
            let Ok(rd) = fs::read_dir(&base) else {
                continue;
            };
            for ent in rd.flatten() {
                let version = ent.file_name().to_string_lossy().into_owned();
                let candidate = ent.path().join(format!("{prefix}-{version}.jar"));
                if candidate.is_file() {
                    found = Some(candidate);
                }
            }
        }
        out.push(found?);
    }
    Some(out)
}

const CATS_USER: &str = "\
import cats.Monad

object Main {
  def liftInt[F[_]](n: Int)(implicit F: Monad[F]): F[Int] = F.pure(n)
  def chain[F[_]](fa: F[Int])(implicit F: Monad[F]): F[Int] =
    F.flatMap(fa)((n: Int) => F.pure(n * 10))
  def describe[F[_]](fa: F[Int])(implicit F: Monad[F]): F[String] =
    F.map(fa)((n: Int) => \"n=\" + n.toString)

  def main(args: Array[String]): Unit = println(\"compiled\")
}
";

/// `cats.Monad` is the case that started this: `F.pure(v)` reported
/// `found: F required: F[Int]`, because a JVM generic signature writes
/// `def pure[A](a: A): F[A]` as `(TA;)TF;`.
#[test]
fn a_higher_kinded_type_class_from_a_real_jar_typechecks() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip cats: scala-library jar not present");
        return;
    };
    let Some(cats) = cats_jars() else {
        eprintln!("skip cats: no cats jars in the local Coursier cache");
        return;
    };
    let dir = tmp_dir("cats");
    let src = dir.join("user.scala");
    fs::write(&src, CATS_USER).unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, &jar, &[src], &cats);
    assert!(ok, "cats program failed to compile:\n{msgs}");
    assert!(!msgs.contains("error:"), "unexpected diagnostics:\n{msgs}");
    let _ = fs::remove_dir_all(&dir);
}

/// The kinds have to come from cats' own pickle, not from a guess: `Monad`
/// takes a type *constructor*, and a proper type is still rejected.
#[test]
fn a_proper_type_is_still_rejected_where_a_real_jar_wants_a_constructor() {
    let Some(jar) = scala_library_jar() else {
        eprintln!("skip cats kind check: scala-library jar not present");
        return;
    };
    let Some(cats) = cats_jars() else {
        eprintln!("skip cats kind check: no cats jars in the local Coursier cache");
        return;
    };
    let dir = tmp_dir("catsbad");
    let src = dir.join("bad.scala");
    fs::write(
        &src,
        "import cats.Monad\nobject BadCats { def f(m: Monad[Int]): Int = 0 }\n",
    )
    .unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let (ok, msgs) = compile(&out, &jar, &[src], &cats);
    assert!(!ok, "expected Monad[Int] to be rejected, got:\n{msgs}");
    assert!(
        msgs.contains("type constructor takes type parameters, but Int does not"),
        "expected a kind error, got:\n{msgs}"
    );
    let _ = fs::remove_dir_all(&dir);
}
