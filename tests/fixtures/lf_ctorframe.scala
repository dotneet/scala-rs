// Frames inside a *constructor*, after the super constructor call.
//
// JVMS 4.10.1.9: `invokespecial <init>` turns `uninitializedThis` into the type
// of the class **being verified**. Those differ for the super constructor call
// every subclass makes, and writing the *invoked* class there made every later
// frame claim `this` was a `B`:
//
// ```text
// VerifyError: Bad type on operand stack in putfield
//   Type 'B' (current frame, stack[0]) is not assignable to 'C'
// ```
//
// Any constructor that needs a frame at all after the super call hit it -- a
// branch, a loop, or a `try` in the constructor body.
class B(val s: String)

class C(n: Int) extends B("b") {
  // A branch after the super call: the join needs a frame, and `this` is in it.
  val sign: String = if (n > 0) "pos" else if (n < 0) "neg" else "zero"

  // A loop in the constructor body, with a loop-carried local.
  val counted: Int = {
    var acc = 0
    var o: Option[Int] = Some(n)
    while (o.isDefined) {
      acc += 1
      o = if (acc < 3) Some(acc) else None
    }
    acc
  }

  // A `try` in the constructor body.
  val guarded: String = try { "g" + n } catch { case _: Throwable => "no" }
}

// The pending value under the `try` is `uninitializedThis` itself: the super
// constructor's argument is produced by a `try`.
class D(n: Int) extends B(try { "d" + n } catch { case _: Throwable => "no" })

object Main {
  def main(args: Array[String]): Unit = {
    val c = new C(1)
    println(c.s)
    println(c.sign)
    println(new C(-1).sign)
    println(new C(0).sign)
    println(c.counted)
    println(c.guarded)
    println(new D(2).s)
  }
}
