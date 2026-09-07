// A legal, acyclic, diamond-dense hierarchy: `A(n)` and `B(n)` each extend
// both `A(n-1)` and `B(n-1)`, so there are 2^n distinct paths from the bottom
// to the top and every one of them reaches the same ancestors.
//
// `SymbolTable::is_sub_type` used to take every one of those paths. This
// fixture is the *positive* half of that: it asks a wide spread of subtype
// questions whose answer is `true` and prints what it got, so a guard that
// bought termination by dropping a `true` -- which would silently change
// overload and implicit selection rather than produce a diagnostic -- shows up
// here as different output.
trait A0 { def tag: String = "A0" }
trait B0 { def btag: String = "B0" }
trait A1 extends A0 with B0
trait B1 extends A0 with B0
trait A2 extends A1 with B1
trait B2 extends A1 with B1
trait A3 extends A2 with B2
trait B3 extends A2 with B2
trait A4 extends A3 with B3
trait B4 extends A3 with B3
trait A5 extends A4 with B4
trait B5 extends A4 with B4
trait A6 extends A5 with B5
trait B6 extends A5 with B5
trait A7 extends A6 with B6
trait B7 extends A6 with B6
trait A8 extends A7 with B7
trait B8 extends A7 with B7

class Bottom extends A8 with B8

object Main {
  // Each of these is an implicit upcast: the argument's static type has to be
  // shown to conform to the parameter's, which is the walk under test.
  def takesA0(x: A0): String = x.tag
  def takesB0(x: B0): String = x.btag
  def takesA4(x: A4): String = x.tag
  def takesB6(x: B6): String = x.btag
  def takesA8(x: A8): String = x.tag

  // An overload whose resolution depends on the walk answering `true` for the
  // more specific alternative: picking `A0` here would print the wrong line.
  def which(x: A0): String = "picked A0"
  def which(x: A8): String = "picked A8"

  def main(args: Array[String]): Unit = {
    val b = new Bottom
    println(takesA0(b))
    println(takesB0(b))
    println(takesA4(b))
    println(takesB6(b))
    println(takesA8(b))
    println(which(b))
    println(which(new A0 {}))
    // Upcast through a chain of intermediate static types.
    val a8: A8 = b
    val a4: A4 = a8
    val a0: A0 = a4
    println(a0.tag)
    val b8: B8 = b
    val b0: B0 = b8
    println(b0.btag)
    println(b.isInstanceOf[A0])
    println(b.isInstanceOf[B5])
  }
}
