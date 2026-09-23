//! A library compiled by scala-rs, read back by a real scalac client: the
//! Scala signature we pickle must describe members the way nsc would.

use crate::support;
use std::{fs, process::Command};

/// Compile `lib` with scala-rs and `client` against it with scalac, run
/// `Main`, and return its stdout (`None` when the toolchain is missing).
fn scalac_client_output(label: &str, lib: &str, client: &str) -> Option<String> {
    let tools = support::toolchain();
    let (Some(scalac), Some(library), Some(java)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip {label}: Scala toolchain or Java unavailable");
        return None;
    };
    let dir = support::TestDir::new(label);
    let lib_src = dir.join("Lib.scala");
    let use_src = dir.join("Use.scala");
    fs::write(&lib_src, lib).unwrap();
    fs::write(&use_src, client).unwrap();
    let lib_out = dir.join("lib");
    let use_out = dir.join("use");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&use_out).unwrap();
    support::CompileCommand::new(&lib_src, &lib_out)
        .scala_library(library)
        .run()
        .assert_success("scala-rs library");
    let client = support::CompileOutcome::from_output(
        Command::new(scalac)
            .arg("-cp")
            .arg(&lib_out)
            .arg("-d")
            .arg(&use_out)
            .arg(&use_src)
            .output()
            .unwrap(),
    );
    client.assert_success("scalac client against scala-rs library");
    let classpath = format!(
        "{}:{}:{}",
        use_out.display(),
        lib_out.display(),
        library.display()
    );
    let run = Command::new(java)
        .args(["-cp", &classpath, "Main"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "run failed:\n{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Some(String::from_utf8_lossy(&run.stdout).into_owned())
}

/// `val tag: Tag.type` names the nested object in a signature. The pickle
/// used to declare `object Tag` a second time for that reference, and scalac
/// bound `S.Tag` to the copy: `NoSuchMethodError: lib.S$.Tag()` at run time.
#[test]
fn nested_object_singleton_type_keeps_one_module_symbol() {
    let Some(out) = scalac_client_output(
        "scalac-client-nested-object-singleton",
        r#"package lib
object S {
  object Tag { override def toString = "Tag!" }
  val tag: Tag.type = Tag
}
"#,
        r#"object Main {
  def main(args: Array[String]): Unit = {
    println(lib.S.Tag)
    println(lib.S.tag)
  }
}
"#,
    ) else {
        return;
    };
    assert_eq!(out, "Tag!\nTag!\n");
}
