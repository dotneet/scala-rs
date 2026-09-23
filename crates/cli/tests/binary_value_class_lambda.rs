//! A binary value class's underlying accessor read on a boxed receiver.
//!
//! Inside a lambda the parameter arrives as the box (`List(m).map(_.v)`), so
//! the receiver is unboxed through the `v()` getter -- and that already is the
//! answer. The accessor has no `$extension` static, and calling
//! `v$extension` was a `NoSuchMethodError` at run time.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
class Meter(val v: Double) extends AnyVal { def twice: Double = v * 2 }
class Name(val s: String) extends AnyVal { def shout: String = s.toUpperCase }
"#;

const USE: &str = r#"
import lib._
object Main {
  def f(m: Meter): Double = m.v
  def main(args: Array[String]): Unit = {
    println(f(new Meter(2)))
    println(List(new Meter(5)).map(x => x.v))
    println(List(new Meter(6)).map(_.v))
    println(List(new Meter(7)).map(_.twice))
    println(List(new Name("ab")).map(_.s))
    println(List(new Name("cd")).map(n => n.shout))
  }
}
"#;

const EXPECTED: &str = "2.0\nList(5.0)\nList(6.0)\nList(14.0)\nList(ab)\nList(CD)\n";

#[test]
fn binary_value_class_underlying_accessor_in_lambda() {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("binary-value-class-lambda");
    let lib_src = dir.join("Lib.scala");
    let use_src = dir.join("Use.scala");
    fs::write(&lib_src, LIB).unwrap();
    fs::write(&use_src, USE).unwrap();
    let lib_out = dir.join("lib");
    let use_out = dir.join("use");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&use_out).unwrap();

    CompileOutcome::from_output(
        Command::new(scalac)
            .arg("-d")
            .arg(&lib_out)
            .arg(&lib_src)
            .output()
            .unwrap(),
    )
    .assert_success("scalac library");
    CompileCommand::new(&use_src, &use_out)
        .classpath(&lib_out)
        .scala_library(library)
        .run()
        .assert_success("scala-rs client");

    let classpath = format!(
        "{}:{}:{}",
        use_out.display(),
        lib_out.display(),
        library.display()
    );
    let run = RunCommand::new("Main").classpath(&classpath).run();
    run.assert_success("run client");
    assert_eq!(run.stdout_string(), EXPECTED);
}
