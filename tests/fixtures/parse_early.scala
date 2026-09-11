// Early definitions (`extends { ... } with T`, `new { ... } with T`) and
// procedure-syntax auxiliary constructors (`def this() { this(1) }`).

trait T { val x: Int; val y: Int; def z = x + y }
trait A { def a: Int }
trait Named { val name: String; val greeting = "hello " + name }

class Box(arg: String) {
  var log = "Box(" + arg + ")"
  // Deprecated procedure syntax, accepted by 2.13 (also under -Xsource:3).
  @deprecated("..") def this() {
    this("default")
    log += " via this()"
  }
  def this(n: Int)
  {
    this(n.toString)
  }
}

class Outer {
  val tag = "outer"
  // `this` in an early definition is the enclosing instance.
  class Inner extends { val self: Outer = this } with AnyRef
}

object Main {
  val a = 5
  // `a` in the early section is `Main.a`: the class's own `def a` does not
  // exist yet (nsc types the section in the constructor context).
  class D extends { val b = a } with A { def a = 1 }
  // A later early value reads an earlier one.
  class E(q: Int) extends { val x = q + a; val y = x * 2 } with T

  def f(a: Int) = new {
    private[this] val b = a
  } with A {
    def a = b
  }

  def main(args: Array[String]): Unit = {
    println(new Box().log)
    println(new Box(7).log)
    println(new D().b)
    println(new E(1).z)
    println(f(7).a)
    val n = new { val name = "early" } with Named
    println(n.greeting)
    val t = new { val x = 2; val y = x + 1 } with T
    println(t.z)
    println(new { val s = "abc" } with AnyRef { override def toString = s }.toString)
    val o = new Outer
    println((new o.Inner).self.tag)
  }
}
