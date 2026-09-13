//! Regressions for source-level implicits inherited through an external Scala
//! parent. The JVM class file exposes a concrete trait forwarder as an
//! ordinary method, while only the parent `ScalaSignature` records `implicit`.
//! The typer must restore that fact without changing lexical precedence or
//! adding a duplicate overload.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_scala-rs"))
}

fn scala_library_jar() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-rs-lib/scala-library-2.13.16.jar");
    p.is_file().then_some(p)
}

fn scalac() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/scala-2.13.16/bin/scalac");
    p.is_file().then_some(p)
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Keep the generated pickle fixture inspectable when this test fails in
    // the desktop runner (its process temp directory is otherwise namespaced
    // away from the shell used to investigate diagnostics).
    let p = PathBuf::from("/private/tmp").join(format!(
        "scala-rs-inherited-extension-{tag}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn jar_tool() -> Option<PathBuf> {
    let out = Command::new("which").arg("jar").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    p.is_file().then_some(p)
}

fn run_scalac(scalac: &Path, args: &[&str]) {
    let out = Command::new(scalac)
        .args(args)
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac failed:\n{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
}

fn run_scala_rs(
    src: &Path,
    out_dir: &Path,
    scala_library: &Path,
    classpath: &str,
) -> (bool, String) {
    let out = Command::new(bin())
        .args([
            "compile",
            src.to_str().unwrap(),
            "-d",
            out_dir.to_str().unwrap(),
            "-cp",
            classpath,
            "--scala-library",
            scala_library.to_str().unwrap(),
        ])
        .output()
        .expect("run scala-rs compile");
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        ),
    )
}

