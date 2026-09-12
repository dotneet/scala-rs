// refchecks warnings: pure expressions in statement position, discarded
// pure results, comparisons that always yield the same answer, and typer's
// discarded `return` value.
trait Held[T] { val held: T }

object WarnPure {
  var count = 0
  val h: Held[Unit] = new Held[Unit] { val held = () }

  def unitBody(): Unit = 1

  def discarded(): Unit = {
    count += 1
    2
  }

  def tryUnit: Unit = try { 10 } catch { case _: Throwable => () }

  def compare(u: Unit, i: Int, b: Boolean): Unit = {
    println(u == ())
    println(i == "x")
    println(b != 3)
  }

  def early(): Unit = {
    val f = () => return 7
    f()
  }

  def main(args: Array[String]): Unit = {
    1 + 1
    count
    h.held
    -5
    unitBody()
    discarded()
    tryUnit
    compare((), 1, true)
    println(count)
  }
}
