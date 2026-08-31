// Every line here is rejected by real scalac 2.13.16 as well: `Ordering[T]`
// is generic in `T`, not `Any`.
object Main {
  def main(args: Array[String]): Unit = {
    println(Ordering[String].compare(1, 2))
    println(Ordering[Int].compare("a", "b"))
    println(Ordering[String].lt(1, 2))
    println(Ordering[String].max(1, 2))
  }
}
