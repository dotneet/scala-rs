//! Annotations on types reach the pickle only where nsc puts them there.
//! Each one used to be pickled as a class of package `scala`, whatever its
//! real owner: `scala.annotation.nowarn` became a `scala.nowarn` that does
//! not exist, and runtime reflection over the type threw `AssertionError:
//! unsafe symbol nowarn (child of package scala)`. nsc pickles static
//! annotations only, so `@nowarn` and `@unchecked` are not there at all.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package p
import scala.annotation.nowarn
class Svc[F[_]](private val dep: List[F[Int]] @nowarn, other: String @unchecked)
"#;

const MAIN: &str = r#"
import scala.reflect.runtime.universe._
object Main {
  def main(args: Array[String]): Unit = {
    val t = typeOf[p.Svc[Option]]
    val ctor = t.decl(termNames.CONSTRUCTOR).asMethod
    println(ctor.paramLists.flatten.map(p => p.name.toString + ": " + p.typeSignature))
  }
}
"#;

#[test]
fn runtime_reflection_reads_annotated_parameter_types() {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(reflect), Some(_)) = (
        tools.scalac(),
        tools.scala_library(),
        tools.scala_reflect(),
        tools.java(),
    ) else {
        eprintln!("skip: scalac, scala-library, scala-reflect or Java is unavailable");
        return;
    };
    let dir = TestDir::new("pickled-type-annotations");
    let lib_src = dir.join("Lib.scala");
    let main_src = dir.join("Main.scala");
    fs::write(&lib_src, LIB).unwrap();
    fs::write(&main_src, MAIN).unwrap();
    let out = |name: &str| {
        let p = dir.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    };
    let (ours, theirs, client) = (out("ours"), out("theirs"), out("client"));
    CompileCommand::new(&lib_src, &ours)
        .scala_library(library)
        .run()
        .assert_success("scala-rs library");
    for (cp, dst, src) in [
        (library, &theirs, &lib_src),
        (theirs.as_path(), &client, &main_src),
    ] {
        let cp = std::env::join_paths([cp, library, reflect]).unwrap();
        CompileOutcome::from_output(
            Command::new(scalac)
                .arg("-cp")
                .arg(cp)
                .arg("-d")
                .arg(dst)
                .arg(src)
                .output()
                .unwrap(),
        )
        .assert_success("scalac");
    }
    let run = |lib: &std::path::Path| {
        let cp = std::env::join_paths([client.as_path(), lib, library, reflect]).unwrap();
        let run = RunCommand::new("Main").classpath(cp).run();
        run.assert_success(&format!("reflection over {}", lib.display()));
        run.stdout_string()
    };
    let expected = run(&theirs);
    assert_eq!(
        expected,
        "List(dep: List[F[Int]], other: String)
"
    );
    assert_eq!(run(&ours), expected);
}
