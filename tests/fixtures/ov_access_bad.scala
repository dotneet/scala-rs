// Rule 6: an override may widen visibility, never narrow it.
//
// scalac 2.13.16: "weaker access privileges in overriding" /
// "  override should be public".
object Main {
  class B { def f: Int = 1 }
  class D extends B { protected override def f: Int = 2 }
  def main(args: Array[String]): Unit = println(new D)
}
