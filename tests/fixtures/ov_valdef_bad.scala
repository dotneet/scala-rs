// Rule 7: a `val` may override a `def`, never the reverse -- a `def` is not a
// stable value, and code that relied on the `val` being stable would break.
//
// scalac 2.13.16: "stable, immutable value required to override:".
object Main {
  class B { val v: Int = 1 }
  class D extends B { override def v: Int = 2 }
  def main(args: Array[String]): Unit = println(new D().v)
}
