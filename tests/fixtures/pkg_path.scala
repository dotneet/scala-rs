object Main {
  def main(args: Array[String]): Unit = {
    val t = scala.util.Try(1 + 1)
    println(t.get)
    val e: scala.util.Either[String, Int] = scala.util.Right(3)
    println(e.getOrElse(0))
    val xs = scala.collection.immutable.List(1, 2)
    println(xs.sum)
  }
}
