// Overriding vals, defs, vars and lazy vals; what a superclass sees through
// a virtual accessor during and after construction.
object Main {
  class A { val x: Int = 1; def y: Int = 2; lazy val z: Int = 3; def show = s"x=$x y=$y z=$z" }
  class B extends A { override val x: Int = 10; override def y: Int = 20; override lazy val z: Int = 30 }
  class C extends A { override val y: Int = 200 }  // val overriding def
  class Snap extends A { val seenX = x; override val x: Int = 99 }

  abstract class Shape { def area: Double; val name: String; override def toString = s"$name(${area})" }
  class Sq(s: Double) extends Shape { val area = s * s; val name = "sq" }
  class Circ(r: Double) extends Shape { def area = 3.0 * r * r; val name = "circ" }

  trait HasVar { var level: Int }
  class Impl extends HasVar { var level = 5 }
  trait VarDefault { var count: Int = 1; def bump(): Unit = count += 1 }
  class VD extends VarDefault { count = 10 }

  trait Named { def name: String }
  case class Person(name: String) extends Named

  class Parent { def who(x: Any): String = "parent-any"; def who(x: String): String = "parent-string" }
  class Child extends Parent { override def who(x: Any): String = "child-any" }

  class GP[T] { def id(t: T): T = t; def tag: String = "gp" }
  class GC extends GP[String] { override def id(t: String): String = t + "!"; override def tag = "gc" }

  def main(args: Array[String]): Unit = {
    val a: A = new B
    println(a.show)
    println(new C().show)
    val s = new Snap
    println(s.seenX + " " + s.x)
    println(new Sq(2)); println(new Circ(1))
    val h: HasVar = new Impl
    h.level = h.level + 1
    println(h.level)
    val vd = new VD; vd.bump(); println(vd.count)
    val n: Named = Person("ann"); println(n.name)
    val c = new Child
    println(c.who(1: Any) + " " + (c: Parent).who("s") + " " + (c: Parent).who("s": Any))
    val g: GP[String] = new GC
    println(g.id("x") + " " + g.tag)
  }
}
