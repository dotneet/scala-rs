//! A generic value class instantiated at a primitive (`Ref[Int, P]`, as
//! refined's `Refined` is used) handed to a parameter declared at it. Its slot
//! is the box of the primitive (`Integer`), and the argument, the `Ref` a
//! lambda parameter holds, is unwrapped to it. Compared with the bare class
//! (`Ref` without its arguments erases to `Object`), the slot looked like a
//! generic one and the `Ref` itself went in (`ClassCastException`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
final class Ref[T, P] private (val value: T) extends AnyVal { override def toString = "Ref(" + value + ")" }
object Ref { def make[T, P](t: T): Either[String, Ref[T, P]] = if (t != null) Right(new Ref[T, P](t)) else Left("null") }
trait Pos
case class Pid(private val v: Ref[Int, Pos]) { def get: Int = v.value }
object Pid { def apply(value: Int): Pid = Ref.make[Int, Pos](value).fold(m => throw new Exception(m), Pid(_)) }
object Main { def main(args: Array[String]): Unit = println((Pid(3).get, Pid(4))) }
"#;

#[test]
fn primitive_generic_value_class_argument_is_unwrapped() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("generic-value-class-primitive-arg");
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
    assert_eq!(expected, "(3,Pid(Ref(4)))\n");
    assert_eq!(run(&ours), expected);
}
