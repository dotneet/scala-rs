// `agent/unitpop`: a `Unit`-valued `Predef` intrinsic and the statement-position
// discard have to agree about whether a value was left on the stack.
//
// `gen_predef_poly` used to `pop` its own result whenever the result type was
// `Unit`. That is right for a *statement* and wrong for an *argument*:
// `println(identity(()))` was handed an empty stack and threw
// `VerifyError: Operand stack underflow` at run time. It compiles and it
// passes every check but running it, which is why both halves are here and why
// the test runs them under `-Xverify:all`.
//
// Both positions are covered on purpose. A fix that merely stopped popping
// would leave a stray reference behind in statement position and fail the
// verifier the other way round -- and that is not hypothetical: the private
// runtime was already failing that way (`if (b) identity(()) else side()` was
// `VerifyError: Inconsistent stackmap frames`), because erasure boxes the
// argument and `unit_stat_leaves_ref` refuses every symbol carrying an
// `Intrinsic`. A non-`Unit` intrinsic call sits beside each `Unit` one so the
// two paths cannot drift.
//
// Every line's expected output is real scalac 2.13.16's.
package unitpoppkg

class Wrap(val n: Int) { def twice: Int = n * 2 }

object Main {
  implicit val ev: String = "ev"
  implicit val eu: Unit = ()

  def side(s: String): Unit = println("side " + s)
  def pair(a: Unit, b: Unit): String = "pair " + a + b
  def myid[A](a: A): A = a

  // ------------------------------------------------- value (argument) position
  def a1(): Unit = println(identity(()))
  def a2(): Unit = println(locally(()))
  def a3(): Unit = println(locally { side("a3"); () })
  def a4(): Unit = println(implicitly[Unit])
  def a5(): Unit = println(pair(identity(()), locally(())))
  def a6(): Unit = println(identity(side("a6")))
  def a7(): Unit = { val u: Unit = identity(()); println(u) }

  // the same three at a *non*-`Unit` type, so the two paths stay together
  def b1(): Unit = println(identity("k"))
  def b2(): Unit = println(identity(7))
  def b3(): Unit = println(locally { "n" })
  def b4(): Unit = println(implicitly[String])
  // a *class*-typed result: the erased `(Object)Object` still owes the
  // `checkcast` `maybe_unbox_erased_result` puts there, or the `invokevirtual`
  // on it is `VerifyError: Bad type on operand stack`.
  def b5(): Unit = println(identity(new Wrap(3)).twice)

  // ------------------------------------------------------- statement position
  // Each discards the value and is then followed by a branch: a stray
  // reference survives into the next stackmap frame and the verifier says so.
  def s1(b: Boolean): Unit = { identity(()); if (b) side("s1t") else side("s1f") }
  def s2(b: Boolean): Unit = { locally { side("s2") }; if (b) side("s2t") else side("s2f") }
  def s3(b: Boolean): Unit = { locally(()); if (b) side("s3t") else side("s3f") }
  def s4(b: Boolean): Unit = { implicitly[Unit]; if (b) side("s4t") else side("s4f") }
  def s5(b: Boolean): Unit = { identity(side("s5")); if (b) side("s5t") else side("s5f") }

  // discarded non-`Unit` results, same shape
  def s6(b: Boolean): Unit = { identity("s6"); locally { "s6" }; implicitly[String]; if (b) side("s6t") else side("s6f") }

  // a discarded intrinsic as a *branch* of a statement `if`, a `match` arm, a
  // `try` body and a `while` body: every one of these is a control-flow join.
  def s7(b: Boolean): Unit = if (b) identity(()) else side("s7f")
  def s8(b: Boolean): Unit = if (b) locally { side("s8") } else side("s8f")
  def s9(b: Boolean): Unit = if (b) identity("s9") else side("s9f")
  def s10(b: Boolean): Unit = b match {
    case true  => identity(())
    case false => side("s10f")
  }
  def s11(b: Boolean): Unit = {
    try identity(())
    catch { case _: Throwable => side("s11c") }
    if (b) side("s11t") else side("s11f")
  }
  def s12(n: Int): Unit = {
    var i = 0
    while (i < n) {
      identity(())
      locally { side("s12-" + i) }
      i += 1
    }
  }

  // the whole body of a `Unit` method is a discarded value too
  def s13(): Unit = identity(())
  def s14(): Unit = locally { side("s14") }
  def s15(): Unit = myid(())

  def main(args: Array[String]): Unit = {
    a1(); a2(); a3(); a4(); a5(); a6(); a7()
    b1(); b2(); b3(); b4(); b5()
    s1(true); s2(true); s3(true); s4(true); s5(true); s6(true)
    s7(true); s7(false); s8(true); s8(false); s9(true); s9(false)
    s10(true); s10(false); s11(true); s12(2)
    s13(); s14(); s15()
    println("done")
  }
}
