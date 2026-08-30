// Rule 2: parameter types are *invariant* under overriding. A different one
// makes the member an overload, so the `override` modifier has nothing to
// refer to.
//
// scalac 2.13.16: "method f overrides nothing."
object Main {
  class B { def f(x: Int): Int = x }
  class D extends B { override def f(x: String): Int = 1 }
  def main(args: Array[String]): Unit = println(new D().f("a"))
}
