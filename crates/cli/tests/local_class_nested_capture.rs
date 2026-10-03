//! A class or object nested in a local class reads the enclosing method's
//! locals through the local class's capture fields. It captured nothing
//! itself, and the read took the method for a class
//! (`NoClassDefFoundError: resolve`) -- or, with the local class's own
//! `$outer` in the way, `NoSuchFieldError: $outer`.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
import scala.collection.mutable
import scala.reflect.ClassTag
trait LT { type Line }
case class G(base: Int) {
  def resolve(lt: LT, xs: Set[Any])(implicit a: ClassTag[lt.Line]): Option[Int] = {
    type Line = lt.Line
    Option.when(xs.nonEmpty) {
      class U(ls: Set[Line]) {
        sealed trait Node
        class Root private (val key: Line, val elements: mutable.UnrolledBuffer[Line]) extends Node
        object Root { def singleton(key: Line): Root = new Root(key, Seq(key).to(mutable.UnrolledBuffer)) }
        private val roots: mutable.Set[Root] = mutable.Set.from(ls.view.map(Root.singleton))
        def count = roots.size + base
      }
      new U(xs.asInstanceOf[Set[Line]]).count
    }
  }
  def counter(n: Int): Int = {
    var hits = 0
    class V {
      object W {
        def bump(): Unit = hits += n
        class X { def twice(): Unit = { bump(); bump() } }
        def x = new X
      }
    }
    val v = new V
    v.W.x.twice()
    v.W.bump()
    hits
  }
}
object Main {
  def main(args: Array[String]): Unit = {
    val lt = new LT { type Line = Integer }
    println(G(1).resolve(lt, Set(1, 2, 3)))
    println(G(0).counter(5))
  }
}
"#;

#[test]
fn members_of_a_local_class_read_its_captures() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("local-class-nested-capture");
    let source = dir.join("K.scala");
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
    assert_eq!(expected, "Some(4)\n15\n");
    assert_eq!(run(&ours), expected);
}
