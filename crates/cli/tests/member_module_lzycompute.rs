//! A member object's accessor initializes the `name$module` field through a
//! private `name$lzycompute$1`, as nsc emits it, rather than inline. Code that
//! inspects a class tells the field apart from mutable state by that method: a
//! singleton-safety check reported every member object of a shared class as a
//! mutable instance field.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
class Svc { case class Req(a: Int); object Helper { def x = 1 }; def run = Req(1).a + Helper.x }
trait Mix { object Inner { def y = 2 } }
class Mixed extends Mix
object Main {
  def names(c: Class[_]): String = c.getDeclaredMethods.map(_.getName).sorted.mkString(",")
  def main(a: Array[String]): Unit = {
    println(names(classOf[Svc]))
    println(names(classOf[Mixed]))
    println((new Svc().run, new Mixed().Inner.y))
  }
}
"#;

#[test]
fn member_object_initializer_is_a_separate_method() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("member-module-lzycompute");
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
        "Helper,Helper$lzycompute$1,Req,Req$lzycompute$1,run\nInner,Inner$lzycompute$1\n(2,2)\n"
    );
    assert_eq!(run(&ours), expected);
}
