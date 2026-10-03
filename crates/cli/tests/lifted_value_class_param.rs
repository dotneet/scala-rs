//! A local `def` inside a lambda whose parameter is a value class, reading
//! that parameter. Lambda lifting makes the lambda's parameter a parameter
//! of the lifted method, and the lambda holds it boxed; the method took it
//! as the underlying value while its body (a lambda capturing it again)
//! read it as the box, and the class failed verification (`VerifyError:
//! Bad type on operand stack`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
final case class Score(value: Float) extends AnyVal
final case class Item(t: Score)
object Main {
  def lazyLocal(xs: List[Float], o: Option[Score]) = o.map { my => lazy val n = xs.filter(_ > my.value); n }
  def defLocal(xs: List[Float], o: Option[Score]) = o.map { my => def n = xs.filter(_ > my.value); n }
  def boxedUse(xs: List[Float], o: Option[Score]) = o.map { my => def n = xs.map(_ => my); n }
  def direct(xs: List[Float], o: Option[Score]) = o.map { my => def n = my.value + xs.size; n }
  def forComp(xs: List[Item], o: Option[Score]): Option[Int] = for {
    my <- o
    x <- {
      lazy val next = xs.withFilter(_.t.value > my.value).map(_.t.value.toInt).headOption
      if (xs.isEmpty) None else next
    }
  } yield x
  def main(args: Array[String]): Unit = {
    val xs = List(1f, 3f)
    val s = Some(Score(2f))
    println(List(lazyLocal(xs, s), defLocal(xs, s), boxedUse(xs, s), direct(xs, s)).mkString(" "))
    println(forComp(List(Item(Score(3f)), Item(Score(1f))), s))
  }
}
"#;

#[test]
fn lifted_local_def_takes_the_boxed_lambda_parameter() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("lifted-value-class-param");
    let source = dir.join("A.scala");
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
    assert_eq!(
        expected,
        "Some(List(3.0)) Some(List(3.0)) Some(List(Score(2.0), Score(2.0))) Some(4.0)\nSome(3)\n"
    );
    assert_eq!(run(&ours), expected);
}
