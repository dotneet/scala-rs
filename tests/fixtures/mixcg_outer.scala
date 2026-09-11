// Outer super accessors, outer accessors of path-prefixed parents, inner
// case class equality, and the scope of parent constructor arguments.

// `Q.super.m` from a class nested in `Q` goes through `Q$$super$m`.
class AAA { def f = 42; def g(x: Int): String = "AAA.g" + x }
class BBB extends AAA { override def f = 24; override def g(x: Int) = "BBB.g" + x }
trait TT extends AAA { override def f = 7 }
class AAA1 extends AAA with TT {
  override def f = 111
  override def g(x: Int) = "AAA1.g" + x
  class BBB1 extends BBB {
    override def f = AAA1.super.f
    def viaTT = AAA1.super[TT].f
    def gg = AAA1.super.g(3)
    def classMix = AAA1.super[AAA].g(4)
    def lam = List(1, 2).map(i => AAA1.super.f + i)
  }
  class BBB2 extends BBB { override def f = BBB2.super.f }
  object Inner { def h = AAA1.super.f }
  val anon = new BBB { override def f = AAA1.super.f + 1000 }
}

// `new p.C {}` holds `p` as the enclosing instance of `C`.
class B(val tag: String) {
  trait C { def c(): String = "C of " + tag }
  trait D extends C { def d(): String = "D of " + tag }
  class K { def k = "K of " + tag }
  def mk(o: B): B#C = new o.C {}
  def mkK(o: B): B#K = new o.K {}
  def mine: C = new C {}
}

// An inner case class is a different type per enclosing instance.
trait Kase {
  case class CC(name: String)
  case class DD[+A1, A2](x1: A1, x2: A2)
}
class Box(val id: Int) { case class In(x: Int) }

// A parent written as a path to an inner class names its enclosing
// instance: `extends a.In` with no argument list, in an object and in a class.
class Encl(val tag: String) { class In { def v = "in of " + tag } }
object PathParent {
  val a = new Encl("PathParent.a")
  class C extends a.In
}
class PathHost {
  val a = new Encl("host.a")
  class D extends a.In
}

// `c.copy(…)` on an inner case class is built on `c`'s enclosing instance,
// wherever the call is written; a generic one still re-infers its type
// arguments.
class Shop(val tag: String) {
  case class Item(x: Int) { def show = tag + ":" + x }
  case class Box2[+A](a: A, n: Int) { def show = tag + ":" + a + "/" + n }
}

// Parent constructor arguments do not see the template's own members.
class P(val p: Int)
trait HasA { def a: Int }
object Scope {
  val a = 5
  class E1 extends P(a) with HasA { def a = 1 }
  class E2(x: Int) extends P(x + a) { def a = 100 }
  class E3[T: Ordering](x: T) extends P(implicitly[Ordering[T]].compare(x, x) + a)
  object E4 extends P(a) { val a = 2 }
  // An early definition is a constructor local ahead of the super call: the
  // parent's arguments do see it.
  class E5 extends { val a = 30 } with P(a + 1) with HasA
}

object Main {
  object Foo extends Kase
  object Bar extends Kase
  def viaParam(p: B): String = new p.C {}.c()

  def main(args: Array[String]): Unit = {
    val a = new AAA1
    val b1 = new a.BBB1
    println(b1.f + " " + b1.viaTT + " " + b1.gg + " " + b1.classMix + " " + b1.lam)
    println(new a.BBB2().f + " " + a.Inner.h + " " + a.anon.f)

    val b = new B("local")
    println(new b.C() {}.c() + " / " + new b.C {}.c())
    println(new b.D {}.d() + " / " + new b.D {}.c())
    println(viaParam(new B("param")) + " / " + new b.K {}.k)
    val f = () => new b.C {}.c()
    println(f())
    val two = new B("two")
    println(b.mk(two).c() + " / " + b.mkK(two).k + " / " + b.mine.c())

    println(Foo.CC("b") == Bar.CC("b"))
    println(Foo.CC("b") == Foo.CC("b"))
    println(Foo.DD("a", 5) == Bar.DD("a", 5))
    println(Foo.DD("a", 5) == Foo.DD("a", 5))
    val h1 = new Box(1)
    val h2 = new Box(2)
    println((h1.In(1) == h2.In(1)) + " " + (h1.In(1) == h1.In(1)))
    println(h1.In(1).hashCode == h2.In(1).hashCode)

    println(new Scope.E1().p + " " + new Scope.E2(1).p + " " + new Scope.E3(3).p + " " + Scope.E4.p)
    println(new Scope.E5().p + " " + new Scope.E5().a)

    val ph = new PathHost
    println(new PathParent.C().v + " / " + new ph.D().v)
    val shop = new Shop("shop")
    val item = shop.Item(1)
    println(item.copy(x = 2).show + " " + item.copy().show + " " + (item.copy() == item))
    val box = shop.Box2("s", 1)
    val widened: shop.Box2[Any] = box.copy(a = 42)
    println(box.copy(n = 5).show + " " + widened.show)
    def viaProjection(i: Shop#Item) = i.copy(x = 9)
    println(viaProjection(item).show)
  }
}
