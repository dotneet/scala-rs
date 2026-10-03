//! `q.Atom(2, k = 3)` for a case class from the class path: the qualified
//! reference to the companion is not rewritten to `q.Atom.apply`, and the
//! path that types it placed the named argument without filling the default
//! it skipped, so the call was matched against `(2, 3)` and reported "no
//! matching overload". An unqualified `Atom(2, k = 3)` and a class compiled in
//! the same run were not affected.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package q
case class Atom(n: Int, s: String = "x", k: Long = 1L)
object Holder { case class In(a: Int = 1, b: String = "b", c: Boolean = false) }
"#;

const USE: &str = r#"
object Main {
  def main(args: Array[String]): Unit =
    println((q.Atom(2, k = 3), q.Atom(n = 4, s = "y"), q.Holder.In(c = true), q.Holder.In(2, c = true)))
}
"#;

const EXPECTED: &str = "(Atom(2,x,3),Atom(4,y,1),In(1,b,true),In(2,b,true))\n";

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
fn qualified_companion_call_fills_skipped_defaults() {
    let tools = toolchain();
    let (Some(scalac_bin), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("qualified-named-default-apply");
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
