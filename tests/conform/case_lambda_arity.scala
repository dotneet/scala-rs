object Main {
  def apply2(g: (Int, String) => String): String = g(7, "n")
  def apply3(g: (Int, Int, Int) => Int): Int = g(1, 2, 3)
  def opt(g: (Option[Int], String) => String): String = g(Some(4), "v")
  def main(args: Array[String]): Unit = {
    println(apply2 { case (n, s) => s + n })
    println(apply3 { case (a, b, c) => a + b + c })
    println(opt { case (Some(n), s) => s + n; case (None, s) => s })
    val f: (Int, Int) => Int = { case (a, b) => a * b }
    println(f(3, 4))
  }
}
