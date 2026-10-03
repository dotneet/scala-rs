//! A trait's overloads that differ in value-class parameters erasing to the
//! same JVM type (`conv(x: InA): OutA` beside `conv(x: InB): OutB`) are each
//! forwarded by an implementing class. A method mixed in from a trait was
//! taken to bridge a same-named method of the same trait whenever their
//! parameters erased alike, and the bridge replaced `conv(InB)`'s forwarder
//! with a method boxing `conv(InA)`'s result (`VerifyError: Bad return type`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
case class InA(value: Int) extends AnyVal
case class InB(value: Int) extends AnyVal
case class OutA(value: Int) extends AnyVal
case class OutB(private val v: String) { def show = v }
object OutB { def fromInt(i: Int) = OutB("b" + i) }
trait Conv {
  def conv(x: InA): OutA = OutA(x.value + 1)
  def conv(x: InB): OutB = OutB.fromInt(x.value)
}
class User extends Conv {
  def run: String = conv(InA(1)).value + " " + conv(InB(2)).show
}
object Main { def main(a: Array[String]): Unit = println(new User().run) }
"#;

#[test]
fn trait_overloads_over_value_classes_keep_their_forwarders() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("mixin-value-class-overloads");
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
    assert_eq!(expected, "2 b2\n");
    assert_eq!(run(&ours), expected);
}
