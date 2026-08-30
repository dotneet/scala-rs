// Constant patterns whose scrutinee is not an `int`.
//
// A `Long` / `Float` / `Double` scrutinee had both operands popped and the
// case taken unconditionally -- `case 1L =>` matched every `Long` -- and a
// primitive constant against a reference scrutinee pushed a raw `int` where
// the verifier wanted an `Object`.
object Main {
  def lng(x: Long): String = x match { case 1L => "one"; case _ => "o" }
  def dbl(x: Double): String = x match { case 1.5 => "1.5"; case _ => "o" }
  def flt(x: Float): String = x match { case 1.5f => "1.5f"; case _ => "o" }
  def chr(x: Char): String = x match { case 'a' => "a"; case _ => "o" }
  // a constant against a *reference* scrutinee: the constant is boxed
  def boxed(x: Any): String = x match {
    case 1 => "int1"
    case 2L => "long2"
    case 1.5 => "d1.5"
    case 'c' => "charc"
    case true => "bool"
    case _ => "o"
  }
  // a constant sub-pattern inside a constructor pattern
  case class P(a: Int, b: Long)
  def sub(p: P): String = p match {
    case P(1, 2L) => "one-two"
    case P(_, 2L) => "any-two"
    case _ => "o"
  }
  def main(args: Array[String]): Unit = {
    println(lng(1L)); println(lng(2L))
    println(dbl(1.5)); println(dbl(2.5))
    println(flt(1.5f)); println(flt(2.5f))
    println(chr('a')); println(chr('b'))
    println(boxed(1)); println(boxed(2L)); println(boxed(1.5))
    println(boxed('c')); println(boxed(true)); println(boxed("x"))
    println(sub(P(1, 2L))); println(sub(P(3, 2L))); println(sub(P(3, 4L)))
  }
}
