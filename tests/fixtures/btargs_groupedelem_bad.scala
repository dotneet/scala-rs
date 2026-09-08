// The restriction on `agent/basetypeargs`. Each of these is rejected by real
// scalac 2.13.16, and each is a way of getting the element or the `CC` of
// `Iterator.GroupedIterator` wrong in the other direction -- accepting the
// receiver's own first type argument as the element, or `Iterator` where the
// declaration really does name something narrower.
object Main {
  // `sliding` is an `Iterator[Seq[A]]`; it is not an `Iterator[A]`.
  def bad1[A](it: Iterator[A]): Iterator[A] = it.sliding(2)

  // Its `map` takes a `Seq[A] => B`, so a function on the element of the
  // *receiver's* argument is not one.
  def bad2[A](it: Iterator[A]): Iterator[A] = it.sliding(2).map((x: A) => x)

  // `Vector.map` still narrows to `Vector`, so it is not a `List`.
  def bad3(xs: Vector[Int]): List[Int] = xs.map(_ + 1)

  def main(args: Array[String]): Unit = ()
}
