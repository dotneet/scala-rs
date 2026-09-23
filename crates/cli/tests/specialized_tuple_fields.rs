//! A scalac-built specialized pair is read through its accessors.
//!
//! `Tuple2$mcII$sp` keeps its values in `_1$mcI$sp` and never writes the
//! generic `_1` / `_2` fields, so a `getfield scala/Tuple2._1` on it reads
//! `null` (unboxed to `0`). A case class's `unapply` builds exactly such a
//! pair, which silently zeroed every extractor pattern on a binary case class.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, process::Command};

const LIB: &str = r#"
package lib
case class Point(x: Int, y: Int)
case class Rect(w: Double, h: Double)
object L {
  def pair: (Int, Int) = (3, 4)
  def pairs: List[(Int, Int)] = List((1, 2), (5, 6))
  def mixed: (Long, Char) = (9L, 'z')
}
"#;

const USE: &str = r#"
import lib._
object Main {
  def main(args: Array[String]): Unit = {
    Point(1, 2) match { case Point(a, b) => println(s"point $a $b") }
    Rect(2.5, 3.5) match { case Rect(w, h) => println(s"rect $w $h") }
    val Point(c, d) = Point(7, 8); println(s"val $c $d")
    println(L.pair._1 + L.pair._2)
    println(L.pairs.map(_._1))
    L.pair match { case (a, b) => println(s"tuple $a $b") }
    println(L.mixed._1 + " " + L.mixed._2)
    val (e, f) = L.pair; println(e * f)
  }
}
"#;

const EXPECTED: &str = "point 1 2\nrect 2.5 3.5\nval 7 8\n7\nList(1, 5)\ntuple 3 4\n9 z\n12\n";

#[test]
fn scalac_specialized_tuple_fields_are_read_through_accessors() {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("specialized-tuple-fields");
    let lib_src = dir.join("Lib.scala");
    let use_src = dir.join("Use.scala");
    fs::write(&lib_src, LIB).unwrap();
    fs::write(&use_src, USE).unwrap();
    let lib_out = dir.join("lib");
    let use_out = dir.join("use");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&use_out).unwrap();

    CompileOutcome::from_output(
        Command::new(scalac)
            .arg("-d")
            .arg(&lib_out)
            .arg(&lib_src)
            .output()
            .unwrap(),
    )
    .assert_success("scalac library");
    CompileCommand::new(&use_src, &use_out)
        .classpath(&lib_out)
        .scala_library(library)
        .run()
        .assert_success("scala-rs client");

    let classpath = format!(
        "{}:{}:{}",
        use_out.display(),
        lib_out.display(),
        library.display()
    );
    let run = RunCommand::new("Main").classpath(&classpath).run();
    run.assert_success("run client");
    assert_eq!(run.stdout_string(), EXPECTED);
}
