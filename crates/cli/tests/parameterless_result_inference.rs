//! Open result parameters of nullary methods survive a relaxed lambda prototype.

use crate::support::{toolchain, TestDir};
use std::{fs, process::Command};

#[test]
fn enclosing_call_solves_parameterless_result_through_lambda() {
    let Some(library) = toolchain().scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    let dir = TestDir::new("parameterless-result-inference");
    let source = dir.join("ParameterlessResult.scala");
    let classes = dir.join("classes");
    fs::create_dir(&classes).unwrap();
    fs::write(
        &source,
        r#"
class Future[+A] {
  def map[B](f: A => B): Future[B] = ???
}
case class Wrapped[F[_], A, B](value: F[Either[A, B]])
class RightOps[B](val value: B) {
  def asRight[A]: Either[A, B] = Right(value)
}
object ParameterlessResult {
  def fetch(input: Future[Option[Int]]): Wrapped[Future, Nothing, Option[Int]] =
    Wrapped {
      input.map { records => new RightOps(records).asRight }
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
}
