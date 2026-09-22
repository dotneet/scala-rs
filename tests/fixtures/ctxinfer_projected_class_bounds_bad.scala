trait Row { type Field }
class Table[A] extends Row { type Field = A }
class Stored[T <: Row, R <: T#Field](val row: T, val value: R)
object Main {
  val wrong = new Stored[Table[String], Int](new Table[String], 1)
}
