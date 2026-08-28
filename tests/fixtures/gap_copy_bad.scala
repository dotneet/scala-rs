case class Point(x: Int, y: Int)

object Main {
  def main(args: Array[String]): Unit = {
    val p = Point(1, 2)
    println(p.copy(z = 1))
  }
}
