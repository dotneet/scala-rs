// `-Ykind-projector` must not change a program that does not use the syntax.
// This file compiles and prints the same with the flag and without it, and
// nothing in it is kind-projector: `*` is multiplication, the repeated
// parameter marker and a user-defined method name, `Lambda` is an ordinary
// name, and a wildcard type argument stays an existential.
object Main {
  final case class Lambda(n: Int)
  final case class Box[A](value: A)

  def widen(x: Int): Int = x * 3

  // Repeated parameters, plain and generic: `T*` is not a placeholder.
  def firstOr(d: Int, xs: Int*): Int = d
  def countOr[A](d: Int, xs: A*): Int = d

  // A wildcard type argument.
  def unbox(b: Box[_]): String = b.toString

  // A method named `*`, called both ways.
  final case class Vec(x: Int) {
    def *(k: Int): Vec = Vec(x * k)
  }

  def main(args: Array[String]): Unit = {
    println(widen(4))
    println(firstOr(1, 2, 3))
    println(countOr(2, "a", "b"))
    println(unbox(Box(3)))
    println((Vec(2) * 5).x)
    println(Vec(2).*(6).x)
    println(Lambda(7).n)
  }
}
