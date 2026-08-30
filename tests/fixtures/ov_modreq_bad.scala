// Rule 3: redefining a *concrete* inherited member requires `override`.
//
// scalac 2.13.16: "`override` modifier required to override concrete member:".
object Main {
  class B { def f(x: Int): Int = x }
  class D extends B { def f(x: Int): Int = x + 1 }
  def main(args: Array[String]): Unit = println(new D().f(1))
}
