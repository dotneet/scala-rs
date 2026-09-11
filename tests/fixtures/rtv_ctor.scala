// Code that runs before a constructor's super (or self) call, where `this`
// is still `uninitializedThis` (fixture prefix `rtv_`).
class Base(val a: Any) { override def toString = "Base(" + a + ")" }
trait Foo { def v: Int = 7 }
object Util { def twice(n: Int) = n * 2 }

class Outer(val k: Int) {
  // `this(...)` arguments read the enclosing instance from the `$outer`
  // parameter (`run/constructors`).
  class Inner(val x: Int, val s: String) {
    def this(s: String) = this(k * 2, s + k)
    def this() = this({ val t = k + 1; t }, "none")
  }
  // An anonymous class in super arguments gets no usable outer
  // (`run/t6957`).
  class Sub extends Base(new Foo {})
}

// A `def` in constructor arguments becomes a static method (`run/t1909b`,
// `run/t1909c`), and receives the argument block's locals.
class Lifted extends Base({ val t = 3; def f(y: Int) = t + y; def g = f(10) * 2; g + Util.twice(f(1)) })
class Lifted2(val q: Int) extends Base(q) {
  def this() = this({ def h(n: Int): Int = if (n == 0) 0 else n + h(n - 1); h(4) })
  def this(s: String) = this({ val k = s.length; def m = k * 100; m })
}

// A local of a template-level expression is captured by a def lifted out of
// it, not read off `this`.
class Template {
  val x = { val t = 1; def f = t + 1; f }
  val y = { val u = 5; def g(k: Int) = u * k; g(3) }
}

object Main {
  trait T
  class WL(a: Any)
  object S { def #::(a: Any): Any = "s" }
  def main(args: Array[String]): Unit = {
    val o = new Outer(5)
    val i1 = new o.Inner("a")
    println(i1.x + " " + i1.s)
    val i2 = new o.Inner()
    println(i2.x + " " + i2.s)
    println(new o.Sub().a.asInstanceOf[Foo].v)
    println(new Lifted)
    println(new Lifted2().q)
    println(new Lifted2("abc").q)
    val t = new Template
    println(t.x + " " + t.y)
    // An anonymous class in the super arguments of another one
    // (`run/t6506`).
    println(new WL(new {} #:: S) with T {}.getClass.getSuperclass.getSimpleName)
    class Local(val q: Int) extends Base(new Foo {}) {
      def this() = this(3)
    }
    println(new Local().q)
  }
}
