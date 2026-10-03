//! A partial-function literal over a value class takes its argument as the
//! box, while its parameter is erased to the underlying value. `applyOrElse`
//! and `isDefinedAt` read the box's field; unboxing the box itself as the
//! underlying value threw `ClassCastException` (`Sid` cannot be cast to
//! `Integer`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
final case class Sid(value: Int) extends AnyVal
final case class Tag(value: String) extends AnyVal
object Main {
  def main(args: Array[String]): Unit = {
    val ids: Seq[Sid] = Seq(Sid(1), Sid(2), Sid(999))
    val known: Map[Sid, String] = Map(Sid(1) -> "a", Sid(2) -> "b")
    println(ids.collect { case s if known.contains(s) => s.value })
    val pf: PartialFunction[Tag, Int] = { case t if t.value.nonEmpty => t.value.length }
    println((pf.isDefinedAt(Tag("")), pf.isDefinedAt(Tag("ab")), Seq(Tag("x"), Tag("")).collect(pf)))
  }
}
"#;

#[test]
fn partial_function_reads_a_value_class_argument_from_its_box() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("partial-function-value-class-param");
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
    assert_eq!(expected, "List(1, 2)\n(false,true,List(1))\n");
    assert_eq!(run(&ours), expected);
}
