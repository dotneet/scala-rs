// Rule 9: a concrete class must implement every deferred member it inherits.
// Without the check this compiled and threw `AbstractMethodError` at the first
// call.
//
// scalac 2.13.16: "class D needs to be abstract." / "Missing implementation
// for member of class B:".
object Main {
  abstract class B { def f: Int }
  class D extends B
  def main(args: Array[String]): Unit = println(new D().f)
}
