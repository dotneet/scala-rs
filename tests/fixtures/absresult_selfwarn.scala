class SelfWarnSafe(val n: Int) {
  def down(i: Int): Int = if (i == 0) n else down(i - 1)
  def other: SelfWarnSafe = new SelfWarnSafe(n + 1)
  def value: Int = n
  def indirect: Int = other.value
}
object Main {
  def main(args: Array[String]): Unit = {
    val s = new SelfWarnSafe(7)
    println(s.down(3))
    println(s.indirect)
  }
}
