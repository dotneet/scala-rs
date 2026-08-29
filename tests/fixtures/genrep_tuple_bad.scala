// Packing an argument list into a tuple is a last resort, not a way to make
// a wrong call type-check: `(Int, Int)` is not an `Int`, and scalac reports
// the too-many-arguments error.
object Main {
  def f(x: Int): Int = x
  def main(args: Array[String]): Unit = println(f(1, 2))
}
