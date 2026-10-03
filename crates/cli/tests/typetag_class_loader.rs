//! A materialized `TypeTag` finds the application's classes when
//! scala-reflect is loaded by a parent class loader, as under a build tool's
//! test runner: nsc builds the tag against
//! `runtimeMirror(this.getClass.getClassLoader)`, and the root mirror used
//! before saw only scala-reflect's own loader (`ScalaReflectionException:
//! class p.Foo in JavaMirror ... not found`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
package p
import scala.reflect.runtime.universe._
class Foo
object Main {
  def main(args: Array[String]): Unit = println(typeTag[Foo].tpe)
}
"#;

/// Runs `p.Main` with the Scala jars in a parent loader of the classes'.
const LAUNCHER: &str = r#"
import java.io.File;
import java.net.URL;
import java.net.URLClassLoader;
public class Layered {
  public static void main(String[] args) throws Exception {
    URL[] libs = new URL[args.length - 1];
    for (int i = 1; i < args.length; i++) libs[i - 1] = new File(args[i]).toURI().toURL();
    ClassLoader parent = new URLClassLoader(libs, ClassLoader.getPlatformClassLoader());
    ClassLoader app = new URLClassLoader(new URL[] { new File(args[0]).toURI().toURL() }, parent);
    Class.forName("p.Main", true, app).getMethod("main", String[].class)
      .invoke(null, (Object) new String[0]);
  }
}
"#;

#[test]
fn type_tag_uses_the_call_sites_class_loader() {
    let t = toolchain();
    let (Some(library), Some(reflect), Some(java), Some(javac), Some(scalac)) = (
        t.scala_library(),
        t.scala_reflect(),
        t.java(),
        t.javac(),
        t.scalac(),
    ) else {
        eprintln!("skip: scala-library, scala-reflect, java, javac or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("typetag-class-loader");
    let source = dir.join("Main.scala");
    let launcher = dir.join("Layered.java");
    fs::write(&source, SOURCE).unwrap();
    fs::write(&launcher, LAUNCHER).unwrap();
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    let tools = dir.join("tools");
    for d in [&ours, &theirs, &tools] {
        fs::create_dir_all(d).unwrap();
    }
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
    let output = Command::new(javac)
        .arg("-d")
        .arg(&tools)
        .arg(&launcher)
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
            .arg(&tools)
            .arg("Layered")
            .arg(classes)
            .arg(library)
            .arg(reflect)
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
    assert_eq!(expected, "p.Foo\n");
    assert_eq!(run(&ours), expected);
}
