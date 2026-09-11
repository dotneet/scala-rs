// `super[T].m` picks a specific parent; plain `super.m` in a class goes to
// the linearization; a trait calling super reaches a class method.
object Main {
  trait A { def m: String = "A"; def n(x: Int): Int = x + 1 }
  trait B { def m: String = "B"; def n(x: Int): Int = x * 10 }
  class C extends A with B {
    override def m: String = super[A].m + super[B].m + super.m
    override def n(x: Int): Int = super[A].n(x) + super[B].n(x)
  }
  class Root { def hello: String = "root"; def v: Int = 1 }
  trait Deco extends Root { override def hello: String = "deco+" + super.hello; override def v: Int = super.v * 5 }
  class Leaf extends Root with Deco { override def hello: String = "leaf+" + super.hello; def rootHello = super[Deco].hello }

  trait Greets { def greet(who: String): String = s"hi $who" }
  class Polite extends Greets { override def greet(who: String): String = super.greet(who) + ", pleased" }
  class VeryPolite extends Polite { override def greet(who: String): String = super.greet(who.toUpperCase) + "!" }

  // super call inside a closure and a nested method
  class Outer extends Root {
    override def v: Int = 7
    def viaClosure: Int = List(1, 2).map(i => super.v + i).sum
    def viaLocal: Int = { def f = super.v * 100; f }
  }

  // super to a generic trait method with a bridge
  trait Box[T] { def put(t: T): String = "box:" + t }
  class IntBox extends Box[Int] { override def put(t: Int): String = "int" + super.put(t + 1) }

  def main(args: Array[String]): Unit = {
    val c = new C
    println(c.m); println(c.n(3))
    val l = new Leaf
    println(l.hello); println(l.rootHello); println(l.v)
    println(new VeryPolite().greet("bob"))
    val g: Greets = new VeryPolite
    println(g.greet("amy"))
    val o = new Outer
    println(o.viaClosure + " " + o.viaLocal)
    println(new IntBox().put(4))
    val b: Box[Int] = new IntBox
    println(b.put(5))
  }
}
