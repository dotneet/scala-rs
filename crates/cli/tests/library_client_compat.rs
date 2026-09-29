//! Shapes a multi-module application build met once each module compiled
//! against the class files of the others: a client compiled by scala-rs
//! against a scalac-built library, run and compared with scalac's client.

use crate::support::{toolchain, CompileCommand, RunCommand, TestDir};
use std::{fs, path::Path, process::Command};

/// Compile `lib` with scalac, then `client` (plus `extra` sources, which may
/// be Java) with both compilers against it; run both `Main`s and return
/// (scalac's stdout, scala-rs's stdout).
fn run_with_both(
    label: &str,
    lib: &str,
    client: &str,
    extra: &[(&str, &str)],
) -> Option<(String, String)> {
    let tools = toolchain();
    let (Some(scalac), Some(library), Some(_)) =
        (tools.scalac(), tools.scala_library(), tools.java())
    else {
        eprintln!("skip: scalac, java or scala-library is unavailable");
        return None;
    };
    let dir = TestDir::new(label);
    let (lib_src, client_src) = (dir.join("Lib.scala"), dir.join("Use.scala"));
    let (lib_out, sc_out, rs_out) = (dir.join("lib"), dir.join("sc"), dir.join("rs"));
    for d in [&lib_out, &sc_out, &rs_out] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(&lib_src, lib).unwrap();
    fs::write(&client_src, client).unwrap();
    let mut sources = vec![client_src.clone()];
    for (name, text) in extra {
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        sources.push(path);
    }
    let scalac_to = |out: &Path, srcs: &[std::path::PathBuf]| {
        let status = Command::new(scalac)
            .arg("-cp")
            .arg(&lib_out)
            .arg("-d")
            .arg(out)
            .args(srcs)
            .status()
            .expect("run scalac");
        assert!(status.success(), "scalac failed on {srcs:?}");
    };
    scalac_to(&lib_out, std::slice::from_ref(&lib_src));
    scalac_to(&sc_out, &sources);
    // scalac leaves a Java source's class files to javac.
    let java: Vec<_> = sources
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "java"))
        .collect();
    if !java.is_empty() {
        let status = Command::new("javac")
            .arg("-cp")
            .arg(&lib_out)
            .arg("-d")
            .arg(&sc_out)
            .args(&java)
            .status()
            .expect("run javac");
        assert!(status.success(), "javac failed");
    }
    let mut rs = CompileCommand::new(&sources[0], &rs_out);
    for extra in &sources[1..] {
        rs = rs.arg(extra);
    }
    rs.classpath(&lib_out)
        .scala_library(library)
        .run()
        .assert_success("scala-rs client");
    for source in &java {
        let class = format!("{}.class", source.file_stem().unwrap().to_string_lossy());
        assert!(
            !contains_file(&rs_out, &class),
            "scala-rs wrote {class}, which is javac's to write"
        );
    }
    let run = |out: &Path, java_out: &Path| {
        let cp = format!(
            "{}:{}:{}:{}",
            out.display(),
            java_out.display(),
            lib_out.display(),
            library.display()
        );
        let run = RunCommand::new("Main").classpath(cp).run();
        run.assert_success("run Main");
        run.stdout_string()
    };
    Some((run(&sc_out, &sc_out), run(&rs_out, &sc_out)))
}

fn contains_file(dir: &Path, name: &str) -> bool {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                contains_file(&path, name)
            } else {
                entry.file_name() == name
            }
        })
}

/// A module entered from its class-file header alone -- a signature read
/// first named one of its type aliases, `def add(fetch: A.Fetch)` -- has no
/// terms yet. `import lib.A.fetch` after `import lib.B.add` reported "value
/// fetch is not a member of object lib.A"; the other import order compiled.
#[test]
fn a_module_first_reached_through_a_type_alias_keeps_its_terms() {
    let lib = r#"
package lib
object A { type Fetch = Int => Int; def fetch: Fetch = _ + 1 }
object B { def add(fetch: A.Fetch): Int = fetch(41) }
"#;
    let client = r#"
import lib.B.add
import lib.A.fetch
object Main { def main(args: Array[String]): Unit = println(add(fetch)) }
"#;
    let Some((scalac, scala_rs)) = run_with_both("stub-module-terms", lib, client, &[]) else {
        return;
    };
    assert_eq!(scala_rs, scalac);
}

