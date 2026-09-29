//! Implicit scopes a scalac-built library contributes to views and implicit
//! values, compared with scalac on the same two compilation units.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::{fs, process::Command};

/// Compile `lib` with scalac, then `client` with both compilers against it,
/// run both `Main`s and return (scalac's stdout, scala-rs's stdout).
fn run_with_both(label: &str, lib: &str, client: &str) -> Option<(String, String)> {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, java or scala-library is unavailable");
        return None;
    };
    let dir = TestDir::new(label);
    let (lib_src, client_src) = (dir.join("Lib.scala"), dir.join("Use.scala"));
    let (lib_out, sc_out, rs_out) = (dir.join("lib"), dir.join("sc"), dir.join("rs"));
    for d in [&lib_out, &sc_out, &rs_out] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(&lib_src, lib).unwrap();
    fs::write(&client_src, client).unwrap();
    let scalac_to = |out: &std::path::Path, src: &std::path::Path| {
        let status = Command::new(scalac)
            .arg("-cp")
            .arg(&lib_out)
            .arg("-d")
            .arg(out)
            .arg(src)
            .status()
            .expect("run scalac");
        assert!(status.success(), "scalac failed on {}", src.display());
    };
    scalac_to(&lib_out, &lib_src);
    scalac_to(&sc_out, &client_src);
    CompileCommand::new(&client_src, &rs_out)
        .classpath(&lib_out)
        .scala_library(library)
        .run()
        .assert_success("scala-rs client");
    let run = |out: &std::path::Path| {
        let cp = format!(
            "{}:{}:{}",
            out.display(),
            lib_out.display(),
            library.display()
        );
        let run = RunCommand::new("Main").classpath(cp).run();
        run.assert_success("run Main");
        run.stdout_string()
    };
    Some((run(&sc_out), run(&rs_out)))
}

/// `Some(x)` and `None` are typed `Some[A]` and `None.type`, and
/// `Option.option2Iterable` is in their implicit scope only through their
/// base class `Option`. A parameter `Iterable[C]` given `None` -- a
/// scalac-built case class's `apply` is where an application first met it --
/// found no view, and neither did `val xs: Iterable[Int] = Some(1)`.
#[test]
fn option_views_reach_some_and_none_through_their_base_class() {
    let lib = r#"
package lib
object Claims {
  case class Simple[I, C](issuer: Option[I], audience: Iterable[C], lifetime: Long)
  def count(xs: Iterable[Int]): Int = xs.size
}
"#;
    let client = r#"
import lib.Claims
object Main {
  def main(args: Array[String]): Unit = {
    val s = Claims.Simple(None, None, 3L)
    val xs: Iterable[Int] = Some(1)
    println((s.audience.size, s.lifetime, xs.toList, Claims.count(Some(2)), Claims.count(None)))
  }
}
"#;
    let Some((scalac, scala_rs)) = run_with_both("option-view-base", lib, client) else {
        return;
    };
    assert_eq!(scala_rs, scalac);
}

/// An implicit a module inherits from a trait is one declaration however it
/// is reached. Naming it through the module (`Reader.exportedReader[Int]`)
/// completes a copy onto the module, and that copy's owner made it outrank
/// the other traits' implicits -- tying with the declaration it copies, so a
/// later `implicitly[Reader[String]]` was ambiguous. pureconfig's
/// `ConfigReader` has this shape: the second `load[...]` derivation in a file
/// failed with "Unable to infer value of type WeakTypeTag[...]".
#[test]
fn an_inherited_implicit_named_through_its_module_stays_one_candidate() {
    let lib = r#"
package rd
trait Exported[A] { def value: A }
trait Reader[A] { def name: String }
trait PrimReaders { implicit val stringReader: Reader[String] = new Reader[String] { def name = "string" } }
trait ExportedReaders {
  implicit def exportedReader[A](implicit e: Exported[Reader[A]]): Reader[A] = e.value
}
object Reader extends PrimReaders with ExportedReaders
object auto {
  implicit def exportAny[A]: Exported[Reader[A]] =
    new Exported[Reader[A]] { def value = new Reader[A] { def name = "exported" } }
}
"#;
    let client = r#"
import rd._
import rd.auto._
object Main {
  def main(args: Array[String]): Unit = {
    val viaModule = Reader.exportedReader[Int]
    println(viaModule.name + " " + implicitly[Reader[String]].name)
  }
}
"#;
    let Some((scalac, scala_rs)) = run_with_both("inherited-implicit-copy", lib, client) else {
        return;
    };
    assert_eq!(scala_rs, scalac);
}
