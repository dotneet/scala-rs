// `Unit` in *argument* position erases to `scala/runtime/BoxedUnit`, not to
// `V`: a descriptor with a bare `V` parameter is not even a legal signature
// and the JVM refused to load the whole class.
object Main {
  def f(x: Unit): String = "got"
  def g(): Unit = ()
  def middle(a: Int, b: Unit, c: String): String = c + a
  def two(a: Unit, b: Unit): String = "two"
  def high(u: Unit): Unit = ()
  // `Nothing` has the same shape: `V` as a result, `scala/runtime/Nothing$`
  // as a parameter. Uncallable, but the class still has to load — the
  // verifier resolves a parameter's class.
  def never(x: Nothing): Int = 1

  class C(val u: Unit) {
    def m(x: Unit, n: Int): Int = n + 1
  }

  def main(args: Array[String]): Unit = {
    println(f(()))
    // The argument of a `V`-returning call still has to become `BoxedUnit`.
    println(f(g()))
    println(middle(1, (), "s"))
    println(two((), ()))
    println(high(()))
    val c = new C(())
    println(c.u)
    println(c.m((), 41))
    // A `Unit` parameter takes a slot, so the parameters after it are read
    // from the right index.
    println(middle(7, g(), "x"))
  }
}
