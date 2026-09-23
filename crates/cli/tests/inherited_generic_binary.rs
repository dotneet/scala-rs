//! Members a binary class inherits from a generic parent keep the parent's
//! type arguments, read from a class directory and from a jar alike.

use crate::support::{toolchain, TestDir};
use std::{fs, path::Path, process::Command};

fn check(output: &std::process::Output, what: &str) -> String {
    assert!(
        output.status.success(),
        "{what} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compile `lib` with scalac, then `client` with scalac and with scala-rs
/// against the library as a directory and as a jar, and require all three
/// programs to print the same thing. `None` when the toolchain is missing.
fn same_as_scalac(label: &str, lib: &str, client: &str) -> Option<String> {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(java)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return None;
    };
    let dir = TestDir::new(label);
    let lib_src = dir.join("Lib.scala");
    let client_src = dir.join("Use.scala");
    fs::write(&lib_src, lib).unwrap();
    fs::write(&client_src, client).unwrap();
    let lib_dir = dir.join("lib");
    fs::create_dir(&lib_dir).unwrap();
    check(
        &Command::new(scalac)
            .arg("-d")
            .arg(&lib_dir)
            .arg(&lib_src)
            .output()
            .unwrap(),
        "scalac Lib.scala",
    );
    let lib_jar = dir.join("lib.jar");
    check(
        &Command::new("jar")
            .arg("cf")
            .arg(&lib_jar)
            .arg("-C")
            .arg(&lib_dir)
            .arg(".")
            .output()
            .unwrap(),
        "jar",
    );
    let run = |classes: &Path, lib: &Path| {
        let classpath = format!(
            "{}:{}:{}",
            classes.display(),
            lib.display(),
            library.display()
        );
        check(
            &Command::new(java)
                .args(["-cp", &classpath, "Main"])
                .output()
                .unwrap(),
            "run",
        )
    };
    let expected_classes = dir.join("scalac-out");
    fs::create_dir(&expected_classes).unwrap();
    check(
        &Command::new(scalac)
            .arg("-cp")
            .arg(&lib_dir)
            .arg("-d")
            .arg(&expected_classes)
            .arg(&client_src)
            .output()
            .unwrap(),
        "scalac Use.scala",
    );
    let expected = run(&expected_classes, &lib_dir);
    for (name, lib) in [("dir", &lib_dir), ("jar", &lib_jar)] {
        let classes = dir.join(format!("out-{name}"));
        fs::create_dir(&classes).unwrap();
        check(
            &Command::new(env!("CARGO_BIN_EXE_scala-rs"))
                .arg("compile")
                .arg(&client_src)
                .arg("-cp")
                .arg(lib)
                .arg("--scala-library")
                .arg(library)
                .arg("-d")
                .arg(&classes)
                .output()
                .unwrap(),
            &format!("scala-rs compile ({name} library)"),
        );
        assert_eq!(run(&classes, lib), expected, "{name} library");
    }
    Some(expected)
}

#[test]
fn two_parameter_trait_method_through_concrete_subclass_keeps_its_type() {
    let lib = r#"
package lib
trait Ops[A] {
  def first(x: A, y: A): A = x
  def one(x: A): A = x
  def pick(x: A, y: Int): A = x
  def pick(x: String, y: Long): String = x
}
class StrBox extends Ops[String]
class IntBox extends Ops[Int]
abstract class Base[A <: CharSequence] { def both(x: A, y: A): A = y }
class SB extends Base[String]
"#;
    let client = r#"
import lib._
object Main {
  def main(args: Array[String]): Unit = {
    val s: String = new StrBox().first("a", "bc")
    println(s.length)
    println(new StrBox().pick("xy", 1).length)
    println(new StrBox().pick("xyz", 1L).length)
    println(new SB().both("a", "bcde").length)
    val i: Int = new IntBox().one(3)
    println(i + 1)
  }
}
"#;
    if let Some(out) = same_as_scalac("inherited-generic-two-params", lib, client) {
        assert_eq!(out, "1\n2\n3\n4\n4\n");
    }
}

/// `class IntBox extends Ops[Int]` sees `first(x: Int, y: Int): Int`, but the
/// class file only has `first(Object, Object)Object`: the arguments have to be
/// boxed to the declaration's erasure and the result unboxed. scala-rs used to
/// decline the member and fall back to the erased forwarder ("found: Any
/// required: Int").
#[test]
fn primitive_instantiated_generic_member_boxes_to_the_declared_descriptor() {
    let lib = r#"
package lib
trait Ops[A] {
  def first(x: A, y: A): A = x
  def second(x: A, y: A): A = y
  def both(x: A, y: A): (A, A) = (x, y)
}
class IntBox extends Ops[Int]
class DoubleBox extends Ops[Double]
final class Meters(val v: Int) extends AnyVal { override def toString = s"${v}m" }
class MeterBox extends Ops[Meters]
"#;
    let client = r#"
import lib._
object Main {
  def main(args: Array[String]): Unit = {
    val i: Int = new IntBox().first(3, 4)
    println(i + 1)
    println(new IntBox().second(3, 4) * 10)
    println(new IntBox().both(5, 6))
    val d: Double = new DoubleBox().second(1.5, 2.5)
    println(d)
    val m: Meters = new MeterBox().first(new Meters(7), new Meters(8))
    println(m.v + 1)
  }
}
"#;
    if let Some(out) = same_as_scalac("inherited-generic-primitive", lib, client) {
        assert_eq!(out, "4\n40\n(5,6)\n2.5\n8\n");
    }
}

#[test]
fn inner_class_result_through_applied_subclass_reads_the_outer_arguments() {
    let lib = r#"
package lib
class Outer[T](val t: T) { class Inner { def get: T = t } }
class Sub[U](u: U) extends Outer[List[U]](List(u))
class Sub2 extends Sub[Int](41)
trait Holder[K] { def key: K; class Slot { def k: K = key } }
class StrHolder extends Holder[String] { def key = "kk" }
object Api {
  def mk2[U](s: Sub[U]): Sub[U]#Inner = new s.Inner
  def mk3(s: Sub[String]): Sub[String]#Inner = new s.Inner
  def mk4(s: Sub2): Sub2#Inner = new s.Inner
  def slot(h: StrHolder): StrHolder#Slot = new h.Slot
}
"#;
    let client = r#"
import lib._
object Main {
  def main(args: Array[String]): Unit = {
    println(Api.mk2(new Sub("ab")).get.head.length)
    println(Api.mk3(new Sub("abc")).get.head.length)
    println(Api.mk4(new Sub2).get.head + 1)
    println(Api.slot(new StrHolder).k.length)
  }
}
"#;
    if let Some(out) = same_as_scalac("inherited-generic-inner-prefix", lib, client) {
        assert_eq!(out, "2\n3\n42\n2\n");
    }
}
