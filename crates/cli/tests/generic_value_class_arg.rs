//! The `$extension` receiver of a value class that wraps a type parameter is
//! passed as that parameter's erasure. Instantiated at another value class
//! it may hold the box (`ArrowAssoc(x: A): A` returns the `Code` instance),
//! and unboxing it to the instantiation's erasure cast the `Code` box to
//! `String` (`ClassCastException`). The same holds for a call returning such
//! a class and for the local an eta-expansion keeps it in (`Gid(3).->`).

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
case class Code(value: String) extends AnyVal
final class Wrap[A](val a: A) extends AnyVal { def get: A = a }
object Api {
  def pair(c: Code): (Code, Int) = c -> 1
  def boxed[A](a: A): Wrap[A] = new Wrap(a)
}
"#;

const USE: &str = r#"
import lib._
import scala.util.chaining._
case class Gid(value: Int) extends AnyVal
case class Uid(value: java.util.UUID) extends AnyVal
object Main {
  def main(args: Array[String]): Unit = {
    println(Code("a") -> 1)
    println(List("b").map(c => Code(c) -> c.length).toMap)
    println(Api.boxed(Code("c")).get + " " + Api.pair(Code("e")))
    val u = new java.util.UUID(1, 2)
    println((Uid(u).tap(identity), Gid(3).pipe(g => g.value + 1), Code("f").tap(identity), Uid(u).pipe(identity).value))
    println((Option("x").map(Gid(3).->), Option(1).map(Code("g").->)))
  }
}
"#;

const EXPECTED: &str = "(Code(a),1)\nMap(Code(b) -> 1)\nCode(c) (Code(e),1)\n\
    (Uid(00000000-0000-0001-0000-000000000002),4,Code(f),00000000-0000-0001-0000-000000000002)\n\
    (Some((Gid(3),x)),Some((Code(g),1)))\n";

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
fn generic_value_class_over_a_value_class_stays_boxed() {
    let tools = toolchain();
    let (Some(scalac_bin), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("generic-value-class-arg");
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
    // Every pairing of the two compilers: each client links against the
    // other one's library as well as against its own.
    for (lib, ours) in [
        (&lib_ours, true),
        (&lib_ours, false),
        (&lib_theirs, true),
        (&lib_theirs, false),
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
        assert_eq!(run.stdout_string(), EXPECTED, "{}", client.display());
    }
}
