// nsc: "parameter 'c' is already specified at parameter position 2"
object Main {
  def g(a: Int, b: Int, c: Int): Int = a + b + c
  def main(args: Array[String]): Unit = {
    println(g(a = 1, c = 3, c = 4))
  }
}
