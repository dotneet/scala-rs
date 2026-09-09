class Cell[A] { def take(a: A): A = a }
object Cells { def empty[A]: Cell[A] = new Cell[A] }
object Bad {
  def explicit: String = Cells.empty[Int].take("wrong")
  def rigid[A](cell: Cell[A]): A = cell.take("wrong")
}
