// Call sites for `rf_impl.scala`. The macro defs live here, in the *second*
// run, because a macro def scala-rs itself compiled into an earlier run does
// not carry its implementation binding in the pickle yet -- see
// `docs/macros.md` §7.17 "What remains".
import scala.language.experimental.macros

object Main {
  def greet(s: String): Unit = macro RfImpl.greet
  def twoLines(s: String): Unit = macro RfImpl.twoLines
  def report(s: String): String = macro RfImpl.report

  var calls = 0
  def once(s: String): String = { calls += 1; s }

  def main(args: Array[String]): Unit = {
    greet("world")
    twoLines("first")
    // The argument has a side effect, so the count says whether the splice
    // was built exactly once.
    println(report(once("second")))
    println(calls)
  }
}
