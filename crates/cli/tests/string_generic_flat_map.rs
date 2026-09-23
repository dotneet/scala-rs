//! `StringOps.flatMap[B](Char => IterableOnce[B]): IndexedSeq[B]` sits
//! beside the `flatMap(Char => String): String` overload, on `StringOps`
//! itself and on `StringOps.WithFilter`.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::fs;

#[test]
fn string_flat_map_to_a_collection_uses_the_generic_overload() {
    let tools = toolchain();
    let Some(library) = tools.scala_library() else {
        eprintln!("skip: scala-library is unavailable");
        return;
    };
    if !tools.has_java() {
        eprintln!("skip: Java is unavailable");
        return;
    }
    let dir = TestDir::new("string-generic-flat-map");
    let source = dir.join("Main.scala");
    let classes = dir.join("classes");
    fs::create_dir(&classes).unwrap();
    fs::write(
        &source,
        r#"
object Main {
  def main(args: Array[String]): Unit = {
    println("hello".flatMap(c => List(c, c)))
    println("ab".flatMap(c => c.toString * 2))
    println("ab".flatMap(_.toString))
    println("ab".flatMap(c => Some(c.toInt)))
    println(for (a <- "ab"; b <- "cd") yield s"$a$b")
    println(for (a <- "ab"; b <- "cd") yield b)
    println(for (c <- "hello" if c != 'l'; d <- List(c, c)) yield d)
    println("hello".withFilter(_ != 'l').flatMap(c => List(c, c)))
    println("hello".withFilter(_ != 'l').flatMap(c => c.toString * 2))
    val v: IndexedSeq[Int] = "xy".flatMap(c => Vector(c.toInt, 1))
    println(v)
    for (a <- "ab"; b <- List(1, 2)) print(s"$a$b ")
    println()
  }
}
"#,
    )
    .unwrap();

    CompileCommand::new(&source, &classes)
        .scala_library(library)
        .run()
        .assert_success("compile");
    let run = RunCommand::new("Main")
        .classpath(format!("{}:{}", classes.display(), library.display()))
        .run();
    run.assert_success("run");
    // scalac 2.13.16 output.
    assert_eq!(
        run.stdout_string(),
        "Vector(h, h, e, e, l, l, l, l, o, o)\n\
         aabb\n\
         ab\n\
         Vector(97, 98)\n\
         Vector(ac, ad, bc, bd)\n\
         cdcd\n\
         Vector(h, h, e, e, o, o)\n\
         Vector(h, h, e, e, o, o)\n\
         hheeoo\n\
         Vector(120, 1, 121, 1)\n\
         a1 a2 b1 b2 \n"
    );
}
