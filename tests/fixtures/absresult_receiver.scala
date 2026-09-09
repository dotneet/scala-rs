trait Cursor[+A] { def next(): A }
object Cursor {
  def empty[T]: Cursor[T] = new Cursor[T] { def next() = throw new NoSuchElementException }
}
class Values[A](a: A) extends Cursor[A] {
  def next() = if (true) a else Cursor.empty.next()
}
object Main { def main(args: Array[String]): Unit = { println(new Values("value").next()); println(ReceiverArguments.value) } }
class Cell[A] { def take(a: A): A = a }
object Cells { def empty[A]: Cell[A] = new Cell[A] }
object ReceiverArguments { def value: String = Cells.empty.take("argument") }
