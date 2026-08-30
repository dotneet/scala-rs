// `Unit` as an operand of `==` / `!=` / `equals` / `##`.
//
// `Unit` erases to `scala/runtime/BoxedUnit` in every *value* position, and a
// comparison's operands are value positions: `() == ()` really does push two
// `BoxedUnit.UNIT`s. The expression that produces a `Unit` leaves nothing on
// the stack, so the singleton has to be materialised at the operand -- without
// that the comparison popped what was never pushed and the whole method failed
// to verify (`VerifyError: Operand stack underflow`), silently, at run time.
//
// The receiver of any member selected on a `Unit` value is the same position:
// `().toString`, `().hashCode`, `().isInstanceOf[Unit]`, `().getClass`.

class Box(val n: Int) {
  override def equals(o: Any): Boolean = o match {
    case b: Box => b.n == n
    case _      => false
  }
  override def hashCode: Int = n
}

case class Rec(u: Unit, n: Int)

object Main {
  def g(): Unit = ()
  def id[A](a: A): A = a
  def eqUnit(a: Unit, b: Unit): Boolean = a == b

  def main(args: Array[String]): Unit = {
    // literal operands
    println(() == ())
    println(() != ())
    // locals
    val u1 = ()
    val u2 = ()
    println(u1 == u2)
    println(u1 != u2)
    // a call whose result is Unit: the call leaves nothing, the operand still
    // has to be there
    println(g() == g())
    println(g() == ())
    println(() == g())
    // Unit parameters, i.e. real BoxedUnit slots
    println(eqUnit((), ()))
    // explicit equals / hashCode / toString on a Unit receiver (`##` needs
    // `scala.runtime.Statics`, which only the real library has -- see
    // `ue_eqlib.scala`)
    println(().equals(()))
    println(().hashCode)
    println(u1.hashCode)
    println(().toString)
    println(().toString.length)
    // type tests on a Unit receiver
    println(().isInstanceOf[Unit])
    println(().asInstanceOf[Unit])
    println(().getClass)
    // through `Any`
    val a: Any = ()
    val b: Any = ()
    println(a == b)
    println(a == ())
    println(() == a)
    println(a != ())
    // a Unit never equals a non-Unit
    println(() == 1)
    println(a == 1)
    println(a == "x")
    // erased through a type parameter: the call already left a boxed ref
    println(id(()) == ())
    println(() == id(()))
    // in a condition, and as a statement whose value is discarded
    println(if (() == ()) "y" else "n")
    var s = 0
    if (g() != ()) s += 1 else s += 2
    while (s == 2 && () != ()) s += 1
    println(s)
    // patterns
    ((): Any) match {
      case () => println("unit")
      case _  => println("other")
    }
    () match { case () => println("u2") }
    // a Unit field inside a case class: the generated equals compares them
    println(Rec((), 3) == Rec((), 3))
    println(Rec((), 3) == Rec((), 4))
    println(Rec((), 3).u == ())
    // a user-defined equals reached with a Unit argument
    println(new Box(1) == new Box(1))
  }
}
