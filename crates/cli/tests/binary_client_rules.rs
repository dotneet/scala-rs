//! Rules a client compiled against a scalac-built library on a *directory*
//! classpath must apply exactly as nsc does, whatever order the library's
//! class files happen to be read in.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, path::Path, process::Command};

/// Compile `lib` with scalac into `<dir>/lib` and `client` with scala-rs
/// against that directory; `None` when the toolchain is unavailable.
fn compile_against_lib_dir(
    label: &str,
    lib: &str,
    client: &str,
) -> Option<(TestDir, CompileOutcome)> {
    let tools = toolchain();
    let (Some(scalac), Some(library)) = (tools.scalac(), tools.scala_library()) else {
        eprintln!("skip: scalac or scala-library is unavailable");
        return None;
    };
    let dir = TestDir::new(label);
    let lib_src = dir.join("Lib.scala");
    let lib_out = dir.join("lib");
    let client_src = dir.join("Use.scala");
    let out = dir.join("out");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&out).unwrap();
    fs::write(&lib_src, lib).unwrap();
    fs::write(&client_src, client).unwrap();
    let status = Command::new(scalac)
        .arg("-d")
        .arg(&lib_out)
        .arg(&lib_src)
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac failed on the library");
    let outcome = CompileCommand::new(&client_src, &out)
        .classpath(&lib_out)
        .scala_library(library)
        .run();
    Some((dir, outcome))
}

fn run_main(dir: &Path) -> Option<String> {
    let tools = toolchain();
    let (Some(_), Some(library)) = (tools.java(), tools.scala_library()) else {
        eprintln!("skip: java is unavailable");
        return None;
    };
    let sep = if cfg!(windows) { ";" } else { ":" };
    let cp = format!(
        "{}{sep}{}{sep}{}",
        dir.join("out").display(),
        dir.join("lib").display(),
        library.display()
    );
    let run = RunCommand::new("Main").classpath(cp).run();
    run.assert_success("run Main");
    Some(run.stdout_string())
}

/// nsc: `found: a.Cell  required: b.Cell`. A nested class the directory
/// classpath only stubbed (its class file unread) was judged "not inner",
/// so `b.Cell` was the bare class and every `a.Cell` conformed to it -- in
/// a jar, or once anything completed the stub first, the same program was
/// rejected.
#[test]
fn directory_classpath_inner_class_keeps_its_prefix() {
    let lib = r#"
package lib
class A3 { class Cell(v: Int); def mk(v: Int): Cell = new Cell(v) }
class A5 { class Cell; def mk: Cell = new Cell }
object O { class Stat(v: Int) }
class O { def x = 1 }
"#;
    let bad = r#"
import lib._
object Main {
  def main(args: Array[String]): Unit = {
    val a3 = new A3; val b3 = new A3
    val x3: b3.Cell = a3.mk(1)
    val a5 = new A5; val b5 = new A5
    val x5: b5.Cell = a5.mk
  }
}
"#;
    let Some((_dir, outcome)) = compile_against_lib_dir("inner-prefix-bad", lib, bad) else {
        return;
    };
    assert!(!outcome.success(), "a3.Cell must not conform to b3.Cell");
    let diags = outcome.diagnostics();
    assert!(
        diags.contains("found: a3.Cell  required: b3.Cell")
            && diags.contains("found: a5.Cell  required: b5.Cell"),
        "{diags}"
    );

    let good = r#"
import lib._
object Main {
  def main(args: Array[String]): Unit = {
    val a3 = new A3
    val x3: a3.Cell = a3.mk(1)
    val y3: A3#Cell = x3
    val s: O.Stat = new O.Stat(1)
    println(y3 != null && s != null)
  }
}
"#;
    let Some((dir, outcome)) = compile_against_lib_dir("inner-prefix-good", lib, good) else {
        return;
    };
    outcome.assert_success("same-prefix inner class");
    if let Some(stdout) = run_main(&dir) {
        assert_eq!(stdout.trim(), "true");
    }
}
