// A negated numeric *literal* is a constant expression (SLS 6.24), so SLS
// 6.26.1's narrowing conversion applies to it exactly as it does to a plain
// literal: `val b: Byte = -3` is legal for the same reason `val b: Byte = 3`
// is. The parser desugars `-3` to `Apply(Select(Literal(3), "unary_-"), Nil)`
// -- there is no negative-literal token -- so the typer has to recover the
// constant-ness from that shape; `unary_-`'s own declared return type is the
// widened `Int`, which is why this is worth compiling and *running*, not
// just accepting: a mistake in the fold could produce a Byte field holding
// the wrong bit pattern while still type-checking.
object Main {
  def main(args: Array[String]): Unit = {
    val b: Byte = -3
    val s: Short = -4
    val c: Char = 65
    // Plain literals, unaffected by this fix, printed alongside the negated
    // ones so a regression in the existing path would show up here too.
    val b2: Byte = 3
    // The two `Byte` boundary values in the negative direction.
    val b3: Byte = 127
    val b4: Byte = -128
    // Widening a negated literal to `Long`/`Float`/`Double` is a different
    // code path (`numeric_widen`, not the narrowing this slice touches) and
    // already worked -- kept here as a check that this change did not
    // disturb it.
    val l: Long = -3
    val f: Float = -3
    val d: Double = -3
    println(b)
    println(s)
    println(c)
    println(b2)
    println(b3)
    println(b4)
    println(l)
    println(f)
    println(d)
  }
}
