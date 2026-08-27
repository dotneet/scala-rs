class Box(val n: Int) extends Ordered[Box] {
  def compare(that: Box): Int = n - that.n
}
class C[A <% Ordered[A]](val x: A)
object Main {
  def main(args: Array[String]): Unit = {
    println(new C(new Box(1)).x.n)
    println(new C(new Box(2)).x.n)
  }
}
