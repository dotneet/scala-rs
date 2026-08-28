class Base(x: Int)(y: Int) { def sum: Int = x + y }
class Derived extends Base(1)(2)
trait Marker
class Tagged extends Base(3)(4) with Marker
object Main {
  def kind(x: Any): String = x match {
    case _: Int | _: String => "prim"
    case (_: Marker | _: Base) => "ours"
    case _ => "other"
  }
  def main(args: Array[String]): Unit = {
    println(new Derived().sum)
    println(new Tagged().sum)
    println(kind(1)); println(kind("s")); println(kind(new Tagged)); println(kind(1.0))
  }
}
