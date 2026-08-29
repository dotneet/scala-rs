// `_*` is only allowed in last position.
object Main {
  def starNotLast(xs: List[Int]): Int = xs match {
    case List(rest @ _*, a) => a
    case _ => 0
  }
  def main(args: Array[String]): Unit = println(starNotLast(Nil))
}
