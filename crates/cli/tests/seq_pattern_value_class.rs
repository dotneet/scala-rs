//! A value class bound out of a sequence pattern (`case Seq(c)`, `case List(c)`)
//! comes out of the sequence's generic slot as its box. It was cast to the
//! underlying class instead (`ClassCastException: Code cannot be cast to
//! String`), where nsc unwraps it through the accessor.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
case class Code(value: String) extends AnyVal
final class Wrap[A](val a: A) extends AnyVal { override def toString = "Wrap(" + a + ")" }
object Main {
  def run(xs: Seq[String], m: Map[Code, Int]): String = xs.map(v => Code(v)) match {
    case Seq(c) => "one " + m.getOrElse(c, 0) + " " + c.value
    case cs => "many " + cs.map(c => m.getOrElse(c, 0)).sum
  }
  def main(args: Array[String]): Unit = {
    val m = Map(Code("x") -> 1, Code("y") -> 2)
    println((run(List("x"), m), run(List("x", "y"), m)))
    println(List(Code("a")) match { case List(c) => c.value; case _ => "" })
    println(Seq(Code("b"), Code("c")) match { case Seq(c, rest @ _*) => (c, rest); case _ => "" })
    println(Option(Code("d")) match { case Some(c) => c.value.length; case None => 0 })
    println(Vector(new Wrap(1), new Wrap(2)) match { case Seq(w, _) => w.a + 1; case _ => 0 })
  }
}
"#;

#[test]
fn sequence_pattern_unwraps_a_value_class() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("seq-pattern-value-class");
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
    assert_eq!(
        expected,
        "(one 1 x,many 3)\na\n(Code(b),List(Code(c)))\n1\n2\n"
    );
    assert_eq!(run(&ours), expected);
}
