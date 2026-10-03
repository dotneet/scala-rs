//! A value class's extractor pattern inside another pattern (`case (i,
//! Code(s))`, the tuple holding the `Code` box). Erasing the pattern as an
//! expression wrapped the inner part in `$vcunbox`, so the backend unwrapped
//! the box and then tested the `String` for a `Code` again: the case never
//! matched (`MatchError`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
case class Offset(value: Int) extends AnyVal
case class Code(value: String) extends AnyVal
object Main {
  def drop(xs: List[Int], offset: Option[Offset]): List[Int] = offset.foldLeft(xs) { case (q, Offset(n)) => q.drop(n) }
  def main(args: Array[String]): Unit = {
    println(drop(List(1, 2, 3), Some(Offset(1))))
    println((1, Code("c")) match { case (i, Code(s)) => s * i })
    println(Option(Offset(4)) match { case Some(Offset(n)) => n + 1; case None => 0 })
    println(List(Offset(5)).map { case Offset(n) => n * 2 })
  }
}
"#;

#[test]
fn value_class_extractor_inside_a_tuple_pattern_matches() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("nested-value-class-pattern");
    let source = dir.join("Main.scala");
    fs::write(&source, SOURCE).unwrap();
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&ours).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    CompileCommand::new(&source, &ours)
        .classpath(library)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let output = Command::new(scalac)
        .arg("-d")
        .arg(&theirs)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = |classes: &std::path::Path| {
        let out = Command::new(java)
            .args(["-Xverify:all", "-cp"])
            .arg(format!("{}:{}", classes.display(), library.display()))
            .arg("Main")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let expected = run(&theirs);
    assert_eq!(expected, "List(2, 3)\nc\n5\nList(10)\n");
    assert_eq!(run(&ours), expected);
}
