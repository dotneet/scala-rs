// Deprecations: library members (from their pickles), a source
// `@deprecated`, the parser's (symbol literal, procedure syntax), and
// uncurry's copied varargs array. Summarized by default, printed one by one
// under -deprecation.
object WarnDepr {
  @deprecated("use two", "1.0") def one: Int = 1

  def proc() { println("proc") }

  def total(xs: Int*): Int = xs.sum

  def main(args: Array[String]): Unit = {
    val s = Stream(1, 2)
    println(s.head)
    println(one)
    println('sym.name)
    val a = Array(1, 2, 3)
    println(total(a: _*))
    println(1 << 2L)
    println(3.round)
    proc()
  }
}
