object Main {
  def f(x: Int): String = "int"
  def f(x: String): String = "str"
  def g(x: Int): String = "1"
  def g(x: Int, y: Int): String = "2"
  def main(args: Array[String]): Unit = {
    println(f(1))
    println(f("a"))
    println(g(1))
    println(g(1, 2))
  }
}
