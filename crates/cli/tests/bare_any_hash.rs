//! A bare `##` is `this.##`. Emitted as a plain member call it named
//! `Object.$hash$hash`, which no class declares, and failed with
//! `NoSuchMethodError` the first time it ran.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
class A {
  override def hashCode = 42
  def h1 = ##
  def h2 = this.##
  def hex = s"${##.toHexString}"
  def inLambda = List(1).map(_ => ##).head
  class Inner { override def hashCode = 5; def h = ## }
}
trait T { override def hashCode = 9; def th = ## }
class B extends T
object O { override def hashCode = 3; def h = ## }
final class W(val u: Int) extends AnyVal { def h = ## }
object Main {
  def main(args: Array[String]): Unit = {
    val a = new A
    println(List(a.h1, a.h2, a.hex, a.inLambda, new a.Inner().h).mkString(" "))
    println(List(new B().th, O.h, new W(11).h).mkString(" "))
  }
}
"#;

#[test]
fn bare_any_hash_hashes_this() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("bare-any-hash");
    let source = dir.join("A.scala");
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
    assert_eq!(expected, "42 42 2a 42 5\n9 3 11\n");
    assert_eq!(run(&ours), expected);
}
