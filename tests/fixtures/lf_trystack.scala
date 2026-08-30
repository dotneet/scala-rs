// A `try` reached with values already on the operand stack.
//
// The JVM clears the operand stack when it enters an exception handler (JVMS
// 4.10.1.6), so whatever was pending is gone on the catch path and the join
// after the `try` saw a stack of depth n on one side and 0 on the other:
// `VerifyError: Inconsistent stackmap frames`. It hit `println(try …)`,
// an argument after another argument, and a constructor argument -- where the
// pending value is the still *uninitialized* result of `new`.
//
// scalac lifts such a `try` into a synthetic `liftedTree1$1()` method and calls
// that from the argument position; we park the pending values in locals for the
// duration of the guarded region instead.
class Box(val s: String)

object Main {
  def two(a: String, b: String): String = a + b

  def main(args: Array[String]): Unit = {
    // The README's repro: a `while` and then a `try` in argument position.
    var i = 0
    while (i < 2) { println("w" + i); i += 1 }
    println(try { "y" } catch { case _: Throwable => "no" })

    // The `try` is the *second* argument, so a value is already pending.
    println(two("p", try { "q" } catch { case _: Throwable => "no" }))

    // A constructor argument: `new` and `dup` leave two uninitialized
    // references on the stack before the `try` runs.
    val b = new Box(try { "a" } catch { case _: Throwable => "b" })
    println(b.s)

    // A primitive pending under the `try`, and a `try` that actually throws.
    println(two("n=" + (1 + (try { 2 } catch { case _: Throwable => 0 })), "!"))
    println(try { throw new RuntimeException("boom") } catch { case e: RuntimeException => e.getMessage })

    // A `try` in argument position inside a loop, storing to a local whose
    // class the loop head has to merge. (`Option#toString` is a case-class
    // method the private runtime does not have, so ask for a `Boolean`.)
    var c: Option[Int] = Some(0)
    var k = 0
    while (k < 3) {
      println(two("k", (try { c.isDefined.toString } catch { case _: Throwable => "?" })))
      c = if (k == 1) None else Some(k)
      k += 1
    }
    println(c.isDefined)

    // `try` with a `finally`, still in argument position.
    println(try { "f" } finally { print("fin ") })
  }
}
