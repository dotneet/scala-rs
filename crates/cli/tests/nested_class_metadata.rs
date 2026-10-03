//! A member class of a top-level object is nested in the object's mirror
//! class, as nsc writes it: its `InnerClasses` entry names the mirror (or the
//! companion class) as the outer class, that class lists it, and the class
//! file carries only the `Scala` marker, its symbols being in the top-level
//! pickle. Scala reflection finds such a class among the mirror's
//! `getDeclaredClasses`; with the module class as the outer one it found
//! nothing (a dependency injector then took the parameter for an `Object`),
//! and a companion class that did not list the member made the JVM reject
//! the pair ("disagree on InnerClasses attribute").

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
object Holder {
  class Unit1 { def v = 1 }
  case class Req(a: Int)
  object Deep { class D(val v: Int) }
}
sealed abstract class Cause
object Cause { case class Bad(code: String) extends Cause }
"#;

const USE: &str = r#"
object Use {
  def main(args: Array[String]): Unit = {
    import scala.reflect.runtime.universe._
    val m = runtimeMirror(Use.this.getClass.getClassLoader)
    for (c <- Seq(classOf[lib.Holder.Unit1], classOf[lib.Holder.Req], classOf[lib.Holder.Deep.D], classOf[lib.Cause.Bad]))
      println(c.getName + " in " + c.getDeclaringClass.getName + " " + c.getSimpleName)
    println(classOf[lib.Holder.type].getDeclaredClasses.map(_.getName).sorted.mkString(","))
    println(Class.forName("lib.Holder").getDeclaredClasses.map(_.getName).sorted.mkString(","))
    println(classOf[lib.Cause].getDeclaredClasses.map(_.getName).sorted.mkString(","))
    println(m.runtimeClass(typeOf[lib.Holder.Unit1]).getName)
    println(m.runtimeClass(typeOf[lib.Cause.Bad]).getName)
    println(new lib.Holder.Unit1().v + lib.Holder.Req(2).a + new lib.Holder.Deep.D(3).v + lib.Cause.Bad("x").code)
  }
}
"#;

#[test]
fn object_member_classes_are_nested_in_the_mirror() {
    let t = toolchain();
    let (Some(library), Some(reflect), Some(java), Some(scalac)) =
        (t.scala_library(), t.scala_reflect(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, scala-reflect, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("nested-class-metadata");
    let lib = dir.join("Lib.scala");
    let use_ = dir.join("Use.scala");
    fs::write(&lib, LIB).unwrap();
    fs::write(&use_, USE).unwrap();
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    let client = dir.join("client");
    let client_of_theirs = dir.join("client-of-theirs");
    for d in [&ours, &theirs, &client, &client_of_theirs] {
        fs::create_dir_all(d).unwrap();
    }
    let base = format!("{}:{}", library.display(), reflect.display());
    CompileCommand::new(&lib, &ours)
        .classpath(library)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let scalac_run = |src: &std::path::Path, cp: &str, out: &std::path::Path| {
        let output = Command::new(scalac)
            .arg("-cp")
            .arg(cp)
            .arg("-d")
            .arg(out)
            .arg(src)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    scalac_run(&lib, &base, &theirs);
    // scalac reads our class files as it reads its own.
    scalac_run(&use_, &format!("{}:{base}", ours.display()), &client);
    scalac_run(
        &use_,
        &format!("{}:{base}", theirs.display()),
        &client_of_theirs,
    );
    let run = |client: &std::path::Path, lib: &std::path::Path| {
        let out = Command::new(java)
            .arg("-cp")
            .arg(format!("{}:{}:{base}", client.display(), lib.display()))
            .arg("Use")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let expected = run(&client_of_theirs, &theirs);
    assert!(
        expected.contains("lib.Holder$Unit1 in lib.Holder Unit1"),
        "{expected}"
    );
    assert_eq!(run(&client, &ours), expected);
}
