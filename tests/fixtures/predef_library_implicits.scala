// Implicits of the library's `Predef` that scala-rs's hand-written prelude
// does not declare: the deprecated tuple `zipped` conversions and the implicit
// classes `Ensuring` and `StringFormat`. `->` is declared by both and must stay
// one conversion.
object Main {
  def main(args: Array[String]): Unit = {
    println((List(1, 2), List("a", "b")).zipped.toList)
    println((List(1, 2), List("a", "b"), List(1.0, 2.0)).zipped.toList)
    println((List(1, 2), Vector(3, 4)).zipped.map(_ + _))
    println(5.ensuring(_ > 0))
    println(3.14159.formatted("%.2f"))
    println(1 -> "one")
  }
}