/// `type Identified[+T <: Identify] = IdentifiedEntity[T#ID, T]` applied at
/// an enclosing type parameter: the projection has to survive the alias so
/// that `T := Foo` reduces it to `Foo`'s `ID`. It fell back to the abstract
/// declaration, and `Foo(1).identify(FooId(2))` was an
/// `IdentifiedEntity[Identify.ID, Foo]`.
#[test]
fn a_projection_through_an_alias_parameter_reduces_at_the_argument() {
    let lib = r#"
package idf
trait Identity[P] { val value: P }
trait Identify { type ID <: Identity[_] }
case class IdentifiedEntity[+ID <: Identity[_], +S <: Identify](id: ID, value: S)
object Identify {
  type Identified[+T <: Identify] = IdentifiedEntity[T#ID, T]
  implicit class EntityFragment[T <: Identify](v: T) {
    def identify(identity: T#ID): Identified[T] = IdentifiedEntity(identity, v)
  }
}
"#;
    let client = r#"
import idf._
import idf.Identify._
case class FooId(value: Int) extends Identity[Int]
case class Foo(n: Int) extends Identify { type ID = FooId }
object Main {
  def main(args: Array[String]): Unit = {
    val a: IdentifiedEntity[FooId, Foo] = Foo(1).identify(FooId(2))
    val b: Identified[Foo] = Foo(3).identify(FooId(4))
    val c: IdentifiedEntity[FooId, Foo] = b
    println((a.id.value, c.id.value))
  }
}
"#;
    let Some((scalac, scala_rs)) = run_with_both("alias-projection", lib, client, &[]) else {
        return;
    };
    assert_eq!(scala_rs, scalac);
}

/// A positional argument after a named one that stands in its own position
/// is legal, and decides between overloads: `g(roles = 2, "s")` is
/// `g(roles: Int, s: String)`, not `g(x: Int, roles: Int)` -- which was
/// picked, then rejected with "positional after named argument". A written
/// companion `apply` beside a case class's own is the same choice.
#[test]
fn named_then_positional_arguments_choose_the_overload_they_fit() {
    let lib = r#"
package ov
case class R(a: Int, roles: Int, ext: String)
object R { def apply(roles: Int, ext: String)(k: Int): R = new R(k, roles, ext) }
object G {
  def g(x: Int, roles: Int): Int = x + roles
  def g(roles: Int, s: String): String = s + roles
}
"#;
    let client = r#"
import ov._
object Main {
  def main(args: Array[String]): Unit = {
    println(R(roles = 1, "x")(9))
    println(G.g(roles = 2, "s"))
    println(G.g(x = 1, roles = 3))
  }
}
"#;
    let Some((scalac, scala_rs)) = run_with_both("named-positional", lib, client, &[]) else {
        return;
    };
    assert_eq!(scala_rs, scalac);
}

/// A Java source beside the Scala ones is read for its declarations, as nsc
/// does; its class files are javac's to write, so none of them appear in the
/// output directory.
#[test]
fn a_java_source_in_the_same_compilation_is_visible_to_scala() {
    let tools = toolchain();
    if tools.java().is_none() || Command::new("javac").arg("-version").output().is_err() {
        eprintln!("skip: javac is unavailable");
        return;
    }
    let lib = "package lib\nobject Unused\n";
    let java = r#"
package mixed;
public final class Limits {
    public static final int MAX = 42;
    public enum Level { LOW, HIGH }
    public static String describe(Level level) { return level.name().toLowerCase(); }
}
"#;
    let client = r#"
import mixed.Limits
object Main {
  def main(args: Array[String]): Unit =
    println((Limits.MAX, Limits.describe(Limits.Level.HIGH), Limits.Level.values.length))
}
"#;
    let Some((scalac, scala_rs)) =
        run_with_both("java-source", lib, client, &[("Limits.java", java)])
    else {
        return;
    };
    assert_eq!(scala_rs, scalac);
}
