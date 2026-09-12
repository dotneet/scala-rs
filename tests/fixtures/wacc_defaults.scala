// Default arguments are read through `name$default$n` getters that are
// ordinary virtual methods: an override that writes its own default has its
// own getter, an override that writes none inherits the default, and a call
// that omits an argument asks the *receiver* for it (evaluated once). scala-rs
// emitted no getter for an override, so every one of these used the base's
// default, and `super` into a trait default had no static accessor to call.
class A { def g(a: Int = 1): Int = a }
class B extends A { override def g(a: Int = 2): Int = a * 10 }
class C extends A { override def g(a: Int): Int = a + 100 }

trait T {
  def h(x: Int)(y: Int = x + 1): Int = x * y
  def k(s: String = "t"): String = s
}
class D extends T {
  override def h(x: Int)(y: Int = x + 10): Int = x + y
  override def k(s: String = "d"): String = s + "!"
}
object O extends T { override def k(s: String = "o"): String = s }
class E extends D {
  def sup: String = super.k()
  override def k(s: String = "e"): String = "E" + s
}
class F extends T {
  override def k(s: String = "f"): String = s + "F"
  def sup: String = super.k()
}
trait U { def u(i: Int = 7): Int }
class G extends U { def u(i: Int): Int = i * 2 }

class P { def f(a: Int = 1, b: String = "x", c: Int = 3): String = s"P $a $b $c" }
class Q extends P {
  override def f(a: Int = 10, b: String = "y", c: Int = 30): String = s"Q $a $b $c"
}

object Main {
  var made = 0
  def mk(): P = { made += 1; new Q }
  def main(args: Array[String]): Unit = {
    println(new B().g() + " " + (new B: A).g())
    println(new C().g() + " " + (new C: A).g())
    val t: T = new D
    println(t.h(2)() + " " + new D().h(3)() + " " + t.k())
    println(O.k() + " " + (O: T).k())
    println(new E().sup + " " + new E().k() + " " + (new E: D).k())
    println(new F().sup + " " + new F().k() + " " + (new F: T).k())
    println(new G().u() + " " + (new G: U).u())
    println(mk().f(b = "q"))
    println(mk().f(c = 7, a = 5))
    println(mk().f() + " " + mk().f(2))
    println(made)
  }
}
