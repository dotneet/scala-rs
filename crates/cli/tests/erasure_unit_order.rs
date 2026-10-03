//! Erasure rewrites the symbol table's types in place, one unit after
//! another, and code generation runs once every unit is erased. Each unit
//! must still be read against the source types, and keep what erasure
//! decided for it:
//!
//! - A `final val` of a library object is inlined in every unit, and reading
//!   it never initializes the object: a unit after another one called the
//!   accessor instead, which ran the object's initializer.
//! - A lambda's value-class parameter arrives boxed: in any unit but the last
//!   the boxing was undone before code generation, and the lambda unboxed a
//!   `Count` it was handed (`VerifyError`).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
object K { println("K initialized"); final val N = "n" }
final class Count(val value: Int) extends AnyVal
"#;

const FIRST: &str = "package a\nobject First\n";

const LAST: &str = "package c\nobject Last\n";

const SECOND: &str = r#"
package b
import lib._
object Main {
  def show(c: Count): String = s"Count(${c.value})"
  def main(args: Array[String]): Unit = {
    println(K.N)
    val e: Either[String, Count] = Right(new Count(41))
    println(e.map(c => c.value + 1))
    println(e.map(c => show(c)))
    println(for { a <- e; b <- Right(new Count(1)) } yield show(a) + show(b))
    println(List(new Count(1), new Count(2)).map(_.value).sum)
  }
}
"#;

#[test]
fn every_unit_is_erased_against_the_source_types() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("erasure-unit-order");
    let lib = dir.join("Lib.scala");
    let first = dir.join("First.scala");
    let second = dir.join("Second.scala");
    let last = dir.join("Last.scala");
    fs::write(&last, LAST).unwrap();
    fs::write(&lib, LIB).unwrap();
    fs::write(&first, FIRST).unwrap();
    fs::write(&second, SECOND).unwrap();
    let lib_out = dir.join("lib");
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    for d in [&lib_out, &ours, &theirs] {
        fs::create_dir_all(d).unwrap();
    }
    let scalac_run = |args: &[&std::path::Path], cp: &str, out: &std::path::Path| {
        let output = Command::new(scalac)
            .arg("-cp")
            .arg(cp)
            .arg("-d")
            .arg(out)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "scalac failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    scalac_run(&[&lib], &library.display().to_string(), &lib_out);
    let cp = format!("{}:{}", lib_out.display(), library.display());
    scalac_run(&[&first, &second, &last], &cp, &theirs);
    CompileCommand::new(&first, &ours)
        .arg(&second)
        .arg(&last)
        .classpath(&cp)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let run = |classes: &std::path::Path| {
        let out = Command::new(java)
            .args(["-Xverify:all", "-cp"])
            .arg(format!("{}:{cp}", classes.display()))
            .arg("b.Main")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "run failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let expected = run(&theirs);
    assert_eq!(
        expected,
        "n\nRight(42)\nRight(Count(41))\nRight(Count(41)Count(1))\n3\n"
    );
    assert_eq!(run(&ours), expected);
}
