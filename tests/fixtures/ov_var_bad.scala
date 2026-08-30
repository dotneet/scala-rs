// Rule 7, the `var` half: a *concrete* mutable variable is not overridable at
// all. (A deferred `var` in a trait is a declaration, and may be implemented.)
//
// scalac 2.13.16: "mutable variable cannot be overridden:".
object Main {
  class B { var v: Int = 1 }
  class D extends B { override var v: Int = 2 }
  def main(args: Array[String]): Unit = println(new D().v)
}
