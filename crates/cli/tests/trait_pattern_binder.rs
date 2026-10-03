//! A typed pattern whose type is a trait, on a class-typed scrutinee
//! (`case e: Compat` on a `Throwable`), binds `e` at `Throwable with Compat`,
//! which erases to `Throwable` like nsc's intersection dominator. The binder was
//! stored as the trait, which disagreed with its uses as the class
//! (`VerifyError: Inconsistent stackmap frames`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
trait Compat { def code: Int }
class MyEx(val code: Int) extends RuntimeException("my") with Compat
object Main {
  def codeOf(c: Compat): Int = c.code
  def translate(t: Throwable): Throwable = t match {
    case e: Compat => e
    case e: IllegalStateException => new RuntimeException(e)
    case e => e
  }
  def describe(t: Throwable): String = t match {
    case e: Compat => e.getMessage + e.code + codeOf(e)
    case _ => "other"
  }
  def main(args: Array[String]): Unit = {
    println(List(translate(new MyEx(1)).getMessage, translate(new IllegalStateException("x")).getMessage))
    println(List(describe(new MyEx(2)), describe(new RuntimeException)))
  }
}
"#;

#[test]
fn trait_type_pattern_binds_the_class() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("trait-pattern-binder");
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
        "List(my, java.lang.IllegalStateException: x)\nList(my22, other)\n"
    );
    assert_eq!(run(&ours), expected);
}
