import scala.math.Ordering
object Main {
  def cmp[T: Ordering](x: T, y: T): Int = implicitly[Ordering[T]].compare(x, y)
  def g[T: scala.reflect.ClassTag](xs: Array[T]): Int = xs.length
  def main(args: Array[String]): Unit = {
    println(cmp(3, 1))
    println(cmp(2, 2))
    println(g(new Array[Int](3)))
  }
}
