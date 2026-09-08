// `Boolean`'s bitwise operators, and the numeric `unary_+`.
//
// `javap -p scala.Boolean` on scala-library-2.13.16.jar declares eight
// instance members: `unary_!`, `==`, `!=`, `||`, `&&`, `|`, `&` and `^`. The
// prelude had the first five; this fixture pins the last three.
//
// The point of *running* it is that `&` and `|` are NOT spellings of `&&` and
// `||`. `p & q` evaluates `q` even when `p` is false; `p && q` does not.
// Compiling is not enough to tell those apart -- both type-check, both pass
// the verifier -- so every operand here appends to a trace, and the trace is
// what the expected output is made of. Emitting the short-circuiting form for
// `&` would print `L` where this prints `LR`.
object Main {
  var trace: String = ""

  def note(tag: String, v: Boolean): Boolean = {
    trace = trace + tag
    v
  }

  def show(what: String, r: Boolean): Unit = {
    println(what + " = " + r + " trace=" + trace)
    trace = ""
  }

  def main(args: Array[String]): Unit = {
    // `&` evaluates both operands even though the left one decides the result.
    show("false &  true", note("L", false) & note("R", true))
    show("false && true", note("L", false) && note("R", true))
    // `|` likewise, even though the left one already makes it true.
    show("true  |  false", note("L", true) | note("R", false))
    show("true  || false", note("L", true) || note("R", false))
    // `^` has no short-circuiting counterpart at all.
    show("true  ^  true", note("L", true) ^ note("R", true))
    show("true  ^  false", note("L", true) ^ note("R", false))

    // The value table, so a wrong instruction (ior for iand, say) shows up.
    println("&: " + (false & false) + " " + (false & true) + " " + (true & false) + " " + (true & true))
    println("|: " + (false | false) + " " + (false | true) + " " + (true | false) + " " + (true | true))
    println("^: " + (false ^ false) + " " + (false ^ true) + " " + (true ^ false) + " " + (true ^ true))

    // They are ordinary members, so the explicit-receiver spelling works too.
    println("explicit: " + true.&(false) + " " + true.|(false) + " " + true.^(false))

    // `unary_+` is the identity, with nsc's widening of the result type:
    // `Int` for Byte/Short/Char/Int, the receiver's own type above that.
    // `(-3).toByte` rather than `val b: Byte = -3`: narrowing a *negated*
    // constant literal to `Byte` is a separate, pre-existing gap here, and
    // this fixture is about `unary_+`, not about that.
    val b: Byte = (-3).toByte
    val s: Short = (-4).toShort
    val c: Char = 'A'
    val i: Int = -5
    val l: Long = -6L
    val f: Float = -7.5f
    val d: Double = -8.25
    val ub: Int = b.unary_+
    val us: Int = s.unary_+
    val uc: Int = c.unary_+
    val ui: Int = i.unary_+
    val ul: Long = l.unary_+
    val uf: Float = f.unary_+
    val ud: Double = d.unary_+
    println("unary_+: " + ub + " " + us + " " + uc + " " + ui + " " + ul + " " + uf + " " + ud)
    println("prefix: " + (+i) + " " + (+l) + " " + (+d))
  }
}
