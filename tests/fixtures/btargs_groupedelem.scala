// `agent/basetypeargs`: a collection's element type is the argument it passes
// to `IterableOnce`, and its `CC` is the constructor its own base type names --
// neither of them is "the receiver's first type argument".
//
// `Iterator.sliding` / `Iterator.grouped` return `Iterator.GroupedIterator[B]`,
// an inner class of `trait Iterator` declared
// `extends AbstractIterator[immutable.Seq[B]]`. Its element is `Seq[B]` and its
// `CC` is `Iterator`, so both guesses are wrong for it at once, and every line
// below distinguishes the right answer from them at run time.
//
// `IntMap[T]` and `LongMap[T]` are the same question one step milder: the
// element is `(Int, T)`, not `T`.
//
// The controls are `Vector` and `TreeMap`, whose `map` really does return their
// own class; narrowing must not be lost to fix the two above.
import scala.collection.immutable.{IntMap, LongMap, SortedSet, TreeMap}

object Main {
  // The result of `sliding` is an `Iterator[Seq[A]]`, two hops up its parents.
  def windows[A](it: Iterator[A], n: Int): Iterator[Seq[A]] = it.sliding(n)

  // `withPartial` keeps the `GroupedIterator`, so `map` is selected on it and
  // its function parameter must be `Seq[A] => B` -- not `A => B`.
  def joined[A](it: Iterator[A], n: Int)(f: Seq[A] => String): List[String] =
    it.sliding(n).withPartial(false).map(f).toList

  // And `map`'s *result* is an `Iterator[Int]`, not a `GroupedIterator[Int]`:
  // a `GroupedIterator[Int]`'s elements would be `Seq[Int]`.
  def chunkSizes[A](it: Iterator[A], n: Int): Iterator[Int] =
    it.grouped(n).map(_.size)

  // An `IntMap[T]` is an `IterableOnce[(Int, T)]`.
  def intMapSum(m: IntMap[Int]): Int = {
    var s = 0
    m.foreach(kv => s += kv._1 + kv._2)
    s
  }

  def longMapKeys(m: LongMap[String]): List[Long] = {
    var b = List.empty[Long]
    m.foreach(kv => b = kv._1 :: b)
    b.sorted
  }

  // A nested class that is *not* a collection keeps the lazy path that gives
  // it its parents. `object SortedSet extends SortedIterableFactory.Delegate`,
  // and `Delegate` is nested in `scala.collection.SortedIterableFactory`: a
  // discarded version of this slice attached its pickled parents, decided it
  // was not a collection, took them away again and left it marked done in
  // `PickleSupply::parented`, so the lazy path never ran and
  // `SortedSet.empty(ord)` was `value empty is not a member of SortedSet$`.
  def emptySorted[A](o: Ordering[A]): SortedSet[A] = SortedSet.empty(o)

  // Controls: these two still narrow to the receiver's own class.
  val vec: Vector[Int] = Vector(1, 2, 3).map(_ * 2)
  val tm: TreeMap[Int, String] = TreeMap(1 -> "a", 2 -> "b").map { case (k, v) => (k + 1, v + "!") }

  def main(args: Array[String]): Unit = {
    println(windows(Iterator(1, 2, 3, 4), 2).map(_.mkString("-")).mkString(","))
    println(joined(Iterator('a', 'b', 'c'), 2)(_.mkString))
    println(chunkSizes(Iterator(1, 2, 3, 4, 5), 2).mkString(","))
    println(intMapSum(IntMap(1 -> 10, 2 -> 20)))
    println(longMapKeys(LongMap(3L -> "c", 1L -> "a")))
    println(vec)
    println(tm)
    println(emptySorted(Ordering.Int).size)
  }
}
