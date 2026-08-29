object Main {
  def pair(g: ((Int, String)) => String): String = g((1, "a"))
  def two(g: (Int, String) => String): String = g(2, "b")
  def one(g: (Int) => Int): Int = g(3)
  def none(g: () => Int): Int = g()
  def main(args: Array[String]): Unit = {
    println(pair(t => t._2 + t._1))
    println(pair { case (n, s) => s + n })
    println(two((n, s) => s + n))
    println(one(_ + 1))
    println(none(() => 9))
    val f: Tuple2[Int, String] => String = t => t._2
    println(f((5, "z")))
  }
}
