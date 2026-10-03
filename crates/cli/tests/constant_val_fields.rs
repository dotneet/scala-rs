//! A template `final val` of a constant type has no field: nsc emits only
//! the accessor, which answers the constant. Reflection-based libraries see
//! the class's fields, so an extra one showed up as a property of its own
//! (and reordered the ones Jackson lists).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
package p
class C(val k: Int) {
  final private val A = true
  final val B = 20
  final val S = "s"
  private val N = 3
  def f = if (A) B + k else N
}
object O { final val X = 1L; final private val Y = 'c'; def g = Y }
object Main {
  def main(args: Array[String]): Unit = {
    val c = new C(1)
    println(c.f + " " + c.B + " " + c.S + " " + O.X + " " + O.g)
    println(classOf[C].getDeclaredFields.map(_.getName).sorted.mkString(","))
    println(O.getClass.getDeclaredFields.map(_.getName).sorted.mkString(","))
  }
}
"#;

#[test]
fn constant_vals_have_no_field() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("constant-val-fields");
    let source = dir.join("C.scala");
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
            .arg("p.Main")
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
    assert_eq!(expected, "21 20 s 1 c\nN,k\nMODULE$\n");
    assert_eq!(run(&ours), expected);
}
