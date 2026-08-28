case class P(x: Int, y: String)
object Main {
  def main(args: Array[String]): Unit = {
    val p = P(1, "a")
    val q = p.copy(y = "b")
    println(q.x + q.y)
    val r = p.copy(2)
    println(r.x + r.y)
    println(p.copy().x)
  }
}
