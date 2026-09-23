//! Members a client reaches through a scalac-compiled library, checked on both
//! classpath forms: a directory and the same classes packaged as a jar. The
//! two are read by different class-file paths (a directory scan carries no
//! descriptors), so a member can type on one and not the other.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::{fs, path::Path, process::Command};

/// Compile `lib` with scalac, then `client` with scala-rs against the library
/// as a directory and as a jar, run each `Main` and compare with `expected`.
fn agrees_on_both_classpaths(label: &str, lib: &str, client: &str, expected: &str) {
    let tools = toolchain();
    let (Some(library), Some(scalac), Some(java)) =
        (tools.scala_library(), tools.scalac(), tools.java())
    else {
        eprintln!("skip {label}: Scala 2.13.16 toolchain and Java are required");
        return;
    };
    let dir = TestDir::new(label);
    let lib_src = dir.join("Lib.scala");
    let client_src = dir.join("Use.scala");
    fs::write(&lib_src, lib).unwrap();
    fs::write(&client_src, client).unwrap();

    let lib_dir = dir.join("lib");
    fs::create_dir(&lib_dir).unwrap();
    let out = Command::new(scalac)
        .arg("-d")
        .arg(&lib_dir)
        .arg(&lib_src)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "scalac rejected the library:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lib_jar = dir.join("lib.jar");
    let jar_tool = java.with_file_name("jar");
    let out = Command::new(if jar_tool.is_file() {
        jar_tool.as_path()
    } else {
        Path::new("jar")
    })
    .arg("cf")
    .arg(&lib_jar)
    .arg("-C")
    .arg(&lib_dir)
    .arg(".")
    .output()
    .unwrap();
    assert!(
        out.status.success(),
        "jar failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    for (form, cp) in [("directory", &lib_dir), ("jar", &lib_jar)] {
        let classes = dir.join(format!("classes-{form}"));
        fs::create_dir(&classes).unwrap();
        CompileCommand::new(&client_src, &classes)
            .classpath(cp)
            .scala_library(library)
            .run()
            .assert_success(&format!("scala-rs compile ({form} classpath)"));
        let classpath = format!(
            "{}:{}:{}",
            classes.display(),
            cp.display(),
            library.display()
        );
        let run = RunCommand::new("Main").classpath(&classpath).run();
        run.assert_success(&format!("run ({form} classpath)"));
        assert_eq!(run.stdout_string(), expected, "{form} classpath");
    }
}

/// The first selection of an inherited trait method replaces the raw class
/// file forwarder with the pickle's declaration; a directory scan leaves the
/// forwarder in the member table, and the second selection used to see both
/// as an overload `Tuple2[Any, Any] | (Int, Int)`.
#[test]
fn second_selection_of_an_inherited_member_is_not_an_overload() {
    agrees_on_both_classpaths(
        "binary-member-reselect",
        r#"
package lib
trait Base { type T; def make: T; def pair: (T, T) = (make, make) }
class Impl extends Base { type T = Int; def make: Int = 3 }
"#,
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    val i = new lib.Impl
    val a = i.pair
    val b = i.pair
    println(a._1 + b._2)
  }
}
"#,
        "6\n",
    );
}
