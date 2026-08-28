// nsc: "positional after named argument." — a named argument that moves an
// argument out of its position makes every later positional argument illegal.
// (`g(a = 1, 2, 3)` stays legal, since `a` is already at position 1.)
object Main {
  def g(a: Int, b: Int, c: Int): Int = a + b + c
  def main(args: Array[String]): Unit = {
    println(g(b = 1, 2, 3))
  }
}
