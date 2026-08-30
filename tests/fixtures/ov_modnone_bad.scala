// Rule 3, the other direction: `override` on a member that redefines nothing.
//
// scalac 2.13.16: "method h overrides nothing".
object Main {
  class B { def g(x: Int): Int = x }
  class D extends B { override def h(x: Int): Int = x }
  def main(args: Array[String]): Unit = println(new D().h(1))
}
