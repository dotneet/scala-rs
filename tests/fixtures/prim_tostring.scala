object Main {
  def f(x: Int): String = x.toString
  def main(args: Array[String]): Unit = {
    println(f(3))
    val g = (x: Int) => x.toString
    println(g(4))
    println(1.toString + 2L.toString + 'c'.toString + true.toString)
  }
}
