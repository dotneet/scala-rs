//! A branch whose condition is a constant is not emitted: nsc folds
//! `0 % 2 == 0` to `true` while typing and its backend drops the arm that
//! cannot run. A table of such entries fits one method for scalac, and has
//! to for scala-rs too.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::{fs, process::Command};

fn source() -> String {
    let entries: Vec<String> = (0..1500)
        .map(|i| format!("({i}, if ({i} % 2 == 0) Some({i}) else None)"))
        .collect();
    format!(
        "object Table {{\n  val rows = List({})\n}}\n\
         object Main {{\n  def main(args: Array[String]): Unit = {{\n    \
         println(Table.rows.size)\n    println(Table.rows.take(3))\n    \
         println(if (Table.rows.size > 2 * 3) \"big\" else \"small\")\n  }}\n}}\n",
        entries.join(", ")
    )
}

#[test]
fn a_table_of_constant_conditions_fits_one_method_like_scalac() {
    let tools = toolchain();
    let (Some(library), Some(scalac)) = (tools.scala_library(), tools.scalac()) else {
        eprintln!("skip: scala-library or scalac is unavailable");
        return;
    };
    if !tools.has_java() {
        eprintln!("skip: Java is unavailable");
        return;
    }
    let dir = TestDir::new("constant-condition-branches");
    let source_path = dir.join("Table.scala");
    fs::write(&source_path, source()).unwrap();

    let reference = dir.join("scalac");
    fs::create_dir(&reference).unwrap();
    let status = Command::new(scalac)
        .env("JAVA_OPTS", "-Xmx1g -Xss8m")
        .arg("-d")
        .arg(&reference)
        .arg(&source_path)
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac failed");
    let expected = RunCommand::new("Main")
        .classpath(format!("{}:{}", reference.display(), library.display()))
        .run();
    expected.assert_success("run the scalac build");

    let ours = dir.join("scala-rs");
    fs::create_dir(&ours).unwrap();
    CompileCommand::new(&source_path, &ours)
        .scala_library(library)
        .run()
        .assert_success("compile with scala-rs");
    let actual = RunCommand::new("Main")
        .classpath(format!("{}:{}", ours.display(), library.display()))
        .run();
    actual.assert_success("run the scala-rs build");
    assert_eq!(actual.stdout_string(), expected.stdout_string());
}
