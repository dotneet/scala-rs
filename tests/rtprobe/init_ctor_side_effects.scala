// Constructor bodies run statements in order; auxiliary constructors run the
// primary one first; superclass constructors run before subclass fields;
// a virtual call from a superclass constructor sees the subclass uninitialized.
object Main {
  val sb = new StringBuilder
  def log(s: String): Unit = sb.append(s).append(';')

  class A(x: Int) {
    log("A1 x=" + x)
    val y = x + 1
    log("A2 y=" + y)
    def this() = { this(100); log("A aux") }
    def this(s: String) = { this(s.length); log("A aux s=" + s) }
  }

  abstract class Sup { log("Sup init describe=" + describe); def describe: String }
  class Sub(val name: String) extends Sup { val tag = "tag:" + name; def describe = "name=" + name + " tag=" + tag }

  class Params(a: Int, val b: Int, var c: Int) {
    def sum = a + b + c
    c += 1
  }

  class Shadow(x: Int) { val x2 = x; private val z = x * 2; def get = z }

  class SuperArgs(v: Int) extends A({ log("arg-eval"); v * 2 }) { log("SuperArgs body") }

  def main(args: Array[String]): Unit = {
    new A(1); println(sb); sb.clear()
    new A(); println(sb); sb.clear()
    new A("abc"); println(sb); sb.clear()
    val s = new Sub("n"); println(sb); sb.clear()
    println(s.describe)
    val p = new Params(1, 2, 3)
    println(p.sum + " " + p.b + " " + p.c)
    p.c = 10
    println(p.sum)
    println(new Shadow(7).get + new Shadow(7).x2)
    new SuperArgs(5); println(sb); sb.clear()
  }
}
