// `Integral` / `Fractional` are part of the `Numeric` type-class hierarchy:
// `trait Integral[T] extends Numeric[T]`, `trait Fractional[T] extends
// Numeric[T]`, `trait Numeric[T] extends Ordering[T]` (javap on the real
// scala-library 2.13.16 jar). `object Numeric`'s implicit instances are
// declared at the *sub*-class: `IntIsIntegral: Integral[Int]`, not
// `Numeric[Int]`.
object Main {
  def widenToNumeric(x: Integral[Int]): Numeric[Int] = x
  def widenToOrdering(x: Integral[Int]): Ordering[Int] = x
  def widenFractional(x: Fractional[Double]): Numeric[Double] = x
  def widenNumeric(x: Numeric[Int]): Ordering[Int] = x

  def sumWith[T](xs: List[T])(implicit n: Numeric[T]): T =
    xs.foldLeft(n.zero)((a, b) => n.plus(a, b))

  def quotient[T](x: T, y: T)(implicit i: Integral[T]): T = i.quot(x, y)
  def remainder[T](x: T, y: T)(implicit i: Integral[T]): T = i.rem(x, y)
  def ratio[T](x: T, y: T)(implicit f: Fractional[T]): T = f.div(x, y)

  def main(args: Array[String]): Unit = {
    // The reported gap: `IterableFactory#range` takes an `Integral[A]`.
    println(List.range(0, 5))
    println(List.range(0, 10, 3))
    println(Vector.range(0, 3))
    println(Seq.range(0, 3))
    println(List.range(0L, 4L))

    // `implicitly` picks exactly what real scalac picks.
    println(implicitly[Numeric[Int]].getClass.getName)
    println(implicitly[Ordering[Int]].getClass.getName)
    println(implicitly[Integral[Int]].getClass.getName)
    println(implicitly[Fractional[Double]].getClass.getName)
    println(implicitly[Numeric[Double]].getClass.getName)
    println(implicitly[Numeric[Float]].getClass.getName)
    println(implicitly[Numeric[BigDecimal]].getClass.getName)
    println(implicitly[Numeric[BigInt]].getClass.getName)
    println(implicitly[Numeric[Long]].getClass.getName)
    println(implicitly[Numeric[Byte]].getClass.getName)
    println(implicitly[Numeric[Short]].getClass.getName)
    println(implicitly[Numeric[Char]].getClass.getName)
    println(implicitly[Fractional[Float]].getClass.getName)
    println(implicitly[Integral[Char]].toInt('a'))
    println(implicitly[Numeric[Byte]].toInt(3.toByte))
    println(implicitly[Numeric[Short]].toInt(3.toShort))

    // `quot` / `rem` / `div` come off the pickled `Integral` / `Fractional`.
    println(quotient(7, 2))
    println(remainder(7, 2))
    println(ratio(7.0, 2.0))
    println(implicitly[Integral[Long]].quot(9L, 2L))
    println(implicitly[Fractional[Float]].div(9.0f, 2.0f))

    // Numeric-driven user code and the `sum` / `product` / `sorted` family
    // must all keep working (one candidate each, no ambiguity).
    println(sumWith(List(1, 2, 3)))
    println(sumWith(List(1.5, 2.5)))
    println(List(1, 2, 3).sum)
    println(List(1, 2, 3).product)
    println(List(1.5, 2.5).sum)
    println(List(3, 1, 2).sorted)
    println(List(3, 1, 2).max)
    println(List(3, 1, 2).min)
    println(List(3, 1, 2).sortBy(x => -x))
    println(List("b", "a").sorted)
    println(widenToOrdering(implicitly[Integral[Int]]).compare(1, 2))
    println(widenToNumeric(implicitly[Integral[Int]]).plus(1, 2))
    println(widenFractional(implicitly[Fractional[Double]]).plus(1.0, 2.0))
    println(widenNumeric(implicitly[Numeric[Int]]).compare(2, 1))

    // `Ordering.Option` was the other prelude hole in this corner.
    println(implicitly[Ordering[Option[Int]]].compare(Some(1), None))
    println(List(Some(2), None, Some(1)).sorted)
  }
}
