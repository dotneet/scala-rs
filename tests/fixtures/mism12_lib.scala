// The same two causes against the real `scala-library`, plus the companion
// `apply` overloads `scala.math.BigDecimal` really declares.
//
//  * `traverse[A, B, M[+X] <: IterableOnce[X]]` is slick's
//    `DBIOAction.traverse`. `in.iterator` on an `M[A]` has to be an
//    `Iterator[A]`: the bound is written in `M`'s own parameter, and until it
//    is replaced the element is `IterableOnce`'s `A`, which prints the same
//    and is a different symbol.
//  * `BigDecimal.apply` eta-expanded at `Double => BigDecimal`: only the
//    `(Int)`, `(String)` and `(java.math.BigDecimal)` alternatives existed, so
//    slick's `new ScalaNumericType[BigDecimal](BigDecimal.apply)` had nothing
//    to pick.

import scala.collection.Factory

class NumericType[T](val fromDouble: Double => T)

object Main {
  def traverse[A, B, M[+X] <: IterableOnce[X]](in: M[A])(f: A => B)(implicit
      cbf: Factory[B, M[B]]
  ): M[B] =
    in.iterator
      .foldLeft(cbf.newBuilder) { (builder, a) => builder += f(a) }
      .result()

  def firstLength[A, M[+X] <: Iterable[X]](in: M[A]): Int =
    in.foldLeft(0)((acc, a) => acc + a.toString.length)

  def main(args: Array[String]): Unit = {
    println(traverse(List(1, 2, 3))(i => i * 2))
    println(traverse(Vector("a", "bb"))(s => s.length))
    println(firstLength(List(10, 200)))

    val bd = new NumericType[BigDecimal](BigDecimal.apply)
    println(bd.fromDouble(1.5))
    println(BigDecimal(7L))
    println(BigDecimal(12L, 2))
    println(BigDecimal(BigInt(9)))
  }
}
