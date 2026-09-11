// scalac: method f overrides nothing -- parameter types are invariant under
// overriding, so `f(x: String)` does not override `f(x: Any)`.
object Main {
  class A { def f(x: Any): String = "A" }
  class B extends A { override def f(x: String): String = "B" }
  def main(args: Array[String]): Unit = println(new B().f("s"))
}
