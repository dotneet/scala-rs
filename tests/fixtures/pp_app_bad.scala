// The other half of the round trip: a `def f: T` we pickled must *stay*
// parameterless. Real scalac rejects each of these against `pp_lib`'s class
// files, whether scala-rs or scalac compiled them, at the same lines with the
// same message.
object BadMain {
  def main(args: Array[String]): Unit = {
    println(Keys.nullary())
    val c = new Counter(0)
    println(c.label())
  }
}
