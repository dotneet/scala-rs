//! A class implementing a generic scala-library trait gets the erasure
//! bridges the library calls through.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::{fs, process::Command};

/// Compiles `source` with scalac and scala-rs, runs `Main` from each, and
/// checks that scala-rs's run succeeds with scalac's stdout.
fn assert_runs_like_scalac(label: &str, source: &str) {
    let tools = toolchain();
    let (Some(scalac), Some(library)) = (tools.scalac(), tools.scala_library()) else {
        eprintln!("skip {label}: scalac or scala-library is unavailable");
        return;
    };
    if !tools.has_java() {
        eprintln!("skip {label}: Java is unavailable");
        return;
    }
    let dir = TestDir::new(label);
    let file = dir.join("Main.scala");
    fs::write(&file, source).unwrap();

    let theirs = dir.join("scalac");
    fs::create_dir_all(&theirs).unwrap();
    let compiled = Command::new(scalac)
        .arg(&file)
        .arg("-d")
        .arg(&theirs)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "scalac rejected the fixture:\n{}{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );
    let expected = RunCommand::new("Main")
        .classpath(format!("{}:{}", theirs.display(), library.display()))
        .run();
    expected.assert_success("scalac's classes");

    let ours = dir.join("ours");
    fs::create_dir_all(&ours).unwrap();
    CompileCommand::new(&file, &ours)
        .scala_library(library)
        .run()
        .assert_success("scala-rs compile");
    let actual = RunCommand::new("Main")
        .classpath(format!("{}:{}", ours.display(), library.display()))
        .run();
    actual.assert_success("scala-rs classes");
    assert_eq!(actual.stdout_string(), expected.stdout_string());
}

#[test]
fn prelude_library_traits_get_erasure_bridges() {
    assert_runs_like_scalac(
        "library-trait-bridges-prelude",
        r#"
class Desc extends Ordering[String] { def compare(a: String, b: String) = b.compareTo(a) }
class IntDesc extends Ordering[Int] { def compare(a: Int, b: Int) = b - a }
class Pos extends PartialFunction[Int, String] {
  def isDefinedAt(x: Int) = x > 0
  def apply(x: Int) = s"pf$x"
}
class NoCase extends Equiv[String] { def equiv(a: String, b: String) = a.equalsIgnoreCase(b) }
object Main {
  def main(args: Array[String]): Unit = {
    println(List("a", "c", "b").sorted(new Desc))
    println(List(1, 3, 2).sorted(new IntDesc))
    println(List("a", "c", "b").sorted(new Ordering[String] {
      def compare(a: String, b: String) = b.compareTo(a)
    }))
    println(List(-1, 2).collect(new Pos))
    val eq: Equiv[String] = new NoCase
    println(eq.equiv("a", "A"))
  }
}
"#,
    );
}
