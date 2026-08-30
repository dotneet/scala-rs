// The hierarchy must not become a rubber stamp. `Integral[Double]` and
// `Fractional[Int]` have no instance in the real library either, and
// `Numeric[T] <: Ordering[T]` must not be read backwards.
object Main {
  def narrow(x: Numeric[Int]): Integral[Int] = x
  def wrongWay(x: Ordering[Int]): Numeric[Int] = x

  def main(args: Array[String]): Unit = {
    println(implicitly[Integral[Double]])
    println(implicitly[Fractional[Int]])
    println(implicitly[Integral[String]])
    println(List.range("a", "z"))
  }
}
