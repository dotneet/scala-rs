//! A parameter declared with an alias of a static object without type
//! parameters (`c: Ctl.Control`, `List[Ctl.Control]`, an alias of the file
//! being compiled) keeps the alias in the pickled signature, as nsc's does,
//! and an alias of a package object is seen through the object's stable path
//! (`dd.Id`, not `dd.package.Id`).
//! Reflection over the class reads it: Airframe builds a constructor's
//! dependencies by the names their types carry, and an expanded
//! `Int => String` matched no binding made with the alias. A client
//! scalac compiles against these classes still reads them.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, path::Path, process::Command};

const LIB: &str = r#"
package d {
  object Ctl {
    type Route = Int => String
    type Control = Route
  }
}
package object dd { type Id[A] = A }
"#;

const SVC: &str = r#"
package p
import scala.reflect.runtime.universe._
import d.Ctl.Control
object Local { type Counts = Map[String, Int] }
trait Box[F[_]]
class Svc(val c: Control, counts: Local.Counts, xs: List[Control], f: d.Ctl.Route, box: Box[dd.Id]) {
  def run(n: Int): String = c(n) + counts.size + xs.size + f(n)
}
object Main {
  def main(a: Array[String]): Unit = {
    val ctor = typeOf[Svc].typeSymbol.asClass.primaryConstructor.asMethod
    ctor.paramLists.flatten.foreach(p => println(p.name.toString + ": " + p.typeSignature))
    println(new Svc(_.toString, Map("a" -> 1), Nil, _ => "!", null).run(3))
  }
}
"#;

const CLIENT: &str = r#"
object Client {
  def main(args: Array[String]): Unit = {
    val c: d.Ctl.Control = i => "c" + i
    println(new p.Svc(c, Map.empty, List(c), c, null).run(1))
  }
}
"#;

const EXPECTED: &str = "c: d.Ctl.Control\ncounts: p.Local.Counts\nxs: List[d.Ctl.Control]\n\
    f: d.Ctl.Route\nbox: p.Box[dd.Id]\n310!\n";

fn scalac(scalac: &Path, cp: &std::ffi::OsStr, out: &Path, src: &Path) {
    CompileOutcome::from_output(
        Command::new(scalac)
            .arg("-cp")
            .arg(cp)
            .arg("-d")
            .arg(out)
            .arg(src)
            .output()
            .unwrap(),
    )
    .assert_success("scalac");
}

#[test]
fn parameter_aliases_are_pickled_as_written() {
    let tools = toolchain();
    let (Some(scalac_bin), Some(library), Some(reflect), Some(_)) = (
        tools.scalac(),
        tools.scala_library(),
        tools.scala_reflect(),
        tools.java(),
    ) else {
        eprintln!("skip: scalac, scala-library, scala-reflect or Java is unavailable");
        return;
    };
    let dir = TestDir::new("pickled-param-alias");
    let write = |name: &str, text: &str| {
        let p = dir.join(name);
        fs::write(&p, text).unwrap();
        p
    };
    let (lib_src, svc_src, client_src) = (
        write("Lib.scala", LIB),
        write("Svc.scala", SVC),
        write("Client.scala", CLIENT),
    );
    let out = |name: &str| {
        let p = dir.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    };
    let (lib, ours, theirs, client) = (out("lib"), out("ours"), out("theirs"), out("client"));
    scalac(scalac_bin, library.as_os_str(), &lib, &lib_src);
    scalac(scalac_bin, lib.as_os_str(), &theirs, &svc_src);
    let cp = std::env::join_paths([lib.as_path(), reflect]).unwrap();
    CompileCommand::new(&svc_src, &ours)
        .classpath(&cp)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    for classes in [&theirs, &ours] {
        let cp =
            std::env::join_paths([classes.as_path(), lib.as_path(), library, reflect]).unwrap();
        let run = RunCommand::new("p.Main").classpath(cp).run();
        run.assert_success(&format!("run {}", classes.display()));
        assert_eq!(run.stdout_string(), EXPECTED, "{}", classes.display());
    }
    let deps = std::env::join_paths([ours.as_path(), lib.as_path()]).unwrap();
    scalac(scalac_bin, &deps, &client, &client_src);
    let cp =
        std::env::join_paths([client.as_path(), ours.as_path(), lib.as_path(), library]).unwrap();
    let run = RunCommand::new("Client").classpath(cp).run();
    run.assert_success("scalac client");
    assert_eq!(run.stdout_string(), "c101c1\n");
}
