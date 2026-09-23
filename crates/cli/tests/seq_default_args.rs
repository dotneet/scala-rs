//! Collection members whose trailing parameter has a default argument in
//! scala-library, where the prelude declares the member itself.
//!
//! The prelude has no default arguments: a default is modelled as a second,
//! shorter overload that codegen completes. A member declared with only one
//! of its arities hides the pickled declaration, so the other spelling was
//! `no matching overload`:
//!
//! * `ArrayOps.indexOf(elem, from = 0)` and `lastIndexOf(elem, end =
//!   length - 1)` had only their two-argument forms;
//! * `List.indexWhere(p, from)` and `startsWith(that, offset = 0)` had only
//!   their one-argument forms.
//!
//! The program is compiled by both compilers and each output is checked
//! against scalac 2.13.16's, recorded below.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::fs;
use std::process::Command;

const SOURCE: &str = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val a = Array(1, 2, 1)
    println(a.indexOf(1))
    println(a.indexOf(1, 1))
    println(a.lastIndexOf(1))
    println(a.lastIndexOf(1, 1))
    val s = Array("x", "y", "x")
    println(s.indexOf("x"))
    println(s.lastIndexOf("x"))
    println(Array('a', 'b', 'a').lastIndexOf('a'))
    println(Array(1.5, 2.5).lastIndexOf(2.5))
    println(Array[Int]().lastIndexOf(1))
    val l = List(1, 2, 1)
    println(l.indexWhere(_ == 1))
    println(l.indexWhere(_ == 1, 1))
    println(l.startsWith(Seq(1)))
    println(l.startsWith(Seq(2), 1))
    println(l.startsWith(Seq(1), 1))
  }
}
"#;

const EXPECTED: &str = "0\n2\n2\n0\n0\n2\n2\n1\n-1\n0\n2\ntrue\ntrue\nfalse\n";

#[test]
fn defaulted_members_take_both_arities_like_scalac() {
    let tools = toolchain();
    let Some(library) = tools.scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    if !tools.has_java() {
        eprintln!("skip: Java is unavailable");
        return;
    }
    let dir = TestDir::new("seq-default-args");
    let source = dir.join("Main.scala");
    fs::write(&source, SOURCE).unwrap();

    let ours = dir.join("ours");
    fs::create_dir(&ours).unwrap();
    CompileCommand::new(&source, &ours)
        .scala_library(library)
        .run()
        .assert_success("compile");
    let run = RunCommand::new("Main")
        .classpath(format!("{}:{}", ours.display(), library.display()))
        .run();
    run.assert_success("run");
    assert_eq!(run.stdout_string(), EXPECTED, "scala-rs output");

    let Some(scalac) = tools.scalac() else {
        eprintln!("skip scalac half: scalac is unavailable");
        return;
    };
    let theirs = dir.join("scalac");
    fs::create_dir(&theirs).unwrap();
    let out = Command::new(scalac)
        .arg("-d")
        .arg(&theirs)
        .arg(&source)
        .output()
        .expect("run scalac");
    assert!(
        out.status.success(),
        "scalac: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = RunCommand::new("Main")
        .classpath(format!("{}:{}", theirs.display(), library.display()))
        .run();
    run.assert_success("run scalac's classes");
    assert_eq!(run.stdout_string(), EXPECTED, "scalac output");
}
