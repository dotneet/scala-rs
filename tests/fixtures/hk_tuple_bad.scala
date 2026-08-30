// Auto-tupling only ever goes one way. None of these may be accepted:
//  - `g((1, 2))` is not un-tupled into `g(1, 2)`;
//  - `one(1, 2)` has a parameter that is not a tuple;
//  - `zero(1, 2)` takes no arguments at all;
//  - `c(1, "x")` has an alternative of the written arity, so scalac reports
//    the type mismatch inside it rather than tupling into `c((1, "x"))`.
object Main {
  def g(a: Int, b: Int): Int = a + b
  def one(x: Int): Int = x
  def zero(): Int = 0
  def c(x: String, y: String): String = "cSS"
  def c(t: (Int, String)): String = "cTup"
  def main(args: Array[String]): Unit = {
    println(g((1, 2)))
    println(one(1, 2))
    println(zero(1, 2))
    println(c(1, "x"))
  }
}
