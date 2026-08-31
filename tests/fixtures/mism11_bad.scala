// The three fixes must not accept what is really wrong.
//
//  * A grouped iterator's element is a `Seq[B]`: a lambda that takes the
//    element type of the *source* is a mismatch, not something the receiver's
//    first type argument may override into place.
//  * An `ArrayBuilder[Int]` is a `Builder[Int, Array[Int]]` and nothing else:
//    `To` is invariant.
//  * A type constructor left open in an argument's expected type says nothing
//    about that position and everything about the others.

import scala.collection.mutable

class Box2[A](val a: A)

class Qry2[E, C[_]](val value: E) {
  def flatMap[F, D[_]](f: E => Qry2[F, D]): Qry2[F, C] =
    new Qry2[F, C](f(value).value)
}

object Main {
  def main(args: Array[String]): Unit = {
    val g = Seq(1, 2, 3, 4).iterator.grouped(2)
    println(g.map((i: Int) => i + 1).toList)

    val b: mutable.Builder[Int, Array[String]] = mutable.ArrayBuilder.make[Int]
    println(b)

    val q = new Qry2[Int, Box2](1)
    val bad = q.flatMap[String, Box2](v => new Qry2[Int, Box2](v))
    println(bad)
  }
}
