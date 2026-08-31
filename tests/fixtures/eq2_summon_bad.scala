// Every line here is rejected by real scalac 2.13.16 as well.
object Main {
  def main(args: Array[String]): Unit = {
    // No `PartialOrdering[Int]` instance exists (unlike `Equiv[Int]` /
    // `Ordering[Int]`); the hierarchy edge must not manufacture one.
    val a = implicitly[PartialOrdering[Int]]
    // Subtyping runs one way only: an `Equiv[Int]` is not an `Ordering[Int]`.
    val b: Ordering[Int] = implicitly[Equiv[Int]]
    // The companion object is not an `Equiv`.
    val c: Equiv[Int] = Equiv
    println((a, b, c))
  }
}
