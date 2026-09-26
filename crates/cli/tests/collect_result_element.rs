//! `collect`'s rebuilt result element comes from a partial function argument
//! only. `java.util.stream.Stream.collect(Collector[_ >: T, A, R]): R` (and
//! any other generic `collect` over a non-function) keeps its declared `R`.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
import java.util.ArrayList
import java.util.stream.{Collector, Collectors}

trait Col[T, A, R] { def result: R }
class Box[T](val t: T) {
  def collect[R, A](c: Col[T, A, R]): R = c.result
  def collect[B](pf: PartialFunction[T, B]): Box[B] = new Box(pf(t))
}

object Main {
  def main(args: Array[String]): Unit = {
    val al = new ArrayList[Integer]()
    al.add(1); al.add(2); al.add(3)
    val joined: String =
      al.stream().map[String](x => x.toString).collect(Collectors.joining(";"))
    val s = al.stream().map[String](x => x.toString).collect(Collectors.joining(","))
    println(joined + " " + s.length)
    val c: Collector[Integer, _, java.util.List[Integer]] = Collectors.toList[Integer]()
    println(al.stream().collect(c).size)
    val box = new Box("abc")
    val r = box.collect(new Col[String, Int, String] { def result = "col" })
    println(r.length)
    println(box.collect({ case s: String => s.length }).t + 1)
  }
}
"#;

#[test]
fn collect_over_a_non_function_keeps_its_declared_result() {
    let tools = toolchain();
    let (Some(library), Some(scalac)) = (tools.scala_library(), tools.scalac()) else {
        eprintln!("skip: scala-library or scalac is unavailable");
        return;
    };
    if !tools.has_java() {
        eprintln!("skip: Java is unavailable");
        return;
    }
    let dir = TestDir::new("collect-result-element");
    let source = dir.join("Main.scala");
    fs::write(&source, SOURCE).unwrap();

    let reference = dir.join("scalac");
    fs::create_dir(&reference).unwrap();
    let status = Command::new(scalac)
        .arg("-d")
        .arg(&reference)
        .arg(&source)
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac failed");
    let expected = RunCommand::new("Main")
        .classpath(format!("{}:{}", reference.display(), library.display()))
        .run();
    expected.assert_success("run the scalac build");

    let ours = dir.join("scala-rs");
    fs::create_dir(&ours).unwrap();
    CompileCommand::new(&source, &ours)
        .scala_library(library)
        .run()
        .assert_success("compile with scala-rs");
    let actual = RunCommand::new("Main")
        .classpath(format!("{}:{}", ours.display(), library.display()))
        .run();
    actual.assert_success("run the scala-rs build");
    assert_eq!(actual.stdout_string(), expected.stdout_string());
}
