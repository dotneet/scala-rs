//! `f()` on a parameterless `def f: R` applies the result: the empty
//! argument list goes to `R`'s `apply()`, as nsc types it. scalikejdbc's
//! `sql"...".update()` is `update.apply()(session)`, and dropping the
//! argument list compiled a statement that never ran. A method overriding
//! one declared with `()` (`override def toString = ...`) is still called
//! with it directly.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
class U { def apply()(implicit s: String): Int = { println("applied " + s); 1 } }
class V { def apply(): Int = { println("applied plain"); 2 } }
class Q {
  def update: U = new U
  def plain: V = new V
  val field: V = new V
  def thunk: () => Int = () => 42
  override def toString = "Q!"
  override def hashCode = 7
}
object Main {
  implicit val s: String = "x"
  def main(args: Array[String]): Unit = {
    val q = new Q
    q.update()
    println(q.plain())
    q.field()
    println(q.thunk())
    println(q.toString() + " " + q.hashCode())
  }
}
"#;

#[test]
fn empty_arguments_to_a_parameterless_def_apply_its_result() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("empty-apply-nullary");
    let source = dir.join("U.scala");
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
        "applied x\napplied plain\n2\napplied plain\n42\nQ! 7\n"
    );
    assert_eq!(run(&ours), expected);
}
