// `Ordering` in *term* position is the companion object, and `Ordering[T]` is
// its summoner `apply[T](implicit ord: Ordering[T])`.
object Main {
  def main(args: Array[String]): Unit = {
    println(Ordering.Int.reverse.compare(1, 2))
    println(Ordering[String].compare("a", "b"))
    println(Ordering[Int].reverse.compare(1, 2))
    println(Ordering.String.reverse.compare("a", "b"))
    println(implicitly[Ordering[Int]].reverse.compare(1, 2))
    println(List("b", "a", "c").sorted(Ordering[String].reverse))
    println(Ordering.by[(String, Int), Int](_._2).compare(("a", 2), ("b", 1)))
    println(Numeric[Int].plus(2, 3))
    println(Numeric.IntIsIntegral.times(2, 3))
    println(Integral[Int].quot(7, 2))
    println(Fractional[Double].div(1.0, 4.0))
    println(BigInt(6) * BigInt(7))
    // The instance the summoner hands back is the one real scalac picks.
    println(Ordering[Int].getClass.getName)
    // The derivation rule stays a derivation rule.
    println(List(Some(2), None, Some(1)).sorted)
  }
}
