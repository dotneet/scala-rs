// `&&` / `||` make their *right* operand a tail position and nothing else.
// Every method here must still be rejected, at the same place nsc rejects it.
import scala.annotation.tailrec

object TrcBoolBad {
  // The left operand is evaluated first and its value is branched on, so a
  // self call there is not in tail position.
  @tailrec def left(n: Int): Boolean = left(n - 1) && (n > 0)

  // `!` consumes the value of its operand.
  @tailrec def negated(n: Int): Boolean = (n <= 0) || !negated(n - 1)

  // The short circuit is not itself in tail position.
  @tailrec def bound(n: Int): Boolean = {
    val b = (n > 0) && bound(n - 1)
    b
  }

  // A user-defined `||` is an ordinary strict method: its argument is
  // evaluated before the call, so it is an argument position and not a tail
  // position. Only `scala.Boolean`'s `&&` and `||` short circuit.
  @tailrec def custom(n: Int): TrcFlag = new TrcFlag(n <= 0) || custom(n - 1)

  // A recursive call in a `val`'s right-hand side is not in tail position
  // even when another call is.
  @tailrec def bound2(n: Int): Int =
    if (n <= 0) 0
    else {
      val x = bound2(n - 1)
      bound2(x)
    }
}

final class TrcFlag(val b: Boolean) {
  def ||(other: TrcFlag): TrcFlag = if (b) this else other
}

class TrcBoolOpen {
  // Reachable through `&&`, but the method can be overridden.
  @tailrec final def ok(n: Int): Boolean = (n <= 0) || ok(n - 1)
  @tailrec def open(n: Int): Boolean = (n <= 0) || open(n - 1)
}
