//! A default argument is stored on its getter's symbol rather than in the
//! unit's tree, and only a getter returning a value class went through
//! erasure. Any other default that used a value class -- `Instant.EPOCH
//! plusMillis Iv(d).millis` through an implicit class over `Iv` -- reached
//! the backend unerased, and `Iv.apply`'s underlying `Duration` met a cast
//! to the box (`ClassCastException`) when a client nsc compiled called the
//! getter.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package q
import java.time.Instant
import scala.concurrent.duration._
case class Iv(value: Duration) extends AnyVal
object Iv {
  implicit class Ops(value: Iv) {
    def millis: Long = value.value match {
      case _: Duration.Infinite => -1L
      case x: FiniteDuration => x.toMillis
    }
  }
}
case class Atom(n: Int, next: Instant = Instant.EPOCH plusMillis Iv(2.seconds).millis, k: Long = Iv(Duration.Inf).millis)
"#;

const USE: &str = r#"
object Main {
  def main(args: Array[String]): Unit = println((q.Atom(1), q.Atom(2, k = 3)))
}
"#;

const EXPECTED: &str = "(Atom(1,1970-01-01T00:00:02Z,-1),Atom(2,1970-01-01T00:00:02Z,3))\n";

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
fn default_getter_bodies_are_erased() {
    let tools = toolchain();
    let (Some(scalac_bin), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("erased-default-getter");
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
