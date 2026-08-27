class Outer {
  class Inner {
    def n: Int = 1
  }
  def inner: Inner = new Inner()
}
object Main {
  def take(x: p.Inner forSome { val p: Outer }): Int = x.n
  def main(args: Array[String]): Unit = {
    println(take(new Outer().inner))
  }
}
