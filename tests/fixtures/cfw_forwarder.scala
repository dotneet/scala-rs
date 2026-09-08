// Which *declaration* of a library member a receiver gets.
//
// A concrete class carries a mixin forwarder for every default method it
// inherits from a trait, and scalac writes a `Signature` for it -- but the
// JVM's signature language cannot say `[B >: A]` and has only one argument
// list. So `scala.collection.AbstractIterable`'s copy of
// `IterableOnceOps.reduceLeft[B >: A](op: (B, A) => B): B` is
// `<B> B reduceLeft(Function2<B, A, B>)` and its copy of
// `foldLeft[B](z: B)(op: (B, A) => B): B` is `foldLeft(Object, Function2)`.
// Both copies were installed, overload resolution had two alternatives where
// nsc has one, and picking the forwarder left `B` with nothing but `Any` to
// be.
//
// Every line below is checked by what it prints: the folds need `B` to be the
// caller's own type, which only the declaration's shape allows.

final class Bag[A](xs: List[A]) extends scala.collection.AbstractIterable[A] {
  def iterator: Iterator[A] = xs.iterator
}

object Main {
  // Reached through the trait, so `IterableOnceOps`' own declarations are in
  // the symbol table and stand next to the forwarders `Bag` inherits.
  def viaTrait[A](xs: scala.collection.Iterable[A])(f: (A, A) => A): A = xs.reduceLeft(f)
  def viaTraitFold[A, B](xs: scala.collection.Iterable[A])(z: B)(f: (B, A) => B): B =
    xs.foldLeft(z)(f)
  def viaTraitOpt[A](xs: scala.collection.Iterable[A])(f: (A, A) => A): Option[A] =
    xs.reduceLeftOption(f)

  // `B >: A` -- the lower bound the class file drops.
  def reduce[A](xs: Bag[A])(f: (A, A) => A): A = xs.reduceLeft(f)
  def reduceOpt[A](xs: Bag[A])(f: (A, A) => A): Option[A] = xs.reduceLeftOption(f)
  // Two parameter clauses -- the second the class file merges into the first.
  def fold[A, B](xs: Bag[A])(z: B)(f: (B, A) => B): B = xs.foldLeft(z)(f)

  def main(args: Array[String]): Unit = {
    println(viaTrait(List(1, 2, 3))(_ + _))
    println(viaTraitFold(List("a", "b"))("")(_ + _))
    println(viaTraitOpt(List(4, 5))(_ + _))

    println(reduce(new Bag(List(1, 2, 3)))(_ + _))
    println(reduceOpt(new Bag(List(1, 2, 3)))(_ + _))
    // `z` is a `String` and the elements are `Int`s, so the second clause has
    // to be typed at `(String, Int) => String`.
    println(fold(new Bag(List(1, 2, 3)))("")((s, i) => s + i))
  }
}
