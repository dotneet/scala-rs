//! Value-class extension methods across compilers.
//!
//! - A value class from the class path whose underlying type is a bounded
//!   type parameter (`final class Ext[O <: Seq[_]](val r: O)`) takes the
//!   bound in `m$extension`, as its class file says; the call was emitted
//!   with `Object` and did not link (`NoSuchMethodError`).
//! - A value case class of ours has the `copy$extension` nsc gives it, which
//!   a client compiled by nsc calls for `v.copy(...)`.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const THEIR_LIB: &str = r#"
package vl
final class Ext[O <: Seq[_], P](val r: O) extends AnyVal {
  def nonEmptyish: Boolean = r.nonEmpty
  def mapLen[Q](f: Int => Q): Q = f(r.length)
}
object Syntax { implicit def toExt[O <: Seq[_]](o: O): Ext[O, Int] = new Ext[O, Int](o) }
"#;

const OUR_USE: &str = r#"
import vl.Syntax._
object Main { def main(a: Array[String]): Unit = { println(List(1, 2).nonEmptyish); println(Vector(1).mapLen(_ + 1)) } }
"#;

const OUR_LIB: &str = r#"
package vc
final case class Names(protected val values: Seq[String]) extends AnyVal { def size = values.size }
"#;

const THEIR_USE: &str = r#"
object Main2 { def main(a: Array[String]): Unit = { val n = vc.Names(Seq("a")); println(n.copy(values = Seq("b", "c")).size + n.size) } }
"#;

#[test]
fn value_class_extensions_link_across_compilers() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("value-class-extension-abi");
    let files = [
        ("L.scala", THEIR_LIB),
        ("U.scala", OUR_USE),
        ("V.scala", OUR_LIB),
        ("W.scala", THEIR_USE),
    ];
    for (n, text) in files {
        fs::write(dir.join(n), text).unwrap();
    }
    for d in ["theirs", "ours", "ours-lib", "their-use"] {
        fs::create_dir_all(dir.join(d)).unwrap();
    }
    let scalac_run = |src: &str, cp: &str, out: &str| {
        let output = Command::new(scalac)
            .arg("-cp")
            .arg(cp)
            .arg("-d")
            .arg(dir.join(out))
            .arg(dir.join(src))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let lib = library.display().to_string();
    let at = |d: &str| format!("{}:{lib}", dir.join(d).display());
    let run = |cp: String, main: &str| {
        let out = Command::new(java)
            .args(["-Xverify:all", "-cp"])
            .arg(cp)
            .arg(main)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    scalac_run("L.scala", &lib, "theirs");
    CompileCommand::new(dir.join("U.scala"), dir.join("ours"))
        .classpath(at("theirs"))
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    assert_eq!(
        run(
            format!("{}:{}", dir.join("ours").display(), at("theirs")),
            "Main"
        ),
        "true\n2\n"
    );

    CompileCommand::new(dir.join("V.scala"), dir.join("ours-lib"))
        .classpath(&lib)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    scalac_run("W.scala", &at("ours-lib"), "their-use");
    assert_eq!(
        run(
            format!("{}:{}", dir.join("their-use").display(), at("ours-lib")),
            "Main2"
        ),
        "3\n"
    );
}
