// A type variable bounded inside a constructor joins its bounds by the plain
// lub, and only one that *is* the parameter type joins numeric bounds by weak
// lub: `Map(1 -> 2, 3 -> 4.5)` is a `Map[Int, AnyVal]` (scala-rs joined `V`
// to `Double`, found `(1, 2)` inapplicable, and printed a `BuildFrom`
// builder), while `List(1, 2.5)` is a `List[Double]`.
object Main {
  def f[K, V](xs: (K, V)*): String = xs.toString
  def g[A](xs: A*): String = xs.toString
  def main(args: Array[String]): Unit = {
    val m = Map(1 -> 2, 3 -> 4.5)
    println(m + " " + m(3) + " " + m(1))
    println(Map((1, 2), (3, 4.5)))
    println(f((1, 2), (3, 4.5)) + " " + f((1, "a"), (3, 4)))
    println(g(1, 2.5) + " " + g(1, 2L) + " " + List(1, 2.5, 3L) + " " + List('a', 1))
    println(List((1, 2), (3, 4.5)) + " " + Vector(Some(1), Some(2.5)))
  }
}
