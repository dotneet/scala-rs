// The valid neighbours of what crates/cli/tests/wacc.rs rejects: scalac
// 2.13.16 compiles and runs every definition here, and so must scala-rs.
import scala.annotation.{implicitNotFound, nowarn, switch, tailrec}
import scala.annotation.unchecked.uncheckedVariance

// A nested package clause opens its enclosing packages; `Use` sees `Top`.
package p {
  class Top { override def toString = "Top" }
  package q {
    package r {
      object Use { def v: String = new Top().toString }
    }
  }
}

// Defaults in one alternative only; an override keeps its own defaults.
object Defaults {
  def f(a: Int = 1): Int = a
  def f(s: String): Int = s.length
  class A { def g(a: Int = 1): Int = a }
  class B extends A {
    override def g(a: Int = 2): Int = a * 10
    def g(s: String): Int = 0
  }
  case class CC(x: Int = 3)
  object CC { def make: CC = CC() }
}

// Overloaded references that nsc resolves: a function type is expected, an
// alternative is implicit-only, a value and a method differ in shape, a
// subclass alternative is the more specific one.
object Refs {
  def over(a: Int): Int = a + 1
  def over(s: String): Int = s.length
  val fi: Int => Int = over
  def imp(implicit i: Int): Int = i
  def imp(s: String): Int = 1
  implicit val ii: Int = 7
  val viaImplicit = imp
  def w(x: Int): Int = x * 2
  val w = 5
  val h: Any => String = _ => "value"
  def h(s: String): String = "method"
  class C { def f(x: Any): String = "C.any" }
  class D extends C { def f(x: Int): String = "D.int" }
}

// Names that share a spelling but not a scope or a namespace.
object Names {
  class A(val x: Int) { def x(i: Int): Int = i + x }
  def typeAndValue[T](T: Int): Int = T
  class Hid { private[this] val y = 1; def y(i: Int): Int = i + y }
  object Obj; class Obj
  def local: Int = {
    abstract class M { def h: Int }
    val m = new M { def h = 6 }
    trait L { def g: Int }
    new L { def g = 1 }.g + m.h
  }
  def pat(a: Any): String = a match {
    case (x, y) => s"pair $x $y"
    case 1 | 2 => "small"
    case List(x, _*) => s"list $x"
    case _ => "other"
  }
}

// Modifiers nsc accepts.
object Mods {
  trait Stack { def put(x: Int): Int }
  class Base extends Stack { def put(x: Int): Int = x }
  trait Doubling extends Stack { abstract override def put(x: Int): Int = super.put(x * 2) }
  final case class F(i: Int)
  sealed abstract class S
  implicit class Twice(val i: Int) { def twice: Int = i * 2 }
  object Wrapper { protected[Wrapper] def secret = 3; def open: Int = secret }
  lazy val lz = 9
}

// Annotations resolve, arguments included.
@implicitNotFound("no show for ${T}") trait Show[T]
@SerialVersionUID(1L) class Ser extends Serializable
class Mark(val n: Int) extends scala.annotation.StaticAnnotation
@Mark(1) class Marked
object Annots {
  @deprecated("old", "1.0") def old: Int = 1
  @inline final def inl: Int = 2
  @transient lazy val tr = 3
  @volatile var vol = 4
  @throws(classOf[java.io.IOException]) def thr(): Int = 5
  @nowarn("cat=deprecation") def nw: Int = old
  def sw(x: Int): Int = (x: @switch) match { case 1 => 10; case _ => 20 }
  @tailrec def loop(n: Int, acc: Int): Int = if (n == 0) acc else loop(n - 1, acc + n)
  def uv[A](xs: List[A @uncheckedVariance]): Int = xs.size
  def spec[@specialized(Int) T](t: T): T = t
}

// A value class may override what `AnyVal` defines, when it says so.
class Meters(val v: Int) extends AnyVal {
  override def toString: String = s"${v}m"
}

object Main {
  def main(args: Array[String]): Unit = {
    println(p.q.r.Use.v)
    println(Defaults.f() + " " + Defaults.f("ab"))
    println(new Defaults.B().g(4) + " " + new Defaults.B().g("s"))
    println(Defaults.CC.make)
    println(Refs.fi(1) + " " + Refs.viaImplicit)
    println(Refs.w + " " + Refs.w(3))
    println(Refs.h(1) + " " + Refs.h("s"))
    println((new Refs.D).f(1) + " " + (new Refs.D).f("s"))
    println(new Names.A(2).x(3) + " " + Names.typeAndValue[String](8))
    println(new Names.Hid().y(1) + " " + Names.local)
    println(Names.pat((1, 2)) + ", " + Names.pat(2) + ", " + Names.pat(List(7, 8)))
    println((new Mods.Base with Mods.Doubling).put(5))
    println(Mods.F(1) + " " + new Mods.Twice(4).twice + " " + Mods.Wrapper.open + " " + Mods.lz)
    println(Annots.nw + Annots.inl + Annots.tr + Annots.vol + Annots.thr() + Annots.sw(1) + Annots.loop(4, 0))
    println(Annots.uv(List(1, 2)) + " " + Annots.spec(3))
    val x = 4
    println(s"$$ $x ${x + 1} $"" + raw"a\tb")
    println(new Meters(3))
  }
}
