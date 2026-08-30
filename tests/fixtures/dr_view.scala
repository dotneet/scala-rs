// 関数型の implicit パラメータを implicit def から埋める（agent/durrange の
// 3 件目）。scala.math.Ordered.orderingToOrdered を eta 展開して渡す経路で、
// view bound（`A <% Ordered[A]`）も同じ経路を通る。
// Ordered$ と Ordering は jar にしかないので --scala-library 専用。
object Main {
  def h[A](x: A, y: A)(implicit ev: A => Ordered[A]): A = if (x < y) y else x
  def g[A <% Ordered[A]](x: A, y: A): A = if (x < y) y else x
  // 入れ子の implicit パラメータ。view はここでも見つかる必要がある。
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
    // 素の変換（implicit def から Ordered へ）も同じ探索でつく。
    val oi: Ordered[Int] = 3
    println(oi < 4)
    val os: Ordered[String] = "abc"
    println(os > "abd")
  }
}
