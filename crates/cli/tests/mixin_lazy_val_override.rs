//! A class mixing in two overrides of the same `lazy val`, one from a trait
//! of the class path and one from a trait compiled in the same run, has one
//! field for it and runs the override that comes first in the
//! linearization. Each kind used to be deduplicated on its own, and the
//! class got two fields of that name (`ClassFormatError: Duplicate field
//! name`).

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
trait Base[I] { lazy val shown: Option[String] = key.map(_.toString); val key: Option[I] }
trait Bin[+T] extends Base[String] {
  override lazy val key: Option[String] = { println("bin"); Some("b" + name) }
  val name: String
  val hint: T
}
"#;

const USE: &str = r#"
trait Src extends lib.Base[String] {
  override lazy val key: Option[String] = { println("src"); Some("s" + name) }
  val name: String
}
case class SrcWins(name: String) extends lib.Bin[Src] with Src { override lazy val hint: Src = this }
class BinWins(val name: String) extends Src with lib.Bin[Int] { val hint = 1 }
object Main {
  def main(args: Array[String]): Unit = {
    val a = SrcWins("x")
    println(a.key + " " + a.key + " " + a.shown + " " + a.hint.name)
    val b = new BinWins("y")
    println(b.key + " " + b.shown)
  }
}
"#;

#[test]
fn source_and_binary_lazy_val_overrides_share_one_field() {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("mixin-lazy-val-override");
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
    assert_eq!(
        outputs[0],
        "src\nSome(sx) Some(sx) Some(sx) x\nbin\nSome(by) Some(by)\n"
    );
    assert_eq!(outputs[1], outputs[0]);
}
