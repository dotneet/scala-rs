// A call on a receiver the assembler already tracks as reaching the method's
// owner needs no `checkcast` -- but a call whose receiver only *widens* to the
// owner still does. Both shapes in one file so the run pins them together.
package tk

trait U {
  def um(): String = "u"
}

// Self type: `this` inside `T` is a `T`, which is not a `U`. The receiver of
// `um()` has to be cast, and nsc casts it too.
trait T { self: U =>
  def viaSelf(): String = um() + this.um()
}

trait Mix {
  def mm(): String = "m"
  // `this` is the interface `Mix`, which is exactly the owner: no cast.
  def useMm(): String = mm() + this.mm()
}

class Base {
  def bm(): String = "b"
}

// A trait extending a *class*. `TT` compiles to an interface, so the hop the
// Scala hierarchy makes -- `TT <: UBase` -- is one the bytecode cannot: the
// receiver here is a `TT` and the verifier will not hand it to a method whose
// `Methodref` names the class `UBase`. This is the shape
// `scala.reflect.api.JavaUniverse` has in the wild (`interfaces: 0` in the
// class file, `extends Universe` in the pickle), and nsc casts here too.
abstract class UBase {
  def um2(): String = "n"
}

trait TT extends UBase {
  def viaClassParent(): String = um2() + this.um2()
}

class CT extends TT

// `this` is a `C`, which reaches `C`, `Base` and `Mix` alike: no cast on any
// of these calls.
class C extends Base with T with U with Mix {
  def cm(): String = "c"
  private def pm(): String = "p"
  final def fm(): String = "f"
  def all(): String = cm() + this.cm() + pm() + fm() + bm() + useMm() + viaSelf()
}

// A receiver that really is erased to `Object` when captured by a lambda: the
// cast has to survive.
class Cap {
  def cm(): String = "z"
}

class Boxed[A](a: A) {
  def get: A = a
  def show(): String = get.toString
}

class Outer {
  def om(): String = "o"
  class Inner {
    def im(): String = om() + Outer.this.om()
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new C().all())
    val c = new Cap
    val f = () => c.cm()
    println(f())
    println(new Boxed("y").show())
    val o = new Outer
    val i = new o.Inner
    println(i.im())
    println(new CT().viaClassParent())
  }
}
