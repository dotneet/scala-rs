// The half that says the rule in `cfw_forwarder.scala` is a restriction and
// not a licence.
//
// Dropping the class file's copy of a declaration must leave the declaration
// itself in force -- including the two things the copy could not say, which
// are exactly what a caller could otherwise get away with. `bad2` is the one
// that matters most: the flattened forwarder *accepted* it, so the pre-fix
// binary compiled a program real scalac rejects.
//
// Real scalac 2.13.16 rejects both of these, at lines 25 and 29.

final class Bag[A](xs: List[A]) extends scala.collection.AbstractIterable[A] {
  def iterator: Iterator[A] = xs.iterator
}

object Main {
  // As in the positive fixture: reaching the members through the trait puts
  // `IterableOnceOps`' own declarations next to `Bag`'s inherited forwarders.
  def viaTrait[A](xs: scala.collection.Iterable[A])(f: (A, A) => A): A = xs.reduceLeft(f)
  def viaTraitFold[A, B](xs: scala.collection.Iterable[A])(z: B)(f: (B, A) => B): B =
    xs.foldLeft(z)(f)

  // `reduceLeft[B >: A](op: (B, A) => B)`: `B` is above the element type, so
  // a `String` accumulator over `Int`s is not one.
  def bad1(xs: Bag[Int]): String = xs.reduceLeft((s: String, i: Int) => s)

  // `foldLeft[B](z: B)(op: (B, A) => B)` really does have two clauses; the
  // class file's one-list rendering of it is not an alternative.
  def bad2(xs: Bag[Int]): Int = xs.foldLeft(0, (a: Int, b: Int) => a + b)

  def main(args: Array[String]): Unit = println(bad1(new Bag(List(1))))
}
