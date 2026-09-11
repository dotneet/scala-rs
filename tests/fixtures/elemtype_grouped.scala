// `Iterator.sliding` returns `GroupedIterator[B]`, whose elements are
// `Seq[B]`, not `B`. A function value passed to its `map` must be checked
// against the element type derived from the `IterableOnce` base type.
object Main {
  def slidingN[A, B](xs: List[A], n: Int)(f: Seq[A] => B): List[B] =
    xs.iterator.sliding(n).withPartial(false).map(f).toList

  def pairs[A](xs: List[A]): List[(A, A)] = slidingN(xs, 2)(x => (x(0), x(1)))

  def main(args: Array[String]): Unit = {
    println(pairs(List(1, 2, 3, 4)))
    val sums: Seq[Int] => Int = _.sum
    println(List(1, 2, 3, 4).iterator.grouped(2).map(sums).toList)
    println(List(1, 2, 3).iterator.sliding(2).map(_.length).toList)
    println(Vector("a", "b").map(_.length))
  }
}
