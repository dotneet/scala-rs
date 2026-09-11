// Default and named arguments: evaluation order is the order written at the
// call site, defaults may refer to earlier parameters, and defaults are
// re-evaluated per call.
object Main {
  val sb = new StringBuilder
  def e[T](tag: String, v: T): T = { sb.append(tag); v }
  def flush(): String = { val s = sb.toString; sb.clear(); s }

  def f(a: Int, b: Int = 10, c: Int = 100): Int = a * 10000 + b * 100 + c
  def g(a: Int, b: String): String = s"$a/$b"
  var counter = 0
  def next(): Int = { counter += 1; counter }
  def h(x: Int = next(), y: Int = next()): String = s"$x,$y"
  def dep(a: Int, b: Int = 0, c: Int = -1): String = s"$a $b $c"
  def dep2(a: Int)(b: Int = a * 2): Int = a + b

  class Pt(val x: Int = 1, val y: Int = 2) { override def toString = s"Pt($x,$y)" }
  case class Cfg(name: String = "def", size: Int = 3, flags: List[String] = Nil)

  def over(x: Int, y: String = "d"): String = "int:" + x + y
  def over(x: String): String = "str:" + x

  def main(args: Array[String]): Unit = {
    println(f(1)); println(f(1, 2)); println(f(1, c = 3)); println(f(c = 3, a = 1)); println(f(b = 5, a = 2))
    println(g(b = e("B", "bee"), a = e("A", 1)) + " order=" + flush())
    println(f(c = e("c", 1), b = e("b", 2), a = e("a", 3)) + " order=" + flush())
    println(h()); println(h(y = 50)); println(h(x = 7)); println(h())
    println(dep(1)); println(dep(1, c = 9)); println(dep2(3)()); println(dep2(3)(1))
    println(new Pt()); println(new Pt(y = 5)); println(new Pt(x = 9))
    val c = Cfg(size = 7)
    println(c); println(c.copy(name = "x")); println(c.copy(flags = List("a", "b"), size = 1))
    println(over(1)); println(over("s")); println(over(2, "e"))
    // named args to a method taking a function
    def ap(f: Int => Int, x: Int) = f(x)
    println(ap(x = 4, f = _ * 3))
  }
}
