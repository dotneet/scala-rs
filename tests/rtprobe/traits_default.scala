// Traits with concrete members, fields, and defaults: diamond inheritance,
// trait fields per instance, trait methods calling abstract ones, generic
// trait methods over a class hierarchy, and a trait extending a class.
object Main {
  trait Id { private var _id = 0; def id: Int = _id; def setId(i: Int): this.type = { _id = i; this } }
  trait Named { val name: String; def label: String = s"$name#" }
  trait Both extends Id with Named { override def label: String = super.label + id }
  class Thing(val name: String) extends Both
  trait Comparable2[T] { def cmp(o: T): Int; def <(o: T): Boolean = cmp(o) < 0; def max(o: T): T = if (cmp(o) >= 0) this.asInstanceOf[T] else o }
  case class N(v: Int) extends Comparable2[N] { def cmp(o: N) = v - o.v }
  abstract class Base { def base: String = "base" }
  trait OnBase extends Base { override def base: String = "onbase:" + super.base }
  class Final extends OnBase
  trait Stateful { var calls = List.empty[String]; def record(s: String): Unit = calls = s :: calls }
  trait A extends Stateful { def a(): Unit = record("a") }
  trait B extends Stateful { def b(): Unit = record("b") }
  class AB extends A with B
  trait WithInit { val initTime: String = "init-" + getClass.getSimpleName }
  class Inits extends WithInit
  trait Greeting { def greet(n: String): String = "Hello " + n }
  object Enthusiastic extends Greeting { override def greet(n: String) = super.greet(n) + "!!!" }
  trait Defaults { def x: Int = 1; def y: Int = x + 1; def z: Int = y * 10 }
  object D1 extends Defaults
  object D2 extends Defaults { override def x = 5 }
  object D3 extends Defaults { override def y = 100 }

  def main(args: Array[String]): Unit = {
    val t1 = new Thing("t1").setId(7); val t2 = new Thing("t2")
    println(t1.label + " " + t2.label + " " + t1.id + t2.id)
    println((N(1) < N(2)) + " " + N(3).max(N(2)) + " " + N(1).max(N(5)))
    println(new Final().base)
    val ab = new AB; ab.a(); ab.b(); ab.a()
    println(ab.calls)
    println(new Inits().initTime)
    println(Enthusiastic.greet("x"))
    println(s"${D1.z} ${D2.z} ${D3.z}")
    val anon = new Id with Named { val name = "anon" }
    println(anon.setId(3).label + " " + anon.id)
  }
}
