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
