// `x @ Pat` binds at the *pattern's* type, not the scrutinee's.
//
// `case n @ N(v, _) => n.copy(...)` used to store the raw scrutinee, so `n`
// stayed typed `T` in the frame and reading `N`'s fields off it was a
// `VerifyError`. The type-pattern spelling (`case n: N`) always worked, which
// is why this went unnoticed.
object Main {
  sealed trait T
  case class N(v: Int, w: T) extends T
  case object L extends T

  def copied(t: T): T = t match {
    case n @ N(v, _) => n.copy(v = v + 10)
    case L => L
  }
  // the type-pattern spelling, as a control
  def control(t: T): T = t match {
    case n: N => n.copy(v = n.v + 10)
    case L => L
  }
  def nested(t: T): String = t match {
    case a @ N(x, b @ N(y, _)) => s"${a.v} $x ${b.v} $y"
    case n @ N(x, _) => s"leaf ${n.v} $x"
    case L => "L"
  }
  def typed(t: T): String = t match {
    case n @ (_: N) => "N" + n.v
    case _ => "other"
  }
  def guarded(t: T): String = t match {
    case n @ N(_, _) if n.v > 0 => "pos" + n.v
    case n @ N(_, _) => "nonpos" + n.v
    case L => "L"
  }
  // `@` narrowing an `Any` scrutinee down to a primitive: the bound local is
  // an `int` slot, so the reference has to be unboxed before it is stored.
  def prim(x: Any): String = x match {
    case i @ (_: Int) => "int" + (i + 1)
    case s @ (_: String) => "str" + s.length
    case _ => "o"
  }
  def extractor(x: Any): String = x match {
    case p @ Some(v @ (_: Int)) => s"some ${p.get} $v"
    case _ => "o"
  }
  def tuple(x: Any): String = x match {
    case p @ (a, b) => s"${p._1} $a $b"
    case _ => "o"
  }
  // a bound variable in a `catch`
  def caught(): String =
    try { throw new IllegalStateException("boom"); "no" }
    catch { case e @ (_: IllegalStateException) => "caught " + e.getMessage }

  def main(args: Array[String]): Unit = {
    println(copied(N(1, L)))
    println(control(N(1, L)))
    println(nested(N(1, N(2, L))))
    println(nested(N(1, L)))
    println(nested(L))
    println(typed(N(3, L)))
    println(typed(L))
    println(guarded(N(1, L)))
    println(guarded(N(-1, L)))
    println(guarded(L))
    println(prim(41))
    println(prim("ab"))
    println(prim(1.0))
    println(extractor(Some(7)))
    println(extractor(Some("x")))
    println(extractor(null))
    println(tuple((1, 2)))
    println(tuple(null))
    println(caught())
  }
}
