trait Row { type Field }
class Table[A] extends Row { type Field = A }
class Stored[T <: Row, R <: T#Field](val row: T, val value: R)
object Main {
  def store[T <: Row, R <: T#Field](row: T, value: R): R = value
  def main(args: Array[String]): Unit = {
    val row = new Table[String]
    println(store(row, "inferred"))
    println(store[Table[String], String](row, "explicit"))
    println(new Stored[Table[String], String](row, "constructor").value)
  }
}
