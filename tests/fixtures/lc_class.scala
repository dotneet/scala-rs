// implicit class local to a method body, a nested def, and a lambda body --
// scalac finds all three as extension-method sources (SLS 7.3: a view
// candidate is drawn from the same scope chain as an implicit parameter).
object Main {
  def main(a: Array[String]): Unit = {
    implicit class F(val n: Int) { def dbl: Int = n * 2 }
    println(3.dbl)

    def inner(): String = {
      implicit class Loud(s: String) { def shout: String = s + "!" }
      "hi".shout
    }
    println(inner())

    val f: Int => String = { n =>
      implicit class Wrap(x: Int) { def wrapped: String = "[" + x + "]" }
      n.wrapped
    }
    println(f(5))
  }
}
