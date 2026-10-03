//! An implicit view imported from an object with a very wide parent graph.
//! The search for the object the view is inherited from gave up after 256
//! classes, so the view lost the import's qualifier and was called on
//! `this` (`ClassCastException: ... cannot be cast to ...Holder$Ops`), as it
//! happens with `import cats.implicits._` and `either.leftWiden`.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

fn library_source() -> String {
    let mut s = String::from("package lib\n");
    let n = 300;
    for i in 0..n {
        s.push_str(&format!("trait T{i}\n"));
    }
    s.push_str("final class Rich(val n: Int) { def twice: Int = n * 2 }\n");
    s.push_str("object Holder { trait Ops { implicit def toRich(n: Int): Rich = new Rich(n) } }\n");
    // The view's trait sits below the wide level, as in cats (`implicits`
    // extends `AllSyntax`, which reaches `Bifunctor.ToBifunctorOps`).
    s.push_str("trait Mid extends Holder.Ops\n");
    s.push_str("object Imp extends Mid");
    for i in 0..n {
        s.push_str(&format!(" with T{i}"));
    }
    s.push('\n');
    s
}

const USE: &str = r#"
import lib.Imp._
class Svc(v: Int) { def run: Int = v.twice }
object Main { def main(args: Array[String]): Unit = println(new Svc(21).run) }
"#;

#[test]
fn view_imported_from_a_wide_object_keeps_its_qualifier() {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("wide-import-view");
    let lib_src = dir.join("Lib.scala");
    let use_src = dir.join("Use.scala");
    fs::write(&lib_src, library_source()).unwrap();
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
    let cp = std::env::join_paths([use_out.as_path(), lib_out.as_path(), library]).unwrap();
    let run = RunCommand::new("Main").classpath(cp).run();
    run.assert_success("client execution");
    assert_eq!(run.stdout_string(), "42\n");
}
