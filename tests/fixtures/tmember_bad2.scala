// An unresolved name applied to type arguments is a missing type, not a kind
// error: nsc reports `not found: type Missing`.
object Main {
  def f(x: Missing[Int]): Int = 0
  def main(args: Array[String]): Unit = println(0)
}
