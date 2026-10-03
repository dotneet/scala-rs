//! A `case class` local to a method of a class, and not reading the enclosing
//! instance. Its constructor still has the enclosing-instance slot, which
//! neither `copy` nor the static companion's `apply` filled (`VerifyError`,
//! `NoSuchMethodError` on the constructor). Extending a local trait also kept
//! `$outer` although the trait never reads the enclosing instance.

use crate::support::{toolchain, CompileCommand, TestDir};
use std::{fs, process::Command};

const SOURCE: &str = r#"
case class Area(lo: Int, hi: Int)
class Layout(areas: Seq[Area]) {
  def check(): List[String] = {
    sealed trait EOrX extends Product { def area: Area; def order: (Int, Boolean) }
    case class Enter(area: Area) extends EOrX { def order = (area.lo, true) }
    case class Exit(area: Area) extends EOrX { def order = (area.hi, false) }
    val out = List.newBuilder[String]
    for {
      e <- (for { a <- areas; if a.lo < a.hi; e <- Seq(Enter(a), Exit(a)) } yield e).sortBy(_.order)
    } e match {
      case Enter(a) => out += s"in $a"
      case Exit(a) => out += s"out $a"
    }
    out.result()
  }
  def more(): List[Any] = {
    trait T
    case class P(a: Int) extends T
    val p = P(1)
    List(p, p.copy(a = 2), List(3).map(P), P.unapply(p), p == P(1))
  }
}
object Main {
  def main(args: Array[String]): Unit = {
    val l = new Layout(Seq(Area(0, 2), Area(1, 3)))
    println(l.check())
    println(l.more())
  }
}
"#;

#[test]
fn local_case_class_fills_the_outer_slot() {
    let t = toolchain();
    let (Some(library), Some(java), Some(scalac)) = (t.scala_library(), t.java(), t.scalac())
    else {
        eprintln!("skip: scala-library, java or scalac is unavailable");
        return;
    };
    let dir = TestDir::new("local-case-class-outer-slot");
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
    assert_eq!(expected, "List(in Area(0,2), in Area(1,3), out Area(0,2), out Area(1,3))\nList(P(1), P(2), List(P(3)), Some(1), true)\n");
    assert_eq!(run(&ours), expected);
}

/// One that does read the enclosing instance needs a companion holding it,
/// which is not implemented: a compile error rather than a constructor call
/// that fails at run time.
#[test]
fn local_case_class_reading_the_enclosing_instance_is_refused() {
    let t = toolchain();
    let Some(library) = t.scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    let dir = TestDir::new("local-case-class-outer-refused");
    let source = dir.join("Main.scala");
    fs::write(
        &source,
        "class C { val k = 5; def f(): Int = { case class E(a: Int) { def g = a + k }; E(1).g } }\n",
    )
    .unwrap();
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    let run = CompileCommand::new(&source, &out)
        .classpath(library)
        .scala_library(library)
        .run();
    assert!(!run.success(), "the compile must fail");
    assert!(
        String::from_utf8_lossy(run.stderr())
            .contains("not implemented: a local `case class E` that reads the enclosing instance"),
        "{}",
        String::from_utf8_lossy(run.stderr())
    );
}
