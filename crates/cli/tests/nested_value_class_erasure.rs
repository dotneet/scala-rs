//! A generic value class instantiated at another value class erases, in a
//! signature, to the inner class's box, as nsc's `erasedValueClassArg` does:
//! `show(w: Wrap[Code])` is `show(Llib/Code;)`, `make: Wrap[Code]` returns
//! `Llib/Code;`. We erased it on down to `Code`'s underlying `String`, so our
//! own library disagreed with scalac's, and a client could not call a scalac
//! library's method at all: the pickled member matched no class-file
//! descriptor and the descriptor's `(String)String` was left in its place
//! (`no matching overload for (String)String with arguments (Wrap[Code])`).

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
case class Code(value: String) extends AnyVal
final class Wrap[A](val a: A) extends AnyVal { def get: A = a }
object Api {
  def show(w: Wrap[Code]): String = w.get.value
  def make(s: String): Wrap[Code] = new Wrap(Code(s))
  def count(w: Wrap[Int]): Int = w.get + 1
  val held: Wrap[Code] = make("h")
  def all(ws: List[Wrap[Code]]): String = ws.map(_.get.value).mkString(",")
}
class Holder(val w: Wrap[Code]) { def twice: String = w.get.value * 2 }
"#;

const USE: &str = r#"
import lib._
object Main {
  def main(args: Array[String]): Unit = {
    val w: Wrap[Code] = Api.make("d")
    println(w.get)
    println(Api.show(w) + " " + Api.show(new Wrap(Code("e"))) + " " + Api.count(new Wrap(2)))
    println(Api.held.get + " " + Api.all(List(w, Api.held)) + " " + new Holder(w).twice)
    val names = Set("show", "make", "count", "held", "all")
    println(Api.getClass.getMethods.filter(m => names(m.getName)).map(_.toString).sorted.mkString("\n"))
    println(classOf[Holder].getConstructors.map(_.toString).mkString)
  }
}
"#;

fn scalac(
    scalac: &std::path::Path,
    cp: &std::path::Path,
    out: &std::path::Path,
    src: &std::path::Path,
) {
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
fn generic_value_class_over_a_value_class_erases_to_its_box() {
    let tools = toolchain();
    let (Some(scalac_bin), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("nested-value-class-erasure");
    let lib_src = dir.join("Lib.scala");
    let use_src = dir.join("Use.scala");
    fs::write(&lib_src, LIB).unwrap();
    fs::write(&use_src, USE).unwrap();
    let out = |name: &str| {
        let p = dir.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    };
    let (lib_ours, lib_theirs) = (out("lib-ours"), out("lib-theirs"));
    CompileCommand::new(&lib_src, &lib_ours)
        .scala_library(library)
        .run()
        .assert_success("scala-rs library");
    scalac(scalac_bin, library, &lib_theirs, &lib_src);
    let mut expected: Option<String> = None;
    // Both clients against both libraries; the first run (scalac against
    // scalac) is the reference, and its reflection lines are the
    // descriptors every library has to have.
    for (lib, ours) in [
        (&lib_theirs, false),
        (&lib_theirs, true),
        (&lib_ours, false),
        (&lib_ours, true),
    ] {
        let client = out(&format!(
            "use-{}-{}",
            if ours { "ours" } else { "theirs" },
            lib.file_name().unwrap().to_string_lossy()
        ));
        if ours {
            CompileCommand::new(&use_src, &client)
                .classpath(lib)
                .scala_library(library)
                .run()
                .assert_success("scala-rs client");
        } else {
            scalac(scalac_bin, lib, &client, &use_src);
        }
        let cp = std::env::join_paths([client.as_path(), lib.as_path(), library]).unwrap();
        let run = RunCommand::new("Main").classpath(cp).run();
        run.assert_success(&format!("client {}", client.display()));
        let got = run.stdout_string();
        match &expected {
            None => {
                assert!(
                    got.contains("show(lib.Code)") && got.contains("lib.Code lib.Api$.make("),
                    "scalac's own descriptors:\n{got}"
                );
                expected = Some(got);
            }
            Some(e) => assert_eq!(&got, e, "{}", client.display()),
        }
    }
}
