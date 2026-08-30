// `==` / `!=` where one side may be `null`.
//
// `x == null` is a *reference* test (nsc emits a bare `ifnonnull`); it used to
// be `x.equals(null)`, which threw on exactly the value the test asks about.
// A general `x == y` on the private runtime has no `BoxesRunTime.equals` to
// hide the null receiver either, so it gets nsc's own expansion:
// `if (x == null) y == null else x.equals(y)`.
object Main {
  case class Box(v: Int)

  def eqNull(x: AnyRef): Boolean = x == null
  def neNull(x: AnyRef): Boolean = x != null
  def nullEq(x: AnyRef): Boolean = null == x
  def nullNe(x: AnyRef): Boolean = null != x
  // a possibly-null receiver against a real value
  def eqStr(x: String): Boolean = x == "a"
  def neStr(x: String): Boolean = x != "a"
  def strEq(x: String): Boolean = "a" == x
  def eqAny(x: Any, y: Any): Boolean = x == y
  def eqBox(x: Box, y: Box): Boolean = x == y
  // a primitive on one side is never null
  def intEqNull(x: Int): Boolean = x == null.asInstanceOf[AnyRef]
  def matchNull(x: AnyRef): String = x match { case null => "n"; case _ => "o" }

  def main(args: Array[String]): Unit = {
    println(eqNull(null)); println(eqNull("a"))
    println(neNull(null)); println(neNull("a"))
    println(nullEq(null)); println(nullEq("a"))
    println(nullNe(null)); println(nullNe("a"))
    println(eqStr(null)); println(eqStr("a")); println(eqStr("b"))
    println(neStr(null)); println(neStr("a"))
    println(strEq(null)); println(strEq("a"))
    println(eqAny(null, null)); println(eqAny(null, "a")); println(eqAny("a", null))
    println(eqAny("a", "a")); println(eqAny(1, 1)); println(eqAny(1, "a"))
    println(eqBox(null, null)); println(eqBox(null, Box(1)))
    println(eqBox(Box(1), null)); println(eqBox(Box(1), Box(1)))
    println(intEqNull(1))
    println(matchNull(null)); println(matchNull("a"))
  }
}
