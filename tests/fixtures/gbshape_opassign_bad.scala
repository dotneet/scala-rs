// `===` is not an op-assignment: nsc's `isOpAssignmentName` rejects an
// operator whose first character is `=`. Reading it as one turned
// `b === false` on a `var b: Boolean` into `b = (b != ...)`, which type-checks
// -- a program real scalac 2.13.16 rejects was accepted, silently, and with
// the comparison's result written into the variable.
//
// `x_=` is not one either, because `x` is not an operator character: nsc
// reports the missing member rather than rewriting the call into an
// assignment to `p.x`.
//
// Every line below is rejected by real scalac 2.13.16, and by no others.
object Bad {
  class Box(val n: Int)
  class P { val x: Int = 1 }

  def f1(): Unit = {
    var b = true
    b === false
  }

  def f2(): Unit = {
    var i = 0
    i === 1
  }

  def f3(b: Box): Unit = {
    b === b
  }

  def f4(p: P): Unit = {
    p.x_=(2)
  }
}
