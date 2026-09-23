//! A binary conversion's inner result keeps its applied outer type.

use crate::support::{toolchain, TestDir};
use std::{fs, process::Command};

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
