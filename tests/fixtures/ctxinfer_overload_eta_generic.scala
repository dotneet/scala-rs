trait Base[A] {
  def f[B >: A](x: B): Int
  def f[B >: A](x: B, n: Int): Int
}
final class Concrete extends Base[String] {
  def f[B >: String](x: B): Int = 7
  def f[B >: String](x: B, n: Int): Int = n
}
object Main {
  def use(base: Base[String]): String => Int = base.f
  def main(args: Array[String]): Unit = {
    val order: Seq[String] = Seq("b", "a")
    val sorted = Vector("a", "b").sortBy(order.indexOf)
    println(sorted.mkString(","))
    println(use(new Concrete)("value"))
  }
}
