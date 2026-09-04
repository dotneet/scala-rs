// Filling a function-typed implicit parameter from an implicit def (the third
// agent/durrange case). Along the path that eta-expands and passes
// scala.math.Ordered.orderingToOrdered; a view bound (`A <% Ordered[A]`) takes the
// same path. Ordered$ and Ordering exist only in the jar, so --scala-library only.
object Main {
  def h[A](x: A, y: A)(implicit ev: A => Ordered[A]): A = if (x < y) y else x
  def g[A <% Ordered[A]](x: A, y: A): A = if (x < y) y else x
  // A nested implicit parameter. The view has to be found here too.
  def top[A](xs: List[A])(implicit ev: A => Ordered[A]): A = xs.reduceLeft(h(_, _))

  def main(args: Array[String]): Unit = {
    println(h(3, 5))
    println(h("a", "b"))
    println(h(2.5d, 1.5d))
    println(g(3, 5))
    println(g("a", "b"))
    println(g(7L, 2L))
    println(top(List(4, 9, 2)))
    println(top(List("b", "z", "a")))
    // A bare conversion (implicit def to Ordered) is found by the same search.
    val oi: Ordered[Int] = 3
    println(oi < 4)
    val os: Ordered[String] = "abc"
    println(os > "abd")
  }
}
