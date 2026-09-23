//! A parameterless polymorphic method whose result is an intersection
//! (`def mk[A]: A with Marker`) is instantiated from the expected type, and
//! a result minimised to `Nothing` is cast to the expected type rather than
//! treated as a call that throws.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::fs;

#[test]
fn nullary_polymorphic_intersection_result_meets_the_expected_type() {
    let tools = toolchain();
    let Some(library) = tools.scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    if !tools.has_java() {
        eprintln!("skip: Java is unavailable");
        return;
    }
    let dir = TestDir::new("nullary-poly-intersection");
    let source = dir.join("Main.scala");
    let classes = dir.join("classes");
    fs::create_dir(&classes).unwrap();
    fs::write(
        &source,
        r#"
trait Marker
trait Tag
class Both extends Marker with Tag { override def toString = "both" }
object Main {
  def mk[A]: A with Marker = (new Both).asInstanceOf[A with Marker]
  def one[A]: A = (new Both).asInstanceOf[A]
  def lst[A]: List[A] = Nil
  def num[A]: A = 42.asInstanceOf[A]
  def take(t: Tag with Marker): String = "took " + t
  def main(args: Array[String]): Unit = {
    val x: Tag with Marker = mk
    println(x)
    val y: Tag = one
    println(y)
    val z: List[Int] = lst
    println(z)
    val w: Tag with Marker = mk[Tag]
    println(w)
    println(take(mk))
    val m: Marker = mk
    println(m)
    val n: Int = num
    println(n + 1)
    val o: Any = one
    println(o)
    val p: Tag = if (args.length > 5) one else new Both
    println(p)
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
        "both\nboth\nList()\nboth\ntook both\nboth\n43\nboth\nboth\n"
    );
}
