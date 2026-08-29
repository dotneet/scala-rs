object Main {
  def split(n: Int): (Int, String) = (n, "n" + n)
  def f(n: Int): String = n match {
    case 0 =>
      val (a, b) = split(7)
      val c = a * 2
      b + c
    case _ =>
      val Some(v) = Option(n)
      "v" + v
  }
  def main(args: Array[String]): Unit = {
    println(f(0))
    println(f(3))
  }
}
