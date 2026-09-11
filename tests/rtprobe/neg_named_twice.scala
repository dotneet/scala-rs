// scalac: parameter 'a' is already specified at parameter position 1.
object Main {
  def f(a: Int, b: Int = 0): Int = a + b
  def main(args: Array[String]): Unit = println(f(1, a = 2))
}
