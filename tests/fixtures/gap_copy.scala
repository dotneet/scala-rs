case class Point(x: Int, y: Int)

object Main {
  def main(args: Array[String]): Unit = {
    val p = Point(1, 2)
    val q = p.copy(3)
    println(q.x + "," + q.y)
    val r = p.copy(y = 9)
    println(r.x + "," + r.y)
    val s = p.copy(x = 5, y = 6)
    println(s.x + "," + s.y)
    val t = p.copy()
    println(t.x + "," + t.y)
    val u = p.copy(x = 7, y = 8)
    println(u.x + "," + u.y)
  }
}
