// Three code-generation faults that nothing but the JVM's own verifier saw.
//
//  1. A `while (true)` that is the first thing in a method branches back to
//     bytecode offset 0, and JVMS 4.7.4 wants a stack map frame there. The
//     frame writer skipped offset 0 as "the implicit initial frame", so the
//     class was rejected with
//     `VerifyError: Expecting a stackmap frame at branch target 0`.
//
//  2. `athrow` verifies only against a `Throwable`. A `Nothing`-typed *tree*
//     does not always leave one on the stack: `Function0.apply` on a lambda
//     that always throws erases to `()Ljava/lang/Object;`.
//
//  3. `NonLocalReturnControl.value()` is `()Ljava/lang/Object;`, so a method
//     with a reference result needs the cast its own descriptor promises
//     before `areturn`.
object Main {
  private var ticks = 0

  // (1) The method body *starts* with the loop, so its head is offset 0.
  def loopAtOffsetZero(): Int = {
    while (true) {
      try {
        ticks += 1
        if (ticks >= 3) return ticks
      } catch {
        case _: RuntimeException => ()
      }
    }
    ticks
  }

  // (2) and (3): the lambda never returns normally, and the value it carries
  // out through the control exception comes back as `Object`.
  def escapeWithString(): String = {
    val f: () => Nothing = () => return "escaped"
    f()
    "not reached"
  }

  // (3) again, with a reference result the handler has to `checkcast`.
  def firstEven(xs: Array[Int]): Option[Int] = {
    val step: Int => Unit = (x: Int) => if (x % 2 == 0) return Some(x)
    var i = 0
    while (i < xs.length) {
      step(xs(i))
      i += 1
    }
    None
  }

  def main(args: Array[String]): Unit = {
    println(loopAtOffsetZero())
    println(escapeWithString())
    // Built element by element: `Array(...)` is a library varargs `apply`,
    // and this fixture also runs against the private runtime.
    val xs = new Array[Int](4)
    xs(0) = 1
    xs(1) = 3
    xs(2) = 4
    xs(3) = 5
    println(firstEven(xs).get)
  }
}
