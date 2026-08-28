case class P(x: Int, y: String)
object Main {
  def f: P = P(1, "a").copy(z = 3)
}
