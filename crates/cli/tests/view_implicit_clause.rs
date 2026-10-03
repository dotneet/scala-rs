//! An implicit view with an implicit parameter clause whose result is a
//! function, applied to an argument: pekko-http's
//! `parameters("a".as[Int]) { a => ... }` is
//! `Directive.addDirectiveApply(directive)(converter).apply(route)`. Both
//! clauses are one call of the view. The inserted application carried the
//! view's final function type, so the backend took the implicit argument
//! for the argument of a function value and emitted the view's call
//! without it (`VerifyError: Bad type on operand stack`).
//!
//! The view comes from source and from a library scalac compiled.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
trait AC[L] { type In; def apply(f: In): Int }
object AC {
  implicit def ac1: AC[Tuple1[Int]] { type In = Int => Int } =
    new AC[Tuple1[Int]] { type In = Int => Int; def apply(f: In) = f(41) }
}
class D[L]
object D {
  implicit def addApply[L](d: D[L])(implicit hac: AC[L]): hac.In => Int = f => hac(f)
  implicit def plain(d: D[Int])(implicit n: Int): (Int => Int) => Int = f => f(n)
}
"#;

const USE: &str = r#"
object Main {
  import lib._
  implicit val seed: Int = 9
  def params(i: Int): D[Tuple1[Int]] = new D
  def ints(i: Int): D[Int] = new D
  def main(a: Array[String]): Unit = {
    println(params(1) { (x: Int) => x + 1 })
    println(ints(1) { (x: Int) => x * 2 })
  }
}
"#;

#[test]
fn view_with_an_implicit_clause_returning_a_function_is_one_call() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("view-implicit-clause");
    let lib = dir.join("Lib.scala");
    let use_ = dir.join("Use.scala");
    fs::write(&lib, LIB).unwrap();
    fs::write(&use_, USE).unwrap();
    let run = |cp: String| {
        let out = Command::new(java)
            .args(["-Xverify:all", "-cp", &cp, "Main"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "run failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let both = dir.join("both");
    fs::create_dir_all(&both).unwrap();
    CompileCommand::new(&lib, &both)
        .arg(&use_)
        .classpath(library)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile of both files");
    assert_eq!(
        run(format!("{}:{}", both.display(), library.display())),
        "42\n18\n"
    );

    let lib_out = dir.join("lib");
    let use_out = dir.join("use");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&use_out).unwrap();
    let output = Command::new(scalac)
        .arg("-d")
        .arg(&lib_out)
        .arg(&lib)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "scalac failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    CompileCommand::new(&use_, &use_out)
        .classpath(format!("{}:{}", lib_out.display(), library.display()))
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile against scalac's library");
    assert_eq!(
        run(format!(
            "{}:{}:{}",
            use_out.display(),
            lib_out.display(),
            library.display()
        )),
        "42\n18\n"
    );
}
