//! A type tag for a type written with an alias of a static object keeps the
//! alias, as nsc's does: `typeTag[Ctl.Control]` is `TypeRef(Ctl.type,
//! Control, Nil)`, not the function type it stands for. That holds for a type
//! argument written at the call, inside an applied type, for an alias of the
//! file being compiled, for a parameterized alias (`Ctl.Fetch[Future]`), and
//! for a tag a macro expansion asks for with the macro's type argument, also
//! when that argument is inferred from a function literal's parameter. Expanded, the tag printed `Int => String`, and a
//! library that tells bindings apart by the names in a tag (Airframe) found
//! no binding for one made with the alias.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, path::Path, process::Command};

const LIB: &str = r#"
package d
object Ctl {
  type Route = Int => String
  type Control = Route
  type Fetch[F[_]] = (Int, String) => F[List[Int]]
  type Pair[A] = (A, A)
}
"#;

const MACROS: &str = r#"
package m
import scala.language.experimental.macros
import scala.reflect.macros.blackbox
object Tags {
  def nameOf[A]: String = macro Impl.nameOf[A]
  def paramOf[A](f: A => Any): String = macro Impl.paramOf[A]
}
object Impl {
  def nameOf[A: c.WeakTypeTag](c: blackbox.Context): c.Tree = {
    import c.universe._
    val t = weakTypeOf[A]
    q"_root_.scala.reflect.runtime.universe.typeTag[$t].tpe.toString"
  }
  def paramOf[A: c.WeakTypeTag](c: blackbox.Context)(f: c.Tree): c.Tree = nameOf[A](c)
}
"#;

const USE: &str = r#"
import scala.reflect.runtime.universe._
import d.Ctl.Control
object Local { type Counts = Map[String, Int] }
object Main {
  def show[A](implicit t: TypeTag[A]): String = t.tpe.toString
  def main(a: Array[String]): Unit = {
    println(show[Control])
    println(typeTag[List[Control]].tpe)
    println(show[Local.Counts])
    println(typeOf[Control] =:= typeOf[Int => String])
    println(m.Tags.nameOf[Control])
    println(m.Tags.nameOf[Option[Local.Counts]])
    println(show[d.Ctl.Fetch[scala.concurrent.Future]])
    println(show[List[d.Ctl.Pair[d.Ctl.Fetch[Option]]]])
    println(m.Tags.nameOf[d.Ctl.Pair[Int]])
    println(m.Tags.paramOf((c: Control) => c(1)))
    println(m.Tags.paramOf { (c: List[d.Ctl.Pair[Int]]) => c.size })
  }
}
"#;

const EXPECTED: &str = "d.Ctl.Control\nList[d.Ctl.Control]\nLocal.Counts\ntrue\n\
    d.Ctl.Control\nOption[Local.Counts]\nd.Ctl.Fetch[scala.concurrent.Future]\n\
    List[d.Ctl.Pair[d.Ctl.Fetch[Option]]]\nd.Ctl.Pair[Int]\nd.Ctl.Control\nList[d.Ctl.Pair[Int]]\n";

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
fn type_tags_keep_static_aliases() {
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
    let dir = TestDir::new("type-tag-alias");
    let write = |name: &str, text: &str| {
        let p = dir.join(name);
        fs::write(&p, text).unwrap();
        p
    };
    let (lib_src, macro_src, use_src) = (
        write("Lib.scala", LIB),
        write("Macros.scala", MACROS),
        write("Use.scala", USE),
    );
    let out = |name: &str| {
        let p = dir.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    };
    let (lib, macros, ours, theirs) = (out("lib"), out("macros"), out("ours"), out("theirs"));
    // The macro implementation runs inside the compiler, so the library
    // halves are scalac's; the client is compiled by both.
    scalac(scalac_bin, library.as_os_str(), &lib, &lib_src);
    scalac(scalac_bin, library.as_os_str(), &macros, &macro_src);
    let deps = std::env::join_paths([lib.as_path(), macros.as_path()]).unwrap();
    scalac(scalac_bin, &deps, &theirs, &use_src);
    let cp = std::env::join_paths([lib.as_path(), macros.as_path(), reflect]).unwrap();
    CompileCommand::new(&use_src, &ours)
        .classpath(&cp)
        .scala_library(library)
        .run()
        .assert_success("scala-rs client");
    for client in [&theirs, &ours] {
        let cp = std::env::join_paths([
            client.as_path(),
            lib.as_path(),
            macros.as_path(),
            library,
            reflect,
        ])
        .unwrap();
        let run = RunCommand::new("Main").classpath(cp).run();
        run.assert_success(&format!("run {}", client.display()));
        assert_eq!(run.stdout_string(), EXPECTED, "{}", client.display());
    }
}
