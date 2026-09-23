//! Expected intersections must keep a shared component out of inference for a new one.

use crate::support::{toolchain, TestDir};
use std::{fs, process::Command};

#[test]
fn flat_map_infers_new_effect_from_remaining_intersection_component() {
    let Some(library) = toolchain().scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    let dir = TestDir::new("intersection-expected");
    let source = dir.join("IntersectionExpected.scala");
    let classes = dir.join("classes");
    fs::create_dir(&classes).unwrap();
    fs::write(
        &source,
        r#"
trait Effect
trait Write extends Effect

class Action[R, E <: Effect] {
  def flatMap[R2, E2 <: Effect](f: R => Action[R2, E2]): Action[R2, E with E2] = ???
  def reverse[R2, E2 <: Effect](f: R => Action[R2, E2]): Action[R2, E2 with E] = ???
}

object IntersectionExpected {
  def left[E <: Effect](a: Action[Int, E]): Action[Int, E with Write] =
    a.flatMap { value => new Action[Int, Write] }

  def right[E <: Effect](a: Action[Int, E]): Action[Int, Write with E] =
    a.reverse { value => new Action[Int, Write] }
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
