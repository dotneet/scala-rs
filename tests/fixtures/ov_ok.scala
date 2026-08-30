// SLS 5.1.4 "Overriding": everything a *legal* override may do. This is the
// guard against the conformance check over-rejecting -- every shape here
// compiled before the check existed and must still compile.
//
// Only string and integer operations, so it runs under the private runtime as
// well as against the real scala-library jar.
object Main {
  // 1. The result type may narrow (covariance).
  class Base { def make: Any = "base" }
  class Narrow extends Base { override def make: String = "narrow" }

  // 2. A different parameter type is an *overload*, not an override, and needs
  //    no `override` modifier.
  class Over { def f(x: Int): String = "int " + x }
  class OverMore extends Over { def f(x: String): String = "str " + x }

  // 3. `override` is not required to implement a deferred member ...
  trait Deferred { def d: String }
  class Implements extends Deferred { def d = "implemented" }
  // ... and is required, and accepted, on a concrete one.
  class Concrete { def c: String = "base" }
  class Redefines extends Concrete { override def c: String = "derived" }

  // 4. An abstract class may re-declare an inherited concrete member as
  //    deferred; a subclass then implements it. `Concrete.c` is still in the
  //    linearization and still concrete, so the modifier is required -- scalac
  //    reports exactly this if it is left off.
  abstract class Redeclares extends Concrete { override def c: String }
  class Grounds extends Redeclares { override def c = "grounded" }

  // 5. `final` in a *sibling* branch is no obstacle.
  class Sealed { final def s: String = "final" }

  // 6. Visibility may widen.
  class Prot { protected def p: String = "prot" }
  class Wider extends Prot { override def p: String = "public" }

  // 7. A `val` may override a `def`, and a `val` ctor parameter may implement
  //    a deferred `val`. A bare ctor parameter shadows without overriding.
  class DefSide { def v: Int = 1 }
  class ValSide extends DefSide { override val v: Int = 2 }
  trait NeedsVal { val n: String }
  class GivesVal(val n: String) extends NeedsVal
  class NamedBase(val name: String)
  class BareParam(name: String) extends NamedBase(name)

  // 8. A type parameter's bound may widen.
  class Bounded { def b[A <: AnyRef](x: A): A = x }
  class Unbounded extends Bounded { override def b[A](x: A): A = x }

  // 9. A generic member implemented at the instantiated type, through an
  //    anonymous class -- the shape the missing check used to miscompile.
  trait It[A] { def next(): A }

  // `def f: T` and `def f(): T` match each other in both directions.
  trait Nilary { def z(): Int }
  class NilaryImpl extends Nilary { def z: Int = 7 }

  // The universal members really are overridable.
  class Talks { override def toString: String = "talks" }
  class Talks2 { override def toString(): String = "talks2" }

  def main(args: Array[String]): Unit = {
    println(new Narrow().make)
    val o = new OverMore
    println(o.f(1) + "/" + o.f("x"))
    println(new Implements().d)
    println(new Redefines().c)
    println(new Grounds().c)
    println(new Sealed().s)
    println(new Wider().p)
    println(new ValSide().v)
    println(new GivesVal("given").n)
    println(new BareParam("bare").name)
    println(new Unbounded().b("boundless"))
    val i = new It[Int] { def next(): Int = 41 + 1 }
    println(i.next())
    println(new NilaryImpl().z)
    println(new Talks)
    println(new Talks2)
  }
}
