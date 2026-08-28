case class P(x: Int, y: String)
object Main {
  def main(args: Array[String]): Unit = {
    val h :: t = 1 :: 2 :: Nil
    println(h)
    val Some(v) = Some(9)
    println(v)
    val P(a, b) = P(3, "z")
    println(a + b)
  }
}
