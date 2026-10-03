//! A trait's concrete method that overrides a library trait's method at a
//! narrower erasure (`trait E[T <: C] extends Encoder[T] { def apply(a: T) }`
//! is `apply(C)` against `Encoder.apply(Object)`) gets its bridge on the
//! interface, as a `default` method. The classes mixing the trait in -- an
//! anonymous class, a SAM lambda's class -- inherit the implementation, and
//! without the bridge the library's call died with `AbstractMethodError`.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package enc
trait Encoder[A] { def apply(a: A): String }
object Encoder { def run[A](e: Encoder[A], a: A): String = e(a) }
"#;

const SOURCE: &str = r#"
package c
class Claim(val n: Int)
trait ClaimEncoder[T <: Claim] extends enc.Encoder[T] {
  final override def apply(a: T): String = "<" + encodeClaim(a) + ">"
  def encodeClaim(entry: T): String
}
object ClaimEncoder {
  def of[T <: Claim](f: T => String): ClaimEncoder[T] = (entry: T) => f(entry)
  def anon[T <: Claim](f: T => String): ClaimEncoder[T] = new ClaimEncoder[T] { def encodeClaim(e: T) = f(e) }
}
class Named extends ClaimEncoder[Claim] { def encodeClaim(e: Claim) = "named" + e.n }
object Main {
  def main(args: Array[String]): Unit = {
    println(enc.Encoder.run(ClaimEncoder.of[Claim](c => "of" + c.n), new Claim(1)))
    println(enc.Encoder.run(ClaimEncoder.anon[Claim](c => "anon" + c.n), new Claim(2)))
    println(enc.Encoder.run(new Named, new Claim(3)))
  }
}
"#;

#[test]
fn trait_bridges_to_a_narrower_override_are_default_methods() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("trait-bridge-default");
    let lib = dir.join("Enc.scala");
    let source = dir.join("Claim.scala");
    fs::write(&lib, LIB).unwrap();
    fs::write(&source, SOURCE).unwrap();
    let lib_out = dir.join("lib");
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    for d in [&lib_out, &ours, &theirs] {
        fs::create_dir_all(d).unwrap();
    }
    let scalac_run = |src: &std::path::Path, cp: &str, out: &std::path::Path| {
        let output = Command::new(scalac)
            .arg("-cp")
            .arg(cp)
            .arg("-d")
            .arg(out)
            .arg(src)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    scalac_run(&lib, &library.display().to_string(), &lib_out);
    let cp = format!("{}:{}", lib_out.display(), library.display());
    scalac_run(&source, &cp, &theirs);
    CompileCommand::new(&source, &ours)
        .classpath(&cp)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let run = |classes: &std::path::Path| {
        let out = Command::new(java)
            .args(["-Xverify:all", "-cp"])
            .arg(format!("{}:{cp}", classes.display()))
            .arg("c.Main")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let expected = run(&theirs);
    assert_eq!(expected, "<of1>\n<anon2>\n<named3>\n");
    assert_eq!(run(&ours), expected);
}
