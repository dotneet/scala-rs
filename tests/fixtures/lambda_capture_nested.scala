final class Cursor2(val value: String)

final class Holder2 {
  private case class Row(text: String)

  def rows(cursor: Cursor2): Row = {
    val make = (_: Int) => Row(cursor.value)
    make(1)
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Holder2().rows(new Cursor2("x")).text)
  }
}
