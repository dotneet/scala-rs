//! Rules a client compiled against a scalac-built library on a *directory*
//! classpath must apply exactly as nsc does, whatever order the library's
//! class files happen to be read in.

use crate::support::{toolchain, CompileCommand, CompileOutcome, RunCommand, TestDir};
use std::{fs, path::Path, process::Command};

/// Compile `lib` with scalac into `<dir>/lib` and `client` with scala-rs
/// against that directory; `None` when the toolchain is unavailable.
fn compile_against_lib_dir(
    label: &str,
    lib: &str,
    client: &str,
) -> Option<(TestDir, CompileOutcome)> {
    let tools = toolchain();
    let (Some(scalac), Some(library)) = (tools.scalac(), tools.scala_library()) else {
        eprintln!("skip: scalac or scala-library is unavailable");
        return None;
    };
    let dir = TestDir::new(label);
    let lib_src = dir.join("Lib.scala");
    let lib_out = dir.join("lib");
    let client_src = dir.join("Use.scala");
    let out = dir.join("out");
    fs::create_dir_all(&lib_out).unwrap();
    fs::create_dir_all(&out).unwrap();
    fs::write(&lib_src, lib).unwrap();
    fs::write(&client_src, client).unwrap();
    let status = Command::new(scalac)
        .arg("-d")
        .arg(&lib_out)
        .arg(&lib_src)
        .status()
        .expect("run scalac");
    assert!(status.success(), "scalac failed on the library");
    let outcome = CompileCommand::new(&client_src, &out)
        .classpath(&lib_out)
        .scala_library(library)
        .run();
    Some((dir, outcome))
}

fn run_main(dir: &Path) -> Option<String> {
    let tools = toolchain();
    let (Some(_), Some(library)) = (tools.java(), tools.scala_library()) else {
        eprintln!("skip: java is unavailable");
        return None;
    };
    let sep = if cfg!(windows) { ";" } else { ":" };
    let cp = format!(
        "{}{sep}{}{sep}{}",
        dir.join("out").display(),
        dir.join("lib").display(),
        library.display()
    );
    let run = RunCommand::new("Main").classpath(cp).run();
    run.assert_success("run Main");
    Some(run.stdout_string())
}

/// nsc: `found: a.Cell  required: b.Cell`. A nested class the directory
/// classpath only stubbed (its class file unread) was judged "not inner",
/// so `b.Cell` was the bare class and every `a.Cell` conformed to it -- in
/// a jar, or once anything completed the stub first, the same program was
/// rejected.
#[test]
fn directory_classpath_inner_class_keeps_its_prefix() {
    let lib = r#"
package lib
class A3 { class Cell(v: Int); def mk(v: Int): Cell = new Cell(v) }
class A5 { class Cell; def mk: Cell = new Cell }
object O { class Stat(v: Int) }
class O { def x = 1 }
"#;
    let bad = r#"
import lib._
object Main {
  def main(args: Array[String]): Unit = {
    val a3 = new A3; val b3 = new A3
    val x3: b3.Cell = a3.mk(1)
    val a5 = new A5; val b5 = new A5
    val x5: b5.Cell = a5.mk
  }
}
"#;
    let Some((_dir, outcome)) = compile_against_lib_dir("inner-prefix-bad", lib, bad) else {
        return;
    };
    assert!(!outcome.success(), "a3.Cell must not conform to b3.Cell");
    let diags = outcome.diagnostics();
    assert!(
        diags.contains("found: a3.Cell  required: b3.Cell")
            && diags.contains("found: a5.Cell  required: b5.Cell"),
        "{diags}"
    );

    let good = r#"
import lib._
object Main {
  def main(args: Array[String]): Unit = {
    val a3 = new A3
    val x3: a3.Cell = a3.mk(1)
    val y3: A3#Cell = x3
    val s: O.Stat = new O.Stat(1)
    println(y3 != null && s != null)
  }
}
"#;
    let Some((dir, outcome)) = compile_against_lib_dir("inner-prefix-good", lib, good) else {
        return;
    };
    outcome.assert_success("same-prefix inner class");
    if let Some(stdout) = run_main(&dir) {
        assert_eq!(stdout.trim(), "true");
    }
}

