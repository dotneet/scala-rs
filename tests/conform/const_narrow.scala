object Main {
  val b: Byte = 1
  val s: Short = 2
  val c: Char = 66
  def take(x: Byte): Byte = x
  def widen(x: Byte): Int = x + 0
  def main(args: Array[String]): Unit = {
    println(b); println(s); println(c)
    println(take(3)); println(widen(4))
  }
}
