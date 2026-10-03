//! A default may name only the parameters of earlier lists. Another parameter
//! of its own list means what the name means outside the method: in
//! `def copy(mode: String = mode)` the field. It was resolved to the
//! parameter, and the default getter read it off a class named after the
//! method (`NoClassDefFoundError: copy`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
case class Def private (private val code: String, mode: String, toc: Option[Int] = None) {
  def copy(mode: String = mode, toc: Option[Int] = toc): Def = Def(mode, mode, toc)
}
object Def { def apply(mode: String): Def = Def(mode, mode) }
class Box(val size: Int, val label: String) {
  def resized(size: Int = size * 2, label: String = label + "!"): Box = new Box(size, label)
  def curried(size: Int)(label: String = label + size): String = label
  override def toString = s"Box($size,$label)"
}
object Main {
  def main(args: Array[String]): Unit = {
    val d = Def("a")
    println((d.copy(toc = Some(1)), d.copy(mode = "b")))
    val b = new Box(3, "x")
    println((b.resized(), b.resized(label = "y"), b.curried(7)()))
  }
}
"#;

#[test]
fn defaults_do_not_see_their_own_parameter_list() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("default-param-scope");
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
        "(Def(a,a,Some(1)),Def(b,b,None))\n(Box(6,x!),Box(6,y),x7)\n"
    );
    assert_eq!(run(&ours), expected);
}
