//! A partial function literal whose result is a value class returns the box:
//! `PartialFunction[A, B]`'s methods return `B`'s erasure, `Object`. It had no
//! expected result type (it is no SAM: `isDefinedAt` is abstract as well), so
//! `collectFirst { case ... => Gid(id) }` handed back an `Integer`, and reading
//! the `Option[Gid]` cast it to `Gid` (`ClassCastException`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
case class Gid(value: Int) extends AnyVal
case class Item(kind: String, id: Int)
object Main {
  def pick(xs: List[Item]): (Gid, Gid) = {
    val g = xs.collectFirst { case Item("grade", id) => Gid(id) }.getOrElse(throw new RuntimeException("none"))
    val c = xs.collectFirst { case Item("chapter", id) => Gid(id) }.getOrElse(throw new RuntimeException("none"))
    (g, c)
  }
  def main(args: Array[String]): Unit = {
    val xs = List(Item("grade", 1), Item("chapter", 2))
    println(pick(xs))
    println(xs.collect { case Item(_, id) if id > 1 => Gid(id) })
    val pf: PartialFunction[Item, Gid] = { case Item(k, id) if k.nonEmpty => Gid(id * 10) }
    println(xs.map(pf).map(_.value))
  }
}
"#;

#[test]
fn partial_function_literal_returns_the_box() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("partial-function-value-class");
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
    assert_eq!(expected, "(Gid(1),Gid(2))\nList(Gid(2))\nList(10, 20)\n");
    assert_eq!(run(&ours), expected);
}
