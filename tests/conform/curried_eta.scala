import scala.annotation.tailrec
object Main {
  @tailrec def loop(n: Int, acc: Int): Int = if (n == 0) acc else loop(n - 1, acc + n)
  def curry(a: Int)(b: Int)(c: Int): Int = a + b + c
  def varargs(xs: Int*): Int = xs.sum
  def byname(x: => Int): Int = x + x
  def main(a: Array[String]): Unit = {
    println(loop(5, 0))
    println(curry(1)(2)(3))
    val f = curry(1) _
    println(f(2)(3))
    println(varargs(1,2,3))
    println(varargs(List(1,2): _*))
    var n = 0
    println(byname({ n += 1; n }))
  }
}
