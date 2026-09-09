// Named arguments in an auxiliary constructor's `this(...)` delegation, and
// assignment to things that are and are not `var`s.
//
// `name = value` inside `this(...)` is a *named argument*, not an assignment.
// The self-delegation path had no idea: it typed each pair as an `Assign`,
// which reported `reassignment to val <name>` against the primary
// constructor's own parameter (in scope in an auxiliary constructor's body)
// and then `no matching overload for constructor` for the `Unit`s left
// behind. `scala/collection/mutable/AnyRefMap.scala`'s four auxiliary
// constructors are eight of those errors on their own.
//
// The half that has to be **executed** is the argument *placement*. A named
// argument that reorders the call must still evaluate left to right as
// written (SLS 6.6.1) while landing in its parameter's slot, and an omitted
// default has to reach the flat `<init>` descriptor. A store to the wrong
// slot type-checks and runs wrong, so `Eff.log` below records the order the
// arguments were evaluated in and each constructed object prints which
// parameter got which value.

object Eff {
  var log: String = ""
  def apply[T](tag: String, t: T): T = { log += tag + ";"; t }
}

class C(a: String, b: Int, c: Boolean = true) {
  // Every named, and reordered: `b` is written first and must still be `b`.
  def this() = this(b = Eff("b", 2), a = Eff("a", "x"), c = false)
  // Mixed: one positional, one named.
  def this(a: String) = this(a, b = 9)
  // A named argument that names the *one-parameter* alternative below, where
  // the primary constructor also declares `a`: the alternative that cannot be
  // applied must not be the one the names are matched against.
  def this(n: Int) = this(a = "n" + n)
  override def toString = a + "/" + b + "/" + c
}

// Curried: `this(a = 1)(b = true)` is one call after flattening.
class D(a: Int)(b: Boolean) {
  def this() = this(a = Eff("da", 1))(b = Eff("db", true))
  override def toString = a + "/" + b
}

// A repeated tail left unfilled by a named argument.
class R(a: Int, rest: Int*) {
  def this() = this(a = 4)
  override def toString = a + "/" + rest.length
}

// The accepted direction of the mutability check: every shape of a real
// `var`, assigned. None of these may start being rejected.
trait TV { var tv: Int = 0 }
class BV { var bv: Int = 0 }
object OV { var ov: Int = 0 }

class Vars(var cv: Int) extends BV with TV {
  private[this] var pv: Int = 0
  var fv: Int = 0
  def bump(): String = {
    var local = 1
    local = local + 1
    cv = 10
    fv = 20
    this.fv = this.fv + 1
    pv = 30
    tv = 40
    bv = 50
    OV.ov = 60
    "" + local + "/" + cv + "/" + fv + "/" + pv + "/" + tv + "/" + bv + "/" + OV.ov
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new C())
    println(Eff.log)
    println(new C("p"))
    println(new C(3))
    Eff.log = ""
    println(new D())
    println(Eff.log)
    println(new R())
    println(new Vars(0).bump())
  }
}
