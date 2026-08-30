// The `collect` result-type overload has to *discriminate*, not just resolve:
// a case block returning something other than `Char` picks the
// `IndexedSeq[B]` alternative, so binding it to a `String` must be rejected.
// Accepting this is how the erasure bug shows up -- the call would then emit
// the `String`-returning `collect$extension` and die with a
// `ClassCastException` at run time.
object Main {
  def main(args: Array[String]): Unit = {
    val s = "abcdef"
    val bad: String = s.collect { case c if c > 'c' => c.toInt }
    println(bad)
  }
}
