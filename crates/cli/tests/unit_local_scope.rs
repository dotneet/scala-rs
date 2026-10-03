//! A `Unit` local declared just before a nested block. It occupies no slot, so
//! it sat at the block's first slot and was released with the block's own
//! locals, and a later read became a field access on a class named after the
//! enclosing method (`NoClassDefFoundError: $anonfun`). A for comprehension
//! with `_ = sideEffect()` followed by a named-argument `copy` is one way to get
//! there.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
case class Ans(at: Int, id: String)
object Main {
  def plain(n: Int): (Unit, Int) = { val u = println("p" + n); val e = { val t = n + 1; t * 2 }; (u, e) }
  def main(args: Array[String]): Unit = {
    println(for { a <- List(1); _ = println(a); e = Ans(0, "x").copy(at = a); b <- List(e) } yield b)
    println(plain(3))
    val f = (n: Int) => { val u = println("f" + n); val e = { val t = n; t }; List(u, e) }
    println(f(4))
  }
}
"#;

#[test]
fn unit_local_survives_a_nested_block() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("unit-local-scope");
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
    assert_eq!(expected, "1\nList(Ans(1,x))\np3\n((),8)\nf4\nList((), 4)\n");
    assert_eq!(run(&ours), expected);
}
