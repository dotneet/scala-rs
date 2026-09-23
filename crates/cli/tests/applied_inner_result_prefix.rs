//! A binary conversion's inner result keeps its applied outer type.

use crate::support::{toolchain, TestDir};
use std::{fs, path::PathBuf, process::Command};

#[test]
fn scala_library_ordering_ops_uses_outer_type_argument() {
    let tools = toolchain();
    let (Some(library), Some(java)) = (tools.scala_library(), tools.java()) else {
        eprintln!("skip: scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("applied-inner-result-prefix");
    let source = dir.join("OrderingClient.scala");
    let classes = dir.join("classes");
    fs::create_dir(&classes).unwrap();
    fs::write(
        &source,
        r#"
import java.time.ZonedDateTime
import scala.math.Ordering.Implicits.infixOrderingOps

object OrderingClient {
  def choose(a: ZonedDateTime, b: ZonedDateTime): ZonedDateTime = a.min(b)
  def main(args: Array[String]): Unit = {
    val early = ZonedDateTime.parse("2020-01-01T00:00:00Z")
    val late = ZonedDateTime.parse("2021-01-01T00:00:00Z")
    assert(choose(late, early) == early)
  }
}
"#,
    )
    .unwrap();

    let compile = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .args(["compile"])
        .arg(&source)
        .arg("--scala-library")
        .arg(library)
        .arg("-d")
        .arg(&classes)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let classpath = format!("{}:{}", classes.display(), library.display());
    let run = Command::new(java)
        .args(["-cp", &classpath, "OrderingClient"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "run failed:\n{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn binary_mixin_result_uses_the_concrete_higher_kinded_alias() {
    let tools = toolchain();
    let Some(library) = tools.scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    let Some(home) = std::env::var_os("HOME") else {
        eprintln!("skip: home directory is unavailable");
        return;
    };
    let artifacts = [
        "org/apache/pekko/pekko-stream_2.13/1.1.2/pekko-stream_2.13-1.1.2.jar",
        "org/apache/pekko/pekko-actor_2.13/1.1.2/pekko-actor_2.13-1.1.2.jar",
        "com/typesafe/config/1.4.5/config-1.4.5.jar",
        "org/reactivestreams/reactive-streams/1.0.4/reactive-streams-1.0.4.jar",
    ];
    let roots = ["Library/Caches/Coursier/v1", ".cache/coursier/v1"];
    let mut jars = Vec::new();
    for artifact in artifacts {
        let jar = roots
            .iter()
            .map(|root| {
                PathBuf::from(&home)
                    .join(root)
                    .join("https/repo1.maven.org/maven2")
                    .join(artifact)
            })
            .find(|jar| jar.is_file());
        let Some(jar) = jar else {
            eprintln!("skip: {artifact} is not cached");
            return;
        };
        jars.push(jar);
    }
    let dir = TestDir::new("binary-mixin-result-alias");
    let classes = dir.join("classes");
    fs::create_dir(&classes).unwrap();
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/pekko_flowops.scala");
    let classpath = jars
        .iter()
        .map(|jar| jar.to_string_lossy())
        .collect::<Vec<_>>()
        .join(":");
    let compile = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
        .arg("compile")
        .arg(source)
        .args(["-cp", &classpath, "--scala-library"])
        .arg(library)
        .arg("-d")
        .arg(&classes)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
}

/// `Outer.this.Repr[A]` returned by a method of a nested class names the
/// *enclosing* class's member. Reading it on the nested class picked up a
/// same-named alias there (`Vector`), so a program scalac rejects compiled and
/// failed with `ClassCastException`, and the correct one was rejected.
#[test]
fn enclosing_this_result_member_is_not_the_nested_class_alias() {
    let tools = toolchain();
    let (Some(library), Some(java), Some(scalac)) =
        (tools.scala_library(), tools.java(), tools.scalac())
    else {
        eprintln!("skip: scala-library, Java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("enclosing-this-result-member");
    let lib_classes = dir.join("lib");
    fs::create_dir(&lib_classes).unwrap();
    let lib_source = dir.join("Lib.scala");
    fs::write(
        &lib_source,
        r#"
package lib
trait Outer {
  type Repr[A] <: Seq[A]
  def mk[A](a: A): Repr[A]
  class Inner {
    type Repr[A] = Vector[A]
    def get[A](a: A): Outer.this.Repr[A] = mk(a)
  }
}
class Sub extends Outer {
  type Repr[A] = List[A]
  def mk[A](a: A): List[A] = List(a)
}
"#,
    )
    .unwrap();
    let lib = Command::new(scalac)
        .arg("-d")
        .arg(&lib_classes)
        .arg(&lib_source)
        .output()
        .unwrap();
    assert!(
        lib.status.success(),
        "scalac failed:\n{}",
        String::from_utf8_lossy(&lib.stderr)
    );

    let compile = |name: &str, body: &str| {
        let source = dir.join(format!("{name}.scala"));
        let classes = dir.join(format!("{name}-classes"));
        fs::create_dir(&classes).unwrap();
        fs::write(&source, body).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_scala-rs"))
            .arg("compile")
            .arg(&source)
            .arg("-cp")
            .arg(&lib_classes)
            .arg("--scala-library")
            .arg(library)
            .arg("-d")
            .arg(&classes)
            .output()
            .unwrap();
        (out, classes)
    };

    let (good, classes) = compile(
        "Good",
        r#"
object Good {
  def main(args: Array[String]): Unit = {
    val o = new lib.Sub
    val i = new o.Inner
    val x: List[Int] = i.get(1)
    println(x.head + 2)
  }
}
"#,
    );
    assert!(
        good.status.success(),
        "compile failed:\n{}{}",
        String::from_utf8_lossy(&good.stdout),
        String::from_utf8_lossy(&good.stderr)
    );
    let classpath = format!(
        "{}:{}:{}",
        classes.display(),
        lib_classes.display(),
        library.display()
    );
    let run = Command::new(java)
        .args(["-cp", &classpath, "Good"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "run failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "3");

    let (bad, _) = compile(
        "Bad",
        r#"
object Bad {
  val o = new lib.Sub
  val i = new o.Inner
  val x: Vector[Int] = i.get(1)
}
"#,
    );
    assert!(
        !bad.status.success(),
        "Outer.this.Repr[Int] was accepted as the nested class's Vector[Int]"
    );
}
