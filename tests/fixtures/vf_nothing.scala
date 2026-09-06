// `Nothing` in a bridge, on both sides of the call.
//
// `Nothing` erases to `scala.runtime.Nothing$`, which is a subtype of nothing
// at all, so an erasure bridge cannot treat it like any other reference:
//
//   * a *parameter* of the implementation typed `Nothing` needs
//     `checkcast scala/runtime/Nothing$` -- the bridge is handed the parent's
//     `Object` (slick's `ResultConverter[…, Updater = Nothing, …]`, two
//     classes, `VerifyError: Bad type on operand stack`);
//   * a *result* of `Nothing$` cannot be handed back or converted. nsc's
//     `adapt` follows such a call with `athrow`; we emitted `areturn`
//     (slick's `override lazy val updateCompiler: Nothing = ??` on
//     `DistributedProfile`, `VerifyError: Bad return type`).
//
// The `update` bridge cannot be *called* from Scala at all -- every argument
// expression of type `Nothing` diverges before the call is reached -- so what
// pins it is that the JVM links `C1`, which verifies every method body. That
// is the whole point of this fixture: `javap` and a non-initialising
// `Class.forName` both read these classes back without a murmur.
package vf

abstract class Conv[R, W, U, T] {
  def read(pr: R): T
  def update(value: T, pr: U): Unit
  def width: Int
}

trait Base {
  def compiler: String
  def named(n: Int): String
}

class C1 extends Conv[String, Int, Nothing, Any] {
  override def read(pr: String): Nothing = throw new RuntimeException("r")
  override def update(value: Any, pr: Nothing): Nothing = throw new RuntimeException("u")
  def width = 1
}

// `lazy val` and plain `def`, both overriding a wider declaration.
object Impl extends Base {
  override lazy val compiler: Nothing = throw new RuntimeException("c")
  override def named(n: Int): Nothing = throw new RuntimeException("n" + n)
}

object Main {
  def caught(body: => Any): String =
    try { body; "no throw" }
    catch { case e: RuntimeException => "caught " + e.getMessage }

  def main(args: Array[String]): Unit = {
    val c: Conv[String, Int, Nothing, Any] = new C1
    println(c.width)
    // Through the erased parent signatures, i.e. through the bridges.
    println(caught(c.read("a")))
    val b: Base = Impl
    println(caught(b.compiler))
    println(caught(b.named(7)))
    // And through the narrow signatures, which are not bridges.
    println(caught(Impl.compiler))
    println(caught(new C1().read("z")))
  }
}
