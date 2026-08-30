// `Null` conforms to no value type, so `case null` against a primitive
// scrutinee is a type mismatch -- not a case that is silently never taken.
object Main {
  def f(x: Int): String = x match { case null => "n"; case _ => "o" }
  def g(x: Double): String = x match { case null => "n"; case _ => "o" }
  def main(args: Array[String]): Unit = println(f(1) + g(1.0))
}
