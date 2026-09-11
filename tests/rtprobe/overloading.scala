// Overload resolution: numeric widening versus exact match, varargs as the
// last resort, null to String versus Object, generic versus specific,
// by-name versus by-value, and inherited overloads.
object Main {
  def f(x: Int) = "Int"; def f(x: Long) = "Long"; def f(x: Double) = "Double"; def f(x: Any) = "Any"
  def g(x: Long) = "g-Long"; def g(x: Double) = "g-Double"
  def h(x: Int*) = "varargs"; def h(x: Int, y: Int) = "two"
  def n(s: String) = "String"; def n(o: Object) = "Object"
  def gen[T](t: T) = "generic"; def gen(i: Int) = "specific-Int"
  def c(x: Char) = "Char"; def c(x: Int) = "Int"
  def sh(x: Short) = "Short"; def sh(x: Int) = "Int"
  def boxy(x: java.lang.Integer) = "Integer"; def boxy(x: Any) = "Any"
  def num(x: Float) = "Float"; def num(x: Double) = "Double"
  def seqOrList(x: Seq[Int]) = "Seq"; def seqOrList(x: List[Int]) = "List"
  def fnOr(f: Int => Int) = "Function1"; def fnOr(x: Int) = "Int value"
  class Base { def m(x: Int) = "Base.m(Int)"; def m(x: String) = "Base.m(String)" }
  class Derived extends Base { def m(x: Double) = "Derived.m(Double)"; override def m(x: Int) = "Derived.m(Int)" }
  def main(args: Array[String]): Unit = {
    val b: Byte = 1; val s: Short = 2; val ch = 'c'; val fl = 1.5f
    println(List(f(1), f(1L), f(1.0), f(b), f(s), f(ch), f(fl), f("x"), f(null)).mkString(" "))
    println(List(g(1), g(1.5f), g('c'), g(b)).mkString(" "))
    println(List(h(), h(1), h(1, 2), h(1, 2, 3)).mkString(" "))
    println(List(n("s"), n(null), n(new Object), n(List(1))).mkString(" "))
    println(List(gen(1), gen("s"), gen(1L), gen(b)).mkString(" "))
    println(List(c('a'), c(97), c(b)).mkString(" ") + " " + List(sh(s), sh(b), sh(3)).mkString(" "))
    println(List(boxy(1), boxy(Integer.valueOf(1)), boxy("s")).mkString(" ") + " " + List(num(1), num(1L), num(1.0f), num(1.0)).mkString(" "))
    println(seqOrList(List(1)) + " " + seqOrList(Vector(1)) + " " + fnOr(_ + 1) + " " + fnOr(3))
    val d = new Derived
    println(List(d.m(1), d.m("s"), d.m(1.5), d.m(1L), (d: Base).m(1)).mkString(" "))
  }
}
