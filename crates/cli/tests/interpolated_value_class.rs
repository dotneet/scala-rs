//! A value class interpolated into a string (`s"id $id"`, `f`, `raw`) is
//! formatted as the instance, `Sid(404)`. The arguments are `StringContext`'s
//! `Any*`, but erasure gave them no expected type and they were appended as
//! the underlying value, `404`.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
case class Sid(value: Int) extends AnyVal
case class Nm(value: String) extends AnyVal
object Main {
  def a(id: Sid): String = s"Session $id"
  def b(id: Sid): Option[String] = Some(1).map(_ => s"Session $id")
  def d(id: Sid): Option[String] = Some(1).map { case 1 => s"Session $id ${id.value}"; case _ => "" }
  def e(id: Sid): String = "Session " + id
  def f(id: Sid, n: Nm): String = f"$id%s/${n}%s/${id.value}%d" + raw"\t$n"
  def main(args: Array[String]): Unit = { val i = Sid(404); println(List(a(i), b(i), d(i), e(i), f(i, Nm("x")))) }
}
"#;

#[test]
fn interpolated_value_class_is_the_instance() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("interpolated-value-class");
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
    assert_eq!(expected, "List(Session Sid(404), Some(Session Sid(404)), Some(Session Sid(404) 404), Session Sid(404), Sid(404)/Nm(x)/404\\tNm(x))\n");
    assert_eq!(run(&ours), expected);
}
