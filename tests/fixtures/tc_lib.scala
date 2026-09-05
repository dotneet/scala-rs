// The half of the interop check that scala-rs compiles: traits only.
// `tc_app.scala` is compiled against the class files this produces by *real*
// scalac 2.13.16, and the two are run together. See
// `crates/cli/tests/traitclass.rs`.
package tclib

trait Greeter {
  val greeting: String = "hello"
  var seen: Int = 0
  private def punct: String = "!"
  def name: String
  def greet: String = greeting + ", " + name + punct
  def counted: String = { seen = seen + 1; greet + seen }
  def mapped: String = List(1, 2, 3).map(_ + name.length).mkString(",")
}

trait Sized {
  def size: Int
  def describe: String = "size=" + size
}
