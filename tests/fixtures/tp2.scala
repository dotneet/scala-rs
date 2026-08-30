// Two mixed-in traits each declare their own private method of the same
// name. Neither may leak an interface signature or a mixin forwarder onto
// `Both` -- if either does, one shadows the other (or a wrong invokestatic
// target class shows up) instead of each trait calling its own.
trait Alpha {
  private def helper: Int = 10
  def a: Int = helper
}
trait Beta {
  private def helper: Int = 20
  def b: Int = helper
}
class Both extends Alpha with Beta

object Main {
  def main(args: Array[String]): Unit = {
    val x = new Both
    println(x.a)
    println(x.b)
  }
}
