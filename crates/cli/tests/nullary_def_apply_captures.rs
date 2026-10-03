//! `keep(x)` for a parameterless local `def keep: Int => Boolean` that captures
//! a local. Lambda lifting gives the call to `keep` the captures, and the call
//! it became still named `keep`, so the application of its result got them as
//! well: the function received the captured `Option` for its `Int` argument
//! (`ClassCastException`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
object Main {
  def run(opt: Option[Int], xs: List[Int]): List[Int] = {
    def keep: Int => Boolean = x => opt.forall(_ == x)
    for { x <- xs if keep(x) } yield x
  }
  def run2(opt: Option[Int], xs: List[Int]): List[Int] = {
    def keep: Int => Boolean = x => opt.forall(_ == x)
    xs.filter(x => keep(x))
  }
  def run3(opt: Option[Int], x: Int): Boolean = {
    def keep: Int => Boolean = y => opt.forall(_ == y)
    keep(x)
  }
  def main(args: Array[String]): Unit = println((run(Some(2), List(1, 2, 3)), run2(None, List(1, 2)), run3(Some(1), 1)))
}
"#;

#[test]
fn applying_a_lifted_nullary_def_passes_its_captures_once() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("nullary-def-apply-captures");
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
    assert_eq!(expected, "(List(2),List(1, 2),true)\n");
    assert_eq!(run(&ours), expected);
}
