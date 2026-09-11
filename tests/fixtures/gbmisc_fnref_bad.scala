// The function prototype picks among the *argument's* alternatives only:
// no `ov3` takes an `Int`, and an overloaded callee whose alternatives
// disagree about the parameter gives no prototype at all. scalac rejects
// both calls, as does scala-rs.
object Main {
  def ov3(): Unit = ()
  def ov3(s: String): Unit = ()
  def h[U](f: Int => U): Unit = ()
  def h(s: String): Unit = ()
  def main(args: Array[String]): Unit = {
    List(1).foreach(ov3)
    h(println)
  }
}
