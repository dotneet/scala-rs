//! A local or anonymous class that reads nothing outside itself but holds a
//! class that does (`class Outer1 { val inner = new Enc { ... prefix ... } }`,
//! `prefix` a member of the enclosing `Data`). The inner class reaches `Data`
//! through the middle one's `$outer`, and the middle one had that field elided
//! (`NoSuchFieldError: $outer`). circe's derived encoders are this shape.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
trait Enc { def enc(i: Int): String }
case class Data(k: Int) {
  implicit val prefix: String = "p" + k
  def run: String = {
    class Outer1 { val inner: Enc = new Enc { def enc(i: Int) = prefix + i } }
    val anon = new { val inner: Enc = new Enc { def enc(i: Int) = prefix + (i * 2) } }
    new Outer1().inner.enc(1) + " " + anon.inner.enc(2)
  }
  lazy val viaLazy: Enc = new Enc { val deep: Enc = new Enc { def enc(i: Int) = prefix + "!" + i }; def enc(i: Int) = deep.enc(i) }
}
object Main { def main(args: Array[String]): Unit = { val d = Data(7); println(d.run + " " + d.viaLazy.enc(3)) } }
"#;

#[test]
fn middle_class_keeps_outer_for_a_nested_reader() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("nested-outer-chain");
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
    assert_eq!(expected, "p71 p74 p7!3\n");
    assert_eq!(run(&ours), expected);
}
