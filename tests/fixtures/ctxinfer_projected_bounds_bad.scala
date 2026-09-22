trait Row { type Field }
class Table[A] extends Row { type Field = A }
object Main {
  def store[T <: Row, R <: T#Field](row: T, value: R): R = value
  val wrongMethod = store[Table[String], Int](new Table[String], 1)
}
