// A type parameter or abstract type member erases to the erasure of its upper
// bound -- a primitive included -- and an inherited alias is read through the
// enclosing class that inherits it. Descriptors are printed through
// reflection so that scalac's and scala-rs's ABIs are compared directly.

trait Base[A] { type B = A }
class C extends Base[String] {
  class D {
    def foo[B1 <: B](b: B1) = 0
    def len[B1 <: B](b: B1): Int = b.length
    class E { def baz[B2 <: B](b: B2): B2 = b }
  }
  def bar[B1 <: B](b: B1): B1 = b
}
trait BaseHK[M[_], A] { type B = M[A] }
object BaseHK { type Id[X] = X }
class CHK extends BaseHK[BaseHK.Id, String] {
  class D { def foo[B1 <: B](b: B1) = 0 }
}

class P[A <: Int](val a: A) {
  def get: A = a
  def twice(x: A): Int = x + x
  def pair: (A, A) = (a, a)
  def lst: List[A] = List(a, a)
  def fn: Int => A = (i: Int) => a
  var v: A = a
  def setv(x: A): Unit = { v = x }
  override def toString = "P(" + a + ")"
}
class P2 extends P[Int](3) { override def get: Int = 5 }
trait T[A <: Int] { def f(a: A): A; def g(a: A): Int = f(a) * 10 }
class TI extends T[Int] { def f(a: Int) = a + 1 }
trait U { type E <: Long; def e: E; def show: String = "e=" + e }
object UL extends U { type E = Long; def e = 42L }
trait X { type T <: Int; def f(t: T): T }
class Y extends X { type T = Int; def f(t: Int): Int = t + 1 }

object Main {
  def id[A <: Int](a: A): A = a
  def idl[A <: Long](a: A): A = a
  def ids[A <: Boolean](a: A): Boolean = !a
  def str[A <: Char](c: A): String = c.toString + c
  def dbl[A <: Double](d: A): Double = d * 2
  def arr[A <: Int](a: Array[A]): A = a(0)
  def mk[A <: Int](a: A): List[A] = List(a, a)
  def first[A <: Int](xs: Seq[A]): A = xs.head
  def max2[A <: Int](x: A, y: A): A = if (x > y) x else y
  def gen[B](b: B): B = b
  def viaGen[A <: Int](x: A): A = gen(x)

  def show(cls: Class[_], name: String): String =
    cls.getDeclaredMethods.filter(_.getName == name).map(_.toString).sorted.mkString(" | ")

  def main(args: Array[String]): Unit = {
    val c = new C
    val d = new c.D()
    val e = new d.E
    println(show(d.getClass, "foo"))
    println(show(e.getClass, "baz"))
    println(show(c.getClass, "bar"))
    println(d.len("abc") + " " + e.baz("q") + " " + c.bar("x"))
    val chk = new CHK
    println(show((new chk.D).getClass, "foo"))

    val p = new P(3)
    println(p.get + p.twice(4))
    println(p.pair + " " + p.lst + " " + p.fn(1))
    p.setv(9)
    println(p.v + " " + p)
    println(classOf[P[_]].getConstructors.map(_.toString).mkString(" | "))
    println(show(classOf[P[_]], "get") + " / " + show(classOf[P[_]], "twice"))
    val q: P[Int] = new P2
    println(q.get + " " + new P2().a)
    val t: T[Int] = new TI
    println(t.f(1) + " " + t.g(2))
    println(show(classOf[T[_]], "f") + " / " + show(classOf[TI], "f"))
    println(UL.show + " " + (UL.e + 1))
    val u: U = UL
    println(u.e)
    println(show(classOf[U], "e"))
    val x: X = new Y
    println(new Y().f(4))
    println(show(classOf[X], "f") + " / " + show(classOf[Y], "f"))

    println(id(3) + 1)
    println(idl(3L) + " " + ids(true) + " " + str('z') + " " + dbl(1.5))
    println(arr(Array(5, 6)) + " " + mk(4) + " " + first(Vector(8, 9)) + " " + max2(3, 7))
    println(viaGen(6))
    for (n <- List("id", "idl", "ids", "str", "dbl", "arr", "mk", "first", "max2", "viaGen"))
      println(show(Main.getClass, n))
    val ps = List(new P(1), new P(2))
    println(ps.map(_.get).sum + " " + ps.map(_.a))
  }
}
