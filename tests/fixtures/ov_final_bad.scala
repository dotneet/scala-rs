// Rule 5: a `final` member is closed.
//
// scalac 2.13.16: "cannot override final member:".
object Main {
  class B { final def f: Int = 1 }
  class D extends B { override def f: Int = 2 }
  def main(args: Array[String]): Unit = println(new D().f)
}
