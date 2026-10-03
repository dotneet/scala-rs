//! A case-class pattern on a value class from the class path calls its
//! companion's `unapply`, whose parameter erases to the underlying value
//! (`Off$.unapply(I)`). A scrutinee held as a reference -- a lambda parameter
//! in a generic slot, or an `Any` -- is the box, so the pattern tests and
//! unwraps it as nsc does. Unboxing the box itself as the underlying value
//! threw `ClassCastException` (`Off` cannot be cast to `Integer`).

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
case class Off(value: Int) extends AnyVal
case class Name(value: String) extends AnyVal
"#;

const USE: &str = r#"
import lib._
object Main {
  def drop(xs: List[Int], o: Option[Off]): List[Int] = o.foldLeft(xs) { case (q, Off(n)) => q.drop(n) }
  def len(o: Option[Name]): Int = o.fold(0) { case Name(s) => s.length }
  def any(o: Any): String = o match {
    case Off(n) => "off " + n
    case Name(s) => "name " + s
    case _ => "other"
  }
  def main(args: Array[String]): Unit = {
    println((drop(List(1, 2, 3, 4), Some(Off(2))), len(Some(Name("abc")))))
    println(List[Any](Off(1), Name("n"), 3).map(any))
  }
}
"#;

const EXPECTED: &str = "(List(3, 4),3)\nList(off 1, name n, other)\n";

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
fn binary_value_class_extractor_reads_the_box() {
    let tools = toolchain();
    let (Some(scalac_bin), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("binary-value-class-extractor");
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
