import scala.language.implicitConversions
object Main {
  class Carb[A](val marker: Int)
  implicit def narrow[N, M[_], A](x: Carb[M[A]])(implicit ev: N <:< M[A]): Carb[N] = new Carb[N](x.marker + 1)
  def bar[M[_], A]: Carb[M[A]] = new Carb[M[A]](7)
  val x: List[Int] = List(1)
  val y: List[Int] = List(2)
  type X = x.type
  type Y = y.type
  def main(args: Array[String]): Unit = {
    val a: Carb[List[Int]] = bar
    val b: Carb[X] = bar
    val c: Carb[Y] = bar
    println(a.marker)
    println(b.marker)
    println(c.marker)
  }
}
