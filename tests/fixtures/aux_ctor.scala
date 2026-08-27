class C(val x: Int, val y: Int) {
  def this(x: Int) = this(x, 0)
  def sum: Int = x + y
}
class D extends C(1)
class E(z: Int) extends C(z)
object Main {
  def main(args: Array[String]): Unit = {
    println(new C(3, 4).sum)
    println(new C(5).sum)
    println(new D().sum)
    println(new E(9).sum)
  }
}