fn pack_jar(classes: &Path, jar: &Path) {
    let tool = jar_tool().expect("jar tool");
    let out = Command::new(tool)
        .args([
            "cf",
            jar.to_str().unwrap(),
            "-C",
            classes.to_str().unwrap(),
            ".",
        ])
        .output()
        .expect("run jar");
    assert!(
        out.status.success(),
        "jar failed:\n{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
}

/// A tiny external library is enough to expose the regression: `SuiteBase`
/// inherits the implicit conversion from `Syntax`, and the implementing class
/// file seen by scala-rs contains only a non-implicit JVM forwarder.
#[test]
fn inherited_implicit_extension_from_scala_parent_pickle() {
    let (Some(scala_library), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip inherited implicit extension: Scala 2.13.16 toolchain is absent");
        return;
    };
    let root = tmp_dir("parent");
    let lib_src = root.join("lib.scala");
    let app_src = root.join("app.scala");
    let lib_classes = root.join("lib-classes");
    let lib_jar = root.join("lib.jar");
    let rs_out = root.join("scala-rs-out");
    fs::create_dir_all(&lib_classes).unwrap();
    fs::create_dir_all(&rs_out).unwrap();
    fs::write(
        &lib_src,
        r#"package inherited

class WrappedString(value: String) {
  def when(body: => Unit): Unit = ()
}

trait Syntax {
  implicit def stringToWrapped(value: String): WrappedString = new WrappedString(value)
}

trait SuiteBase extends Syntax
"#,
    )
    .unwrap();
    fs::write(
        &app_src,
        r#"import inherited.SuiteBase

class InheritedSpec extends SuiteBase {
  "outer" when {}
}
"#,
    )
    .unwrap();
    run_scalac(
        &scalac,
        &[
            "-d",
            lib_classes.to_str().unwrap(),
            lib_src.to_str().unwrap(),
        ],
    );
    pack_jar(&lib_classes, &lib_jar);

    let classpath = format!("{}:{}", lib_jar.display(), scala_library.display());
    let (ok, diagnostics) = run_scala_rs(&app_src, &rs_out, &scala_library, &classpath);
    assert!(
        ok,
        "scala-rs rejected an inherited implicit conversion from a parent pickle:\n{diagnostics}"
    );
    assert!(
        rs_out.join("InheritedSpec.class").is_file(),
        "scala-rs did not emit InheritedSpec.class"
    );
    let _ = fs::remove_dir_all(root);
}

/// A pickled implicit conversion may itself mention a Java generic type that
/// nothing else has loaded yet. Twirl's helper converts
/// `java.lang.Iterable[T]` to Scala `Iterable[T]`; GitBucket first exposes its
/// receiver as the generic result of a Java method (`java.util.List[Version]`).
/// Both Java symbols must be completed while reading the pickle, and the
/// receiver hierarchy must be complete before view applicability is tested.
#[test]
fn pickled_view_accepts_inferred_java_generic_result() {
    let (Some(scala_library), Some(scalac)) = (scala_library_jar(), scalac()) else {
        eprintln!("skip Java generic view probe: Scala 2.13.16 toolchain is absent");
        return;
    };
    let root = tmp_dir("java-generic");
    let lib_src = root.join("lib.scala");
    let app_src = root.join("app.scala");
    let lib_classes = root.join("lib-classes");
    let lib_jar = root.join("lib.jar");
    let rs_out = root.join("scala-rs-out");
    fs::create_dir_all(&lib_classes).unwrap();
    fs::create_dir_all(&rs_out).unwrap();
    fs::write(
        &lib_src,
        r#"package javageneric

class Provider {
  def values: java.util.List[String] = null
}

object Syntax {
  implicit def javaIterableToScala[T](value: java.lang.Iterable[T]): scala.collection.Iterable[T] =
    null
}
"#,
    )
    .unwrap();
    fs::write(
        &app_src,
        r#"package javageneric

import _root_.javageneric.Syntax._

object Use {
  def lastValue(provider: Provider): String = provider.values.last
}
"#,
    )
    .unwrap();
    run_scalac(
        &scalac,
        &[
            "-d",
            lib_classes.to_str().unwrap(),
            lib_src.to_str().unwrap(),
        ],
    );
    pack_jar(&lib_classes, &lib_jar);

    let classpath = format!("{}:{}", lib_jar.display(), scala_library.display());
    let (ok, diagnostics) = run_scala_rs(&app_src, &rs_out, &scala_library, &classpath);
    assert!(
        ok,
        "scala-rs rejected a pickled view over an inferred Java generic result:\n{diagnostics}"
    );
    assert!(
        rs_out.join("javageneric/Use.class").is_file(),
        "scala-rs did not emit javageneric.Use"
    );
    let _ = fs::remove_dir_all(root);
}

fn scalatest_classpath() -> Option<String> {
    let jars = [
        "/Users/shinji/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/org/scalatest/scalatest-wordspec_2.13/3.2.20/scalatest-wordspec_2.13-3.2.20.jar",
        "/Users/shinji/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/org/scalatest/scalatest-core_2.13/3.2.20/scalatest-core_2.13-3.2.20.jar",
        "/Users/shinji/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/org/scalatest/scalatest-shouldmatchers_2.13/3.2.20/scalatest-shouldmatchers_2.13-3.2.20.jar",
        "/Users/shinji/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/org/scalatest/scalatest-matchers-core_2.13/3.2.20/scalatest-matchers-core_2.13-3.2.20.jar",
        "/Users/shinji/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/org/scalatest/scalatest-compatible/3.2.20/scalatest-compatible-3.2.20.jar",
        "/Users/shinji/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/org/scalactic/scalactic_2.13/3.2.20/scalactic_2.13-3.2.20.jar",
        "/Users/shinji/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2/org/scala-lang/scala-reflect/2.13.18/scala-reflect-2.13.18.jar",
    ];
    jars.iter()
        .all(|p| Path::new(p).is_file())
        .then(|| jars.join(":"))
}

/// The real Scalatest 3.2.20 hierarchy combines both the inherited
/// `AnyWordSpecLike` conversion and `Matchers`' own conversions. This keeps
/// the end-to-end CLI path covered after the dependency-independent fixture.
#[test]
fn scalatest_any_word_spec_and_matchers_dsl() {
    let (Some(scala_library), Some(classpath)) = (scala_library_jar(), scalatest_classpath())
    else {
        eprintln!("skip Scalatest DSL: cached Scalatest 3.2.20 jars are absent");
        return;
    };
    let root = tmp_dir("scalatest");
    let src = root.join("tiny.scala");
    let out = root.join("out");
    fs::create_dir_all(&out).unwrap();
    fs::write(
        &src,
        r#"import org.scalatest.wordspec.AnyWordSpec
	import org.scalatest.matchers.should.Matchers
	import org.scalactic.Prettifier
	import org.scalactic.source.Position

class TinyAnyWordSpec extends AnyWordSpec with Matchers {
	implicit val sourcePosition: Position = Position("tiny.scala", "", 1)
	implicit val sourcePrettifier: Prettifier = Prettifier.default

  "outer" when {
    "inner" should {
      "works" in {}
    }
  }
}
"#,
    )
    .unwrap();
    let (ok, diagnostics) = run_scala_rs(&src, &out, &scala_library, &classpath);
    assert!(
        ok,
        "scala-rs rejected the Scalatest AnyWordSpec/Matchers DSL:\n{diagnostics}"
    );
    assert!(
        out.join("TinyAnyWordSpec.class").is_file(),
        "scala-rs did not emit TinyAnyWordSpec.class"
    );
    let _ = fs::remove_dir_all(root);
}
