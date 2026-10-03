//! A materialized `TypeTag` of a class nested in a class is the projection
//! `Outer#Inner` when it is named from outside `Outer`, and `Outer.this.Inner`
//! from inside, as nsc builds them. It was always `Outer.this.Inner`: `=:=`
//! still held, but anything keyed by the type's string (a dependency
//! injector's bindings) no longer matched scalac's.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
package p
import scala.reflect.runtime.universe._
trait Backend { class DatabaseDef; type Database = DatabaseDef }
trait Profile { type B <: Backend }
class Outer {
  class Inner
  def inside: Type = typeTag[Inner].tpe
}
object Main {
  def main(args: Array[String]): Unit = {
    println(typeTag[Outer#Inner].tpe)
    println(new Outer().inside)
    println(typeTag[Backend#Database].tpe)
    println(typeTag[Backend#Database].tpe =:= typeOf[Backend#DatabaseDef])
  }
}
"#;

#[test]
fn type_tag_of_a_nested_class_keeps_its_prefix() {
    let t = toolchain();
    let (Some(library), Some(reflect), Some(java), Some(scalac)) =
        (t.scala_library(), t.scala_reflect(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, scala-reflect, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("typetag-projection");
    let source = dir.join("Main.scala");
    fs::write(&source, SOURCE).unwrap();
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&ours).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    let cp = format!("{}:{}", library.display(), reflect.display());
    CompileCommand::new(&source, &ours)
        .classpath(&cp)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let output = Command::new(scalac)
        .arg("-cp")
        .arg(&cp)
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
            .arg("-cp")
            .arg(format!("{}:{cp}", classes.display()))
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
    assert!(expected.starts_with("p.Outer#Inner\n"), "{expected}");
    assert_eq!(run(&ours), expected);
}
