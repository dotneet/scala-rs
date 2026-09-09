case class Result(n: Int)
object Main {
  def main(args: Array[String]): Unit = {
    val m = Map("a" -> 1)
    val xs = m.collect { case (_, value) => Result(value + 1) }
    println(xs.head.n)
  }
}
