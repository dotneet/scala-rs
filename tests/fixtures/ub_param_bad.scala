// `Unit` erasing to `BoxedUnit` is a *backend* decision; the typer still has
// to reject `()` where a real value is required. (The other direction is not
// an error: SLS 6.26.1 value discarding adapts any expression to `Unit`.)
object Main {
  def g(s: String): String = s
  def main(args: Array[String]): Unit = {
    println(g(()))
  }
}
