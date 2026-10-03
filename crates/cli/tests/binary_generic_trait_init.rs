//! A class-path trait with type parameters still has its `$init$` run by a
//! class that mixes it in. Its members are supplied from its pickle, which
//! does not list `$init$`, and the check for one read only those: the class
//! skipped `Base.$init$`, and every `val` of `Base` stayed `null`
//! (`NullPointerException` on the first use).

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
trait Cfg { def name: String }
trait Base[T, C <: Cfg] {
  println("Base init")
  val config: C
  val algs: Seq[String] = Seq("HS256")
  def show(t: T): String = config.name + algs.mkString + t
}
object StudentCfg extends Cfg { def name = "student" }
trait Plain { println("Plain init"); val p: Int = 7 }
trait Student extends Base[Int, StudentCfg.type] with Plain {
  println("Student init")
  val config: StudentCfg.type = StudentCfg
}
"#;

const USE: &str = r#"
trait Api { def run(): String }
class Impl extends Api with lib.Student { def run(): String = show(1) + " " + p }
object Single extends lib.Student
object Main {
  def main(args: Array[String]): Unit = {
    println(new Impl().run())
    println(Single.show(2))
    println(new lib.Student {}.algs)
  }
}
"#;

#[test]
fn generic_binary_trait_is_initialized() {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("binary-generic-trait-init");
    let lib_src = dir.join("Lib.scala");
    let use_src = dir.join("Use.scala");
    fs::write(&lib_src, LIB).unwrap();
    fs::write(&use_src, USE).unwrap();
    let lib_out = dir.join("lib");
    fs::create_dir_all(&lib_out).unwrap();
    CompileOutcome::from_output(
        Command::new(scalac)
            .arg("-d")
            .arg(&lib_out)
            .arg(&lib_src)
            .output()
            .unwrap(),
    )
    .assert_success("scalac library");
    let mut outputs = Vec::new();
    for reference in [true, false] {
        let out = dir.join(if reference { "reference" } else { "native" });
        fs::create_dir_all(&out).unwrap();
        if reference {
            CompileOutcome::from_output(
                Command::new(scalac)
                    .arg("-cp")
                    .arg(&lib_out)
                    .arg("-d")
                    .arg(&out)
                    .arg(&use_src)
                    .output()
                    .unwrap(),
            )
            .assert_success("scalac client");
        } else {
            CompileCommand::new(&use_src, &out)
                .classpath(&lib_out)
                .scala_library(library)
                .run()
                .assert_success("scala-rs client");
        }
        let cp = std::env::join_paths([out.as_path(), lib_out.as_path(), library]).unwrap();
        let run = RunCommand::new("Main").classpath(cp).run();
        run.assert_success("client execution");
        outputs.push(run.stdout_string());
    }
    assert!(
        outputs[0].contains("Base init\nPlain init\nStudent init\nstudentHS2561 7\n"),
        "{}",
        outputs[0]
    );
    assert_eq!(outputs[1], outputs[0]);
}
