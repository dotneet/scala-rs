//! A class written in a template's super constructor call is created before
//! the template's instance exists. nsc makes it an inner class of the next
//! enclosing instance and passes it the template's constructor parameters and
//! block locals as captures. It used to be an inner class of the template
//! itself, built with a `null` outer (`NullPointerException` on any enclosing
//! member), and it read constructor parameters off that missing instance.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
trait Mk { def make: String }
abstract class Base(val mk: Mk)
trait Tup[C] { def make(t: (Int, String)): C }
abstract class TupBase[C](val mk: Tup[C])
class Top(p: Int) extends Base(new Mk { def make = "top" + p })
object TopO extends Base(new Mk { def make = "topo" })
class Repo(val tag: String) {
  def helper = "h" + tag
  class C(q: String) extends Base(new Mk { def make = q + helper + new Mk { def make = "in" + q }.make })
  class D extends Base({ val k = tag * 2; new Mk { def make = k } })
  object O extends Base(new Mk { def make = helper })
  case class G(a: Int, b: String)
  object G {
    implicit object Shape extends TupBase(new Tup[G] { def make(t: (Int, String)): G = (apply _).tupled(t) })
  }
  def run = List(new C("c").mk.make, new D().mk.make, O.mk.make, G.Shape.mk.make((1, tag)))
}
object Main {
  def main(args: Array[String]): Unit = println(new Repo("x").run ++ List(new Top(3).mk.make, TopO.mk.make))
}
"#;

#[test]
fn super_call_classes_reach_the_enclosing_instance() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("super-call-class-outer");
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
    assert_eq!(expected, "List(chxinc, xx, hx, G(1,x), top3, topo)\n");
    assert_eq!(run(&ours), expected);
}