#[test]
fn projected_binary_alias_does_not_suppress_case_apply_overload() {
    let lib = r#"
package lib
class Code(val value: Int)
object Catalog {
  class Row(val value: Int)
  class Table { type Element = Row }
}
"#;
    let client = r#"
import lib.{Catalog, Code}
case class Entry(code: Code)
object Entry {
  def apply(row: Catalog.Table#Element): Entry = Entry(code = new Code(row.value))
}
object Main {
  def main(args: Array[String]): Unit = {
    println(Entry(new Code(7)).code.value)
    println(Entry(new Catalog.Row(9)).code.value)
  }
}
"#;
    let Some((dir, outcome)) = compile_against_lib_dir("projected-case-apply", lib, client) else {
        return;
    };
    outcome.assert_success("case apply overload beside a projected binary alias");
    if let Some(stdout) = run_main(&dir) {
        assert_eq!(stdout.trim(), "7\n9");
    }
    let tools = toolchain();
    let reference = dir.join("reference");
    fs::create_dir_all(&reference).unwrap();
    let status = Command::new(tools.scalac().unwrap())
        .arg("-cp")
        .arg(dir.join("lib"))
        .arg("-d")
        .arg(&reference)
        .arg(dir.join("Use.scala"))
        .status()
        .unwrap();
    assert!(status.success(), "scalac rejected the client");
    let cp = std::env::join_paths([
        reference,
        dir.join("lib"),
        tools.scala_library().unwrap().to_path_buf(),
    ])
    .unwrap();
    let run = RunCommand::new("Main").classpath(cp).run();
    run.assert_success("reference case apply overloads");
    assert_eq!(run.stdout_string().trim(), "7\n9");
}

/// nsc: `not enough arguments for constructor Plain: (tag: String)`. A
/// classpath class was never judged, so `new lib.Plain` compiled to an
/// `<init>()V` the class does not have and threw `NoSuchMethodError`. `Plain`
/// is also reached first through `Mk.p`'s descriptor, which stubs it the way
/// a Java class is stubbed; its pickled constructor still decides.
#[test]
fn unapplied_new_of_binary_class_needs_its_arguments() {
    let lib = r#"
package lib
class Plain(val tag: String)
object Mk { def p: Plain = new Plain("x") }
class Two(val a: Int)(implicit val n: Int)
class Dflt(val tag: String = "d") { override def toString = "Dflt" + tag }
class Impl(implicit val n: Int) { override def toString = "Impl" + n }
class Aux(val a: Int) { def this() = this(3); override def toString = "Aux" + a }
"#;
    let bad = r#"
object Main {
  def main(args: Array[String]): Unit = {
    implicit val n: Int = 4
    val q = lib.Mk.p
    val a = new lib.Plain
    val b = new lib.Plain()
    val t = new lib.Two
  }
}
"#;
    let Some((_dir, outcome)) = compile_against_lib_dir("unapplied-new-bad", lib, bad) else {
        return;
    };
    assert!(!outcome.success(), "new lib.Plain must be rejected");
    let diags = outcome.diagnostics();
    assert_eq!(
        diags
            .matches("not enough arguments for constructor Plain: (tag: String)")
            .count(),
        2,
        "{diags}"
    );
    assert!(
        diags.contains("not enough arguments for constructor Two: (a: Int)(implicit n: Int)")
            && diags.contains("Unspecified value parameter a."),
        "{diags}"
    );

    // Defaulted and implicit parameters, an auxiliary no-argument
    // constructor, Java classes and scala-library classes all still take a
    // bare `new`.
    let good = r#"
object Main {
  def main(args: Array[String]): Unit = {
    implicit val n: Int = 4
    val d = new lib.Dflt
    val i = new lib.Impl
    val x = new lib.Aux
    val sb = new java.lang.StringBuilder
    val ub = new scala.collection.mutable.UnrolledBuffer[Int]
    val pq = new scala.collection.mutable.PriorityQueue[Int]
    println(s"$d $i $x ${sb.length} ${ub.size} ${pq.size}")
  }
}
"#;
    let Some((dir, outcome)) = compile_against_lib_dir("unapplied-new-good", lib, good) else {
        return;
    };
    outcome.assert_success("omissible constructor arguments");
    if let Some(stdout) = run_main(&dir) {
        assert_eq!(stdout.trim(), "Dfltd Impl4 Aux3 0 0 0");
    }
}

/// nsc's `qualifies`: a wildcard import offers only what the reference site
/// may access. `private[lib] def pkgOnly` in `Util`, imported from another
/// package beside `Other`'s public `pkgOnly`, was entered too, and the
/// reference was "ambiguous" where nsc binds `Other.pkgOnly`.
#[test]
fn wildcard_import_skips_inaccessible_qualified_private() {
    let lib = r#"
package lib
object Util {
  private[lib] def pkgOnly: Int = 7
  def pub: Int = 1
}
object Other { def pkgOnly: Int = 8 }
"#;
    let client = r#"
import lib.Util._
import lib.Other._
object Main {
  def main(args: Array[String]): Unit = println(s"$pub $pkgOnly")
}
"#;
    let Some((dir, outcome)) = compile_against_lib_dir("qualified-private-import", lib, client)
    else {
        return;
    };
    outcome.assert_success("private[lib] member does not compete");
    if let Some(stdout) = run_main(&dir) {
        assert_eq!(stdout.trim(), "1 8");
    }

    // Alone, the inaccessible member is no binding at all: nsc's "not found".
    let alone = r#"
import lib.Util._
object Main {
  def main(args: Array[String]): Unit = println(pkgOnly)
}
"#;
    let Some((_dir, outcome)) = compile_against_lib_dir("qualified-private-alone", lib, alone)
    else {
        return;
    };
    assert!(!outcome.success(), "private[lib] pkgOnly must not be reachable");
    let diags = outcome.diagnostics();
    assert!(diags.contains("not found: value pkgOnly"), "{diags}");
}

/// The same rule for a qualified-private member compiled in this run, and
/// the member still binds inside its boundary.
#[test]
fn wildcard_import_skips_inaccessible_qualified_private_from_source() {
    let tools = toolchain();
    let (Some(library), Some(_)) = (tools.scala_library(), tools.java()) else {
        eprintln!("skip: scala-library or Java is unavailable");
        return;
    };
    let dir = TestDir::new("qualified-private-source");
    let src = dir.join("Use.scala");
    let out = dir.join("out");
    fs::create_dir_all(&out).unwrap();
    fs::write(
        &src,
        r#"
package lib {
  object Util { private[lib] def pkgOnly: Int = 7 }
  object Other { def pkgOnly: Int = 8 }
  object Inside {
    import Util._
    def get: Int = pkgOnly
  }
}
package app {
  import lib.Util._
  import lib.Other._
  object Main {
    def main(args: Array[String]): Unit = println(s"$pkgOnly ${lib.Inside.get}")
  }
}
"#,
    )
    .unwrap();
    CompileCommand::new(&src, &out)
        .scala_library(library)
        .run()
        .assert_success("source private[lib] member does not compete");
    let sep = if cfg!(windows) { ";" } else { ":" };
    let run = RunCommand::new("app.Main")
        .classpath(format!("{}{sep}{}", out.display(), library.display()))
        .run();
    run.assert_success("run app.Main");
    assert_eq!(run.stdout_string().trim(), "8 7");

    fs::write(
        &src,
        r#"
package lib {
  object Util { private[lib] def pkgOnly: Int = 7 }
}
package app {
  import lib.Util._
  object Main {
    def main(args: Array[String]): Unit = println(pkgOnly)
  }
}
"#,
    )
    .unwrap();
    let outcome = CompileCommand::new(&src, &out).scala_library(library).run();
    assert!(!outcome.success(), "private[lib] pkgOnly must not be reachable");
    let diags = outcome.diagnostics();
    assert!(diags.contains("not found: value pkgOnly"), "{diags}");
}
