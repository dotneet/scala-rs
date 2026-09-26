//! A view on an operator's receiver is also searched in the argument types'
//! implicit scope, as nsc's view to `?{def op(x: ? >: A): ?}` is: `2 / y`
//! with `y: BigDecimal` goes through `BigDecimal.int2bigDecimal`, and `1 + y`
//! does too even though `any2stringadd` offers a `+` that takes a `String`.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val y = BigDecimal(4)
    val x = BigInt(3)
    println(2 / y)
    println(2 * x)
    println(1 + y)
    println(3L - x)
    println(2.5 * y)
    println((1 + y) / (2 - x.toInt) + (7 % x).toInt)
    println(1 + "s" + y)
  }
}
"#;

#[test]
fn operator_views_come_from_the_argument_companion() {
    let tools = toolchain();
    let (Some(library), Some(scalac)) = (tools.scala_library(), tools.scalac()) else {
        eprintln!("skip: scala-library or scalac is unavailable");
        return;
    };
    if !tools.has_java() {
        eprintln!("skip: Java is unavailable");
        return;
    }
    let dir = TestDir::new("arg-companion-views");
    let source = dir.join("Main.scala");
    fs::write(&source, SOURCE).unwrap();

    let reference = dir.join("scalac");
    fs::create_dir(&reference).unwrap();
    let status = Command::new(scalac)
        .arg("-d")
        .arg(&reference)
        .arg(&source)
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac failed");
    let expected = RunCommand::new("Main")
        .classpath(format!("{}:{}", reference.display(), library.display()))
        .run();
    expected.assert_success("run the scalac build");

    let ours = dir.join("scala-rs");
    fs::create_dir(&ours).unwrap();
    CompileCommand::new(&source, &ours)
        .scala_library(library)
        .run()
        .assert_success("compile with scala-rs");
    let actual = RunCommand::new("Main")
        .classpath(format!("{}:{}", ours.display(), library.display()))
        .run();
    actual.assert_success("run the scala-rs build");
    assert_eq!(actual.stdout_string(), expected.stdout_string());
}
