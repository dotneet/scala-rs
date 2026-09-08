// The legal neighbours of the two shapes `checkEphemeral` rejects.

// A universal trait: only `def`s, plus the members nsc's "OK" list names --
// an import, a type member, and (unlike a value class) a nested class.
trait Greets extends Any {
  import scala.math.abs
  type Self = Greets
  class Helper { def twice(n: Int): Int = n * 2 }
  def name: String
  def greet: String = "hello, " + name + abs(-1)
  def helped: Int = new Helper().twice(21)
}

// A value class: `def`s and a nested *type alias*, with an anonymous class and
// a PartialFunction literal in a method body (scala/bug#7571).
class Meters(val n: Int) extends AnyVal with Greets {
  type Pair = (Int, Int)
  def name: String = "meters"
  def pair: Pair = (n, n)
  def anon: Int = {
    val f = new Function0[Int] { def apply(): Int = n + 1 }
    val pf: PartialFunction[Int, Int] = { case x => x * 10 }
    f() + pf(n)
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val m = new Meters(4)
    println(m.greet)
    println(m.helped)
    println(m.pair)
    println(m.anon)
  }
}
