//! A tag materialized inside a derived implicit instantiates the call's
//! undetermined type parameter at its lower bound, as nsc does.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::fs;

#[test]
fn derived_class_tag_evidence_solves_the_undetermined_parameter_to_nothing() {
    let tools = toolchain();
    let Some(library) = tools.scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    if !tools.has_java() {
        eprintln!("skip: Java is unavailable");
        return;
    }
    let dir = TestDir::new("implicit-derived-tag-undet");
    let source = dir.join("Main.scala");
    let classes = dir.join("classes");
    fs::create_dir(&classes).unwrap();
    fs::write(
        &source,
        r#"
import scala.reflect.ClassTag
class Ev[A](val name: String)
class Box[+A](val v: String) { override def toString = "Box(" + v + ")" }
class IBox[A](val v: String) { override def toString = "IBox(" + v + ")" }
object Main {
  implicit def evAny[A](implicit ct: ClassTag[A]): Ev[A] = new Ev[A](ct.toString)
  def make[A](x: String)(implicit ev: Ev[A]): Box[A] = new Box[A](x + ":" + ev.name)
  def imake[A](x: String)(implicit ev: Ev[A]): IBox[A] = new IBox[A](x + ":" + ev.name)
  def tag[A](x: String)(implicit ct: ClassTag[A]): Box[A] = new Box[A](x + ":" + ct.toString)
  def main(args: Array[String]): Unit = {
    val b: Box[Long] = make("hi")
    println(b)
    val c: Box[Any] = make("hi")
    println(c)
    val d: IBox[String] = imake("hi")
    println(d)
    val e: Box[AnyRef] = make("hi")
    println(e)
    println(make("x"))
    val t: Box[Long] = tag("t")
    println(t)
  }
}
"#,
    )
    .unwrap();

    CompileCommand::new(&source, &classes)
        .scala_library(library)
        .run()
        .assert_success("compile");
    let run = RunCommand::new("Main")
        .classpath(format!("{}:{}", classes.display(), library.display()))
        .run();
    run.assert_success("run");
    // scalac 2.13.16 output.
    assert_eq!(
        run.stdout_string(),
        "Box(hi:Nothing)\n\
         Box(hi:Nothing)\n\
         IBox(hi:java.lang.String)\n\
         Box(hi:Nothing)\n\
         Box(x:Nothing)\n\
         Box(t:Nothing)\n"
    );
}
