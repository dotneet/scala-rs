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
fn abstract_superclass_gets_bridge_to_trait_implementation() {
    assert_runs_like_scalac(
        "abstract-super-trait-bridge",
        r#"
abstract class Base {
  protected def value(): Base
  final def read(): Base = value()
}
trait Impl extends Base {
  override def value(): Impl = this
}
abstract class Middle extends Base with Impl
final class Leaf extends Middle
object Main {
  def main(args: Array[String]): Unit = println(new Leaf().read().getClass.getSimpleName)
}
"#,
    );
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

/// The prelude declares `Numeric` without members, so the overridden
/// `fromInt(Int): T` has to come from its pickle before the bridge pass.
#[test]
fn memberless_prelude_traits_get_erasure_bridges() {
    assert_runs_like_scalac(
        "library-trait-bridges-numeric",
        r#"
class Num extends Numeric[Int] {
  def plus(x: Int, y: Int) = x + y; def minus(x: Int, y: Int) = x - y; def times(x: Int, y: Int) = x * y
  def negate(x: Int) = -x; def fromInt(x: Int) = x; def parseString(s: String) = s.toIntOption
  def toInt(x: Int) = x; def toLong(x: Int) = x.toLong; def toFloat(x: Int) = x.toFloat; def toDouble(x: Int) = x.toDouble
  def compare(x: Int, y: Int) = Integer.compare(x, y)
}
class Frac extends Fractional[Double] {
  def plus(x: Double, y: Double) = x + y; def minus(x: Double, y: Double) = x - y; def times(x: Double, y: Double) = x * y
  def div(x: Double, y: Double) = x / y
  def negate(x: Double) = -x; def fromInt(x: Int) = x.toDouble; def parseString(s: String) = s.toDoubleOption
  def toInt(x: Double) = x.toInt; def toLong(x: Double) = x.toLong; def toFloat(x: Double) = x.toFloat; def toDouble(x: Double) = x
  def compare(x: Double, y: Double) = java.lang.Double.compare(x, y)
}
object Main {
  def main(args: Array[String]): Unit = {
    println(List(1, 2, 3).sum(new Num))
    println(List(1, 2, 3).product(new Num))
    println(List(1, 3, 2).max(new Num))
    println(List(1.5, 2.5).sum(new Frac))
  }
}
"#,
    );
}
