// Fixture for the `agent/intrinsicqual` slice: `identity` / `locally` /
// `implicitly` are calls on their own receiver.
//
// `gen_apply` used to dispatch the `Predef` polymorphic intrinsic on the
// *name* alone, and `gen_predef_poly` discards the qualifier and every
// argument but the first, emitting
// `scala/Predef$.<name>:(Ljava/lang/Object;)Ljava/lang/Object;`. So every
// selection spelled `identity`, `locally` or `implicitly` became
// `Predef.identity(firstArg)` -- the identity function -- and the program
// gave back its own argument instead of running the method it wrote.
//
// The `C-` / `O-` prefixes are what tell the user's members apart from
// `Predef`'s. Every line here is compared byte for byte with real scalac
// 2.13.16 on the same source.

class Poly(tag: String) {
  def identity(x: String): String = "C-identity[" + tag + "]:" + x
  def locally(x: String): String = "C-locally[" + tag + "]:" + x
  def implicitly(x: String): String = "C-implicitly[" + tag + "]:" + x
  // Two arguments: `gen_predef_poly` reads `args(0)` and drops the rest, so
  // the second argument was never even evaluated.
  def identity(a: String, b: String): String = "C-identity2:" + a + "|" + b
}

object Helper {
  def identity(x: String): String = "O-identity:" + x
  def locally(x: String): String = "O-locally:" + x
  def implicitly(x: String): String = "O-implicitly:" + x
}

object Main {
  var trace: String = ""

  implicit val strEv: String = "ev"

  def mk(tag: String): Poly = {
    trace = trace + "<mk:" + tag + ">"
    new Poly(tag)
  }

  def side(x: String): String = {
    trace = trace + "<arg:" + x + ">"
    x
  }

  def main(args: Array[String]): Unit = {
    // The user's members, selected on an object and on an instance.
    println(Helper.identity("a"))
    println(Helper.locally("b"))
    println(Helper.implicitly("c"))

    val p = new Poly("p")
    println(p.identity("d"))
    println(p.locally("e"))
    println(p.implicitly("f"))
    println(p.identity("g", "h"))

    // The qualifier is an expression with a side effect, and so is the second
    // argument: dropping either is visible in the trace.
    println(mk("m").identity(side("i"), side("j")))
    println(trace)

    // `Predef`'s own, which must stay the intrinsic: unqualified, qualified,
    // and through the `scala.` prefix.
    println(identity("k"))
    println(Predef.identity("l"))
    println(scala.Predef.identity("m"))
    println(locally { "n" })
    println(locally("o"))
    println(implicitly[String])
    println(identity(7))
    println(locally(8))
  }
}
