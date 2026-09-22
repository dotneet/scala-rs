object PrototypeMergeBad {
  class Box[A](val a: A)
  def wrap[A](a: A): Box[A] = new Box(a)
  val broad: Any = 1
  val invalid: Box[String] = wrap(broad)
}
