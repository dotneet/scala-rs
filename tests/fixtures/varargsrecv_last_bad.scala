// A repeated parameter covers every argument from its position on, so one
// that is not last in its clause has no meaning. scalac 2.13.16 reports
// `*-parameter must come last` at each of these three, and nothing else that
// this compiler has to reproduce.
object Bad {
  def f(xs: Int*, y: Int): Int = y
}
case class BadCase(xs: Int*, y: Int)
class BadClass(xs: Int*, y: Int)
