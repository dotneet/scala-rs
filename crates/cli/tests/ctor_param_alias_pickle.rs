//! A constructor parameter written with a type alias keeps the alias in the
//! pickle, as nsc keeps it (and as ours did for a method's parameters).
//! Reflection-driven injection keys a binding by the declared type: Airframe
//! bound `Save[Option]` found no binding for the expanded `Int => Option[Int]`
//! it read off our constructor (`MISSING_DEPENDENCY`).

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package p
object Repo { type Save[F[_]] = Int => F[Int]; def save: Save[Option] = i => Some(i) }
class Svc(val s: Repo.Save[Option], t: Repo.Save[List]) {
  def run(x: Repo.Save[Option]): Repo.Save[Option] = x
  def go = s(1) ++ t(2)
}
"#;

const MAIN: &str = r#"
import scala.reflect.runtime.universe._
object Main {
  def main(args: Array[String]): Unit = {
    val t = typeOf[p.Svc]
    println(t.decl(termNames.CONSTRUCTOR).asMethod.paramLists.flatten.map(_.typeSignature))
    println(t.decl(TermName("run")).typeSignature)
    println(new p.Svc(p.Repo.save, i => List(i, i)).go)
  }
}
"#;

#[test]
fn constructor_parameter_keeps_its_type_alias() {
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
    let dir = TestDir::new("ctor-param-alias-pickle");
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
        "List(p.Repo.Save[Option], p.Repo.Save[List])
(x: p.Repo.Save[Option]): p.Repo.Save[Option]
List(1, 2, 2)
"
    );
    assert_eq!(run(&ours), expected);
}
