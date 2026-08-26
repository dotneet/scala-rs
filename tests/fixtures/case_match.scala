case class Point(x: Int, y: Int)

object Main {
  def main(args: Array[String]): Unit = {
    val p = Point(3, 4)
    println(p match {
      case Point(a, b) => a + b
    })
  }
}
