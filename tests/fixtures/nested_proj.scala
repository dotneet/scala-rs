class Outer {
  class Inner {
    type X = Int
    def n: X = 41
  }
  def inner: Inner = new Inner
}
trait A { type T }
class AI extends A { type T = Int }
class Holder { type Inner = AI }
object Main {
  def fromClass(x: Outer#Inner#X): Int = x
  def fromAlias(x: Holder#Inner#T): Int = x
  def main(args: Array[String]): Unit = {
    println(fromClass(new Outer().inner.n))
    println(fromAlias(2))
  }
}
