// `null` against every kind of pattern (SLS 8.1.1 / 8.1.2).
//
// `case null` is a *reference* comparison; it used to be compiled as
// `x.equals(null)`, which threw a `NullPointerException` on the one scrutinee
// the case exists to catch. A constant pattern puts the constant on the left
// for the same reason, a type pattern never matches `null` (`instanceof` says
// so), and an extractor pattern is guarded by `ifnull` rather than handed a
// `null` to reason about.
object Ex {
  def unapply(x: Any): Option[Int] = x match {
    case s: String => Some(s.length)
    case _ => None
  }
}
object Main {
  case class Box(v: Int)

  def literal(x: Any): String = x match {
    case null => "null"
    case "a" => "A"
    case 1 => "one"
    case _ => "o"
  }
  def stable(x: Any): String = x match { case Nil => "nil"; case _ => "o" }
  def typePat(x: Any): String = x match { case s: String => "s"; case _ => "o" }
  def anyPat(x: Any): String = x match { case r: AnyRef => "ref"; case _ => "o" }
  def extractor(x: Any): String = x match { case Ex(n) => "ex" + n; case _ => "o" }
  def caseClass(x: Any): String = x match { case Box(v) => "box" + v; case _ => "o" }
  def catchAll(x: Any): String = x match { case _ => "w" }
  // the ordering the report turned on: the tuple case must fall through to
  // `case null` without calling an extractor.
  def tupleThenNull(x: Any): String = x match {
    case (a, b) => s"pair $a $b"
    case null => "null"
    case _ => "other"
  }
  // a `String` scrutinee: `"a".equals(x)`, never `x.equals("a")`
  def strEq(x: String): String = x match { case "a" => "A"; case _ => "o" }

  def main(args: Array[String]): Unit = {
    println(literal(null)); println(literal("a")); println(literal(1)); println(literal(2))
    println(stable(null)); println(stable(Nil))
    println(typePat(null)); println(typePat("s"))
    println(anyPat(null)); println(anyPat("s"))
    println(extractor(null)); println(extractor("abc"))
    println(caseClass(null)); println(caseClass(Box(3)))
    println(catchAll(null))
    println(tupleThenNull(null)); println(tupleThenNull((1, 2))); println(tupleThenNull(7))
    println(strEq(null)); println(strEq("a"))
  }
}
