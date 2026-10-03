//! An implicit macro that ends in `c.abort` while the search is still
//! choosing makes only its own candidate inapplicable: nsc expands macro
//! candidates as it searches and goes on without the one that aborted. Here
//! the export macro derives `Show` for a case class only, so for `Plain` the
//! lexically imported `viaShow` cannot be built and the companion's
//! `Out.plain` is chosen -- for an implicit argument, for a view, and for a
//! view on a by-name argument. The abort was reported as the error instead,
//! and under the by-name argument the view was dropped silently.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package m

import scala.language.experimental.macros
import scala.reflect.macros.blackbox

final case class Exported[A](value: A)

trait Show[A] { def show(a: A): String }
object Show {
  implicit def imported[A](implicit e: Exported[Show[A]]): Show[A] = e.value
}

object auto {
  implicit def exportShow[A]: Exported[Show[A]] = macro AutoImpl.exportShow[A]
}

object AutoImpl {
  def exportShow[A: c.WeakTypeTag](c: blackbox.Context): c.Tree = {
    import c.universe._
    val t = weakTypeOf[A]
    if (!t.typeSymbol.isClass || !t.typeSymbol.asClass.isCaseClass)
      c.abort(c.enclosingPosition, s"cannot derive Show for $t")
    q"_root_.m.Exported(new _root_.m.Show[$t] { def show(a: $t): String = ${"derived "} + a })"
  }
}

class Plain(val n: Int)

trait Out[A] { def out(a: A): String }
object Out {
  implicit val plain: Out[Plain] = new Out[Plain] { def out(a: Plain): String = "plain " + a.n }
}

object support {
  implicit def viaShow[A](implicit s: Show[A]): Out[A] = new Out[A] { def out(a: A): String = "shown " + s.show(a) }
}

final class Resp(val text: String)
object Resp {
  implicit def from[A](a: A)(implicit o: Out[A]): Resp = new Resp(o.out(a))
}
"#;

const USE: &str = r#"
import m._
import m.auto._
import m.support._

case class Point(x: Int)

object Main {
  def render[A](a: A)(implicit o: Out[A]): String = o.out(a)
  def complete(r: => Resp): String = r.text
  def main(args: Array[String]): Unit = {
    println(render(new Plain(1)))
    println(render(Point(2)))
    println(complete(new Plain(3)))
    println(complete(Point(4)))
    val r: Resp = new Plain(5)
    println(r.text)
  }
}
"#;

const EXPECTED: &str =
    "plain 1\nshown derived Point(2)\nplain 3\nshown derived Point(4)\nplain 5\n";

#[test]
fn an_aborted_macro_implicit_falls_back_to_the_next_candidate() {
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
    let dir = TestDir::new("macro-abort-implicit-fallback");
    let lib_src = dir.join("Lib.scala");
    let use_src = dir.join("Use.scala");
    fs::write(&lib_src, LIB).unwrap();
    fs::write(&use_src, USE).unwrap();
    let out = |name: &str| {
        let p = dir.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    };
    let (lib, ours, theirs) = (out("lib"), out("ours"), out("theirs"));
    // The macro implementation runs inside the compiler, so the library is
    // scalac's; the client is compiled by both.
    let scalac_run = |cp: &std::path::Path, dest: &std::path::Path, src: &std::path::Path| {
        CompileOutcome::from_output(
            Command::new(scalac)
                .arg("-cp")
                .arg(cp)
                .arg("-d")
                .arg(dest)
                .arg(src)
                .output()
                .unwrap(),
        )
        .assert_success("scalac");
    };
    scalac_run(library, &lib, &lib_src);
    scalac_run(&lib, &theirs, &use_src);
    let cp = std::env::join_paths([lib.as_path(), reflect]).unwrap();
    CompileCommand::new(&use_src, &ours)
        .classpath(&cp)
        .scala_library(library)
        .run()
        .assert_success("scala-rs client");
    for client in [&theirs, &ours] {
        let cp = std::env::join_paths([client.as_path(), lib.as_path(), library]).unwrap();
        let run = RunCommand::new("Main").classpath(cp).run();
        run.assert_success(&format!("run {}", client.display()));
        assert_eq!(run.stdout_string(), EXPECTED, "{}", client.display());
    }
}
