class Outer {
  class Inner {
    type X = Int
    def n: X = 41
  }
}
trait A { type T }
class AI extends A { type T = Int }
class Holder { type Inner = AI }
object Main {
  def fromClass(x: Outer#Inner#X): Int = x
  def fromAlias(x: Holder#Inner#T): Int = x
  def main(args: Array[String]): Unit = {
    val o = new Outer
    val i = new o.Inner
    println(fromClass(i.n))
    println(fromAlias(2))
  }
}
