//! A value class reaching a reference slot stays boxed. Two places handed
//! out the bare underlying value instead: the bridge of a constructor `val`
//! whose parent declares the member generically (`def values: F[A]`
//! implemented by `val values: Box[A]`), and an argument to a repeated
//! parameter (`f(a: Any*)`). Either way the caller's cast to the value class
//! failed with `ClassCastException`.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
final class Box[A](val v: Vector[A]) extends AnyVal { override def toString = "Box" + v }
final class N(val i: Int) extends AnyVal { override def toString = "N" + i }
trait Base[A, F[_]] extends Any {
  protected def values: F[A]
  def get: F[A] = values
}
case class Holder(override protected val values: Box[Int]) extends Base[Int, Box]
trait Base2[A, F[_]] { def values: F[A]; val other: F[A] }
class H2(val values: Box[Int], val other: Box[Int]) extends Base2[Int, Box]
trait Gen[T] { def g: T }
class H3(val g: N) extends Gen[N]
object Main {
  def many(a: Any*): Seq[Any] = a
  def ns(a: N*): Int = a.map(_.i).sum
  def main(args: Array[String]): Unit = {
    val b: Base[Int, Box] = Holder(new Box(Vector(1)))
    val h2: Base2[Int, Box] = new H2(new Box(Vector(2)), new Box(Vector(3)))
    val g3: Gen[N] = new H3(new N(5))
    val n = new N(6)
    val all: List[Any] = List(b.get, h2.values, h2.other, g3.g, many(n).head, many(n, g3.g), List[Any](g3.g).head)
    println(all.mkString(" "))
    println(ns(n, g3.g) + " " + ns(Seq(n): _*))
  }
}
"#;

#[test]
fn value_classes_in_reference_slots_stay_boxed() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("value-class-ref-slots");
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
        "BoxVector(1) BoxVector(2) BoxVector(3) N5 N6 ArraySeq(N6, N5) N5\n11 6\n"
    );
    assert_eq!(run(&ours), expected);
}
