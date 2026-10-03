//! An object nested in a class (or a trait) is one instance per enclosing
//! instance:
//!
//! - a lambda in one of its methods that calls another of its methods
//!   captures the object, as it would a class instance. It captured nothing,
//!   as for a top-level object, and the call went to the lambda's first
//!   captured value instead (`VerifyError`);
//! - its accessor creates it under the enclosing instance's monitor and, when
//!   the constructor throws, releases the monitor and rethrows, as nsc's
//!   does. It kept the monitor, and HotSpot replaced the real exception with
//!   `IllegalMonitorStateException`.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
trait Base { def tailRecM[A](a: A)(f: A => Int): (Int => Int) => Int }
class Outer {
  implicit object M extends Base {
    private def loop[A](f: A => Int, inner: Int => Int)(a: A): Int = inner(f(a))
    override def tailRecM[A](a: A)(f: A => Int): (Int => Int) => Int =
      inner => Option(a).map(loop(f, inner)).get
  }
  object Failing { val x: Int = throw new RuntimeException("boom") }
}
trait T { object N { val y: Int = sys.error("trait boom") } }
object Top {
  object M2 {
    private def loop[A](f: A => Int, inner: Int => Int)(a: A): Int = inner(f(a))
    def tailRecM[A](a: A)(f: A => Int): (Int => Int) => Int = inner => Option(a).map(loop(f, inner)).get
  }
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Outer().M.tailRecM("abc")(_.length)(_ * 2))
    println(Top.M2.tailRecM("abcd")(_.length)(_ * 3))
    try new Outer().Failing.x catch { case e: Throwable => println(e.getClass.getName + ": " + e.getMessage) }
    try (new T {}).N.y catch { case e: Throwable => println(e.getClass.getName + ": " + e.getMessage) }
  }
}
"#;

#[test]
fn member_objects_behave_as_instances() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("member-object-lambdas");
    let source = dir.join("E.scala");
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
        "6\n12\njava.lang.RuntimeException: boom\njava.lang.RuntimeException: trait boom\n"
    );
    assert_eq!(run(&ours), expected);
}
