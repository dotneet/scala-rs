object Main {
  def f(a: Int)(b: String): String = a.toString + b
  def main(args: Array[String]): Unit = {
    val g: Int => String => String = f
    println(g(1)("x"))
    println(f(2)("y"))
  }
}
