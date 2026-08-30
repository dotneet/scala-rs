// Every line here is rejected by real scalac 2.13.16 as well.
object Main {
  def main(args: Array[String]): Unit = {
    // The companion object is not an `Ordering`.
    val a: Ordering[Int] = Ordering
    // `Ordering.Option` is a derivation rule, not a view: it must not convert
    // an `Ordering[Int]` into an `Ordering[Option[Int]]`.
    val b: Ordering[Option[Int]] = Ordering.Int
    // 2.13 has no `Ordering.Foo` / `Numeric.Int`.
    val c = Ordering.Foo
    val d = Numeric.Int
    // No instance for a type that has none.
    val f = Ordering[Object]
    println((a, b, c, d, f))
  }
}
