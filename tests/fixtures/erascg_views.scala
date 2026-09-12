// A lambda body converted by a view under a function prototype, and the
// function type an eta-expanded repeated parameter gets.
object Main {
  def f[T](xs: T*): T = xs.head
  def g[T] = f[T] _
  def sum(xs: Int*): Int = xs.sum
  def cat(sep: String)(xs: String*): String = xs.mkString(sep)

  def main(args: Array[String]): Unit = {
    // `flatMap[B](f: A => IterableOnce[B])` with an `Array` result: the body
    // is wrapped (`wrapIntArray` / `genericWrapArray`), not cast.
    println(Seq(1, 2).flatMap(x => Array(x, x)))
    println(List("a", "b").flatMap(s => Array(s, s + s)))
    println(Vector(1).flatMap(x => Array(x.toString)))
    println(Seq(1, 2).flatMap { x => if (x > 1) Array(x) else Array[Int]() })
    println(Option(3).toList.flatMap(x => Array(x * 2)))
    println(for (x <- List(1, 2); y <- Array(x, 10 * x)) yield y)
    val fn: Int => IterableOnce[Int] = x => Array(x, x + 1)
    println(List(5).flatMap(fn))

    // Eta-expansion of a repeated parameter is a function of one `Seq`.
    println(g("hello" +: args.toIndexedSeq))
    val h = sum _
    println(h(List(1, 2, 3)))
    val h2: Seq[Int] => Int = sum _
    println(h2(Vector(4, 5)))
    println(List(Seq(1, 2), Seq(3)).map(sum _))
    println(List(Seq(1, 2), Seq(3)).map(sum))
    val m: Seq[Int] => Int = sum
    println(m(Seq(7, 8)))
    val c = cat _
    println(c("-")(List("x", "y")))
    val c2 = cat("+") _
    println(c2(Seq("p", "q")))
  }
}
