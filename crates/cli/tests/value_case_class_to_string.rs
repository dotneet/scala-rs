//! A value case class's synthesized `toString` has an `$extension` on the
//! companion, as nsc gives it: a client nsc compiles turns `v.toString` and
//! an interpolated `v` into `Code$.toString$extension(u)`. Only `copy`,
//! `equals` and `hashCode` had one, and such a client failed with
//! `NoSuchMethodError: Code$.toString$extension`. A value class that writes
//! its own `toString` keeps the extension its member gets.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package v
case class Code(value: String) extends AnyVal
case class Num(n: Int) extends AnyVal
case class Shown(n: Int) extends AnyVal { override def toString = "S" + n }
"#;

const USE: &str = r#"
object Main {
  def main(args: Array[String]): Unit =
    println((v.Code("x").toString, v.Num(3).toString, v.Shown(4).toString, s"${v.Code("y")}"))
}
"#;

const EXPECTED: &str = "(Code(x),Num(3),S4,Code(y))\n";

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
fn value_case_class_to_string_has_an_extension() {
    let tools = toolchain();
    let (Some(scalac_bin), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("value-case-class-to-string");
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
