// `Equiv[T]` summon (`implicitly[Equiv[Int]]`) and the real ABI hierarchy
// `Ordering[T] <: PartialOrdering[T] <: Equiv[T]`.
object Main {
  def main(args: Array[String]): Unit = {
    println(implicitly[Equiv[Int]].equiv(1, 1))
    println(implicitly[Equiv[Int]].equiv(1, 2))
    println(implicitly[Equiv[String]].equiv("a", "a"))
    println(implicitly[Equiv[Long]].equiv(3L, 3L))
    println(implicitly[Equiv[Boolean]].equiv(true, false))
    println(implicitly[Equiv[BigInt]].equiv(BigInt(2), BigInt(2)))
    println(Equiv.Int.equiv(5, 5))
    // The instance the summoner hands back is the one real scalac picks.
    println(implicitly[Equiv[Int]].getClass.getName)
    println(implicitly[Equiv[Double]].getClass.getName)
    // `Ordering[T] <: PartialOrdering[T] <: Equiv[T]`: a degrading
    // assignment, not a summon -- these use the `Ordering.Int` instance.
    val e: Equiv[Int] = Ordering.Int
    val p: PartialOrdering[Int] = Ordering.Int
    println(e.equiv(3, 3))
    println(p.lteq(1, 2))
    println(p.lteq(2, 1))
  }
}
