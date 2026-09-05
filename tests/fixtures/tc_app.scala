// Compiled by real scalac 2.13.16 against scala-rs's `tc_lib.scala` output
// (and, as the control, against scalac's own). Both runs must print the same
// thing, which is what "our traits have nsc's ABI" actually means.
import tclib._

class Sub extends Greeter with Sized {
  def name = "world"
  def size = 3
  override def greet: String = "<" + super.greet + ">"
}

object Main {
  def main(args: Array[String]): Unit = {
    val s = new Sub
    println(s.greet)
    println(s.counted)
    println(s.counted)
    println(s.mapped)
    println(s.describe)
    val g: Greeter = s
    println(g.greeting + "/" + g.seen)
  }
}
