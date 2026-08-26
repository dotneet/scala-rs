object Main {
  def f(x: Int, y: Any): String = "a"
  def f(x: Any, y: Int): String = "b"
  def main(args: Array[String]): Unit = {
    println(f(1, 1))
  }
}
