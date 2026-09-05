// Narrowing the conversions by the argument count must not turn into
// accepting a call no conversion can take, and must not silence a real
// ambiguity.
//
// `nope` has no applicable alternative at any arity, so the diagnostic is
// still "is not a member". `tie` is a genuine ambiguity that the argument
// count cannot break: two different conversions both offer a one-argument
// `~~~`, which is what nsc reports as an ambiguous conversion too.

class Cell(val b: Boolean)

class OneArg(c: Cell) {
  def &&&(o: Cell): Cell = c
  def ~~~(o: Cell): Cell = c
}

class TwoArg(c: Cell) {
  def &&&(o: Cell, guard: Boolean): Cell = c
  def ~~~(o: Cell): Cell = c
}

object Conv {
  implicit def one(c: Cell): OneArg = new OneArg(c)
  implicit def two(c: Cell): TwoArg = new TwoArg(c)
}

object Bad {
  import Conv._

  def three(t: Cell): Cell = t.&&&(t, true, true)
  def tie(t: Cell): Cell = t ~~~ t
}
