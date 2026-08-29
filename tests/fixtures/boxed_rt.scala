// The half of `boxed.scala` that does not need the real scala-library:
// the boxing views are intrinsics (`Integer.valueOf` / `Integer.intValue`),
// and the wrapper classes come from the JDK, so both work on the private
// runtime too. `RichInt`/`RichChar`/`Array.apply` are library-only and live
// in `boxed.scala` instead.
object Main {
  def takesInt(x: Int): Int = x + 1

  def main(args: Array[String]): Unit = {
    val i: java.lang.Integer = 3
    val back: Int = i
    println(back + 1)
    println(takesInt(i))
    println(i.intValue)
    println(java.lang.Integer.parseInt("42"))
    println(java.lang.Integer.MAX_VALUE)
    println(java.lang.Character.isDigit('4'))
    val c: java.lang.Character = 'x'
    println(c.charValue)
    val z: java.lang.Boolean = true
    println(z.booleanValue)
    val d: java.lang.Double = 0.5
    println(d.doubleValue)
    val n: java.lang.Integer = 'c'
    println(n.intValue)
    val m: java.lang.Long = 9
    println(m.longValue)
  }
}
