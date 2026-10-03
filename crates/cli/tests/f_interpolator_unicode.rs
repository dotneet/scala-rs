//! The literal parts of an `f` interpolation keep their non-ASCII characters.
//! They were copied byte by byte, each UTF-8 byte becoming a character of its
//! own, so `f"【$x】..."` came out as `ã...` (a file name built that way was
//! garbled).

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
object Main {
  def main(args: Array[String]): Unit = {
    val x = "市"
    val m = 3
    val parts = List(f"【$x】キュ${m}%02d月", f"é$x%s 100%% ü", f"${m}%d☃%n".trim)
    for (p <- parts) println(p.length + " " + p.map(_.toInt).mkString(","))
  }
}
"#;

#[test]
fn f_interpolation_keeps_non_ascii_literals() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("f-interpolator-unicode");
    let source = dir.join("Main.scala");
    fs::write(&source, SOURCE).unwrap();
    let ours = dir.join("ours");
    let theirs = dir.join("theirs");
    fs::create_dir_all(&ours).unwrap();
    fs::create_dir_all(&theirs).unwrap();
    CompileCommand::new(&source, &ours)
        .classpath(library)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let output = Command::new(scalac)
        .arg("-d")
        .arg(&theirs)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = |classes: &std::path::Path| {
        let out = Command::new(java)
            .args(["-Xverify:all", "-cp"])
            .arg(format!("{}:{}", classes.display(), library.display()))
            .arg("Main")
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
    assert_eq!(expected, "8 12304,24066,12305,12461,12517,48,51,26376\n9 233,24066,32,49,48,48,37,32,252\n2 51,9731\n");
    assert_eq!(run(&ours), expected);
}
