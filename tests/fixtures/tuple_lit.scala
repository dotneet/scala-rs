object Main {
  def swap(p: (Int, String)): (String, Int) = (p._2, p._1)
  def main(args: Array[String]): Unit = {
    val p = (1, "x")
    println(p._1)
    println(p._2)
    val (a, b) = p
    println(a + b)
    val q = swap((7, "s"))
    println(q._1 + q._2)
    val pairs = (1, 2) :: (3, 4) :: Nil
    pairs match {
      case (m, n) :: _ => println(m + n)
      case Nil => println(0)
    }
  }
}
