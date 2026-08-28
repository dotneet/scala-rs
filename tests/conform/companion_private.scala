class Counter(val n: Int) {
  private val owned = 7
  def bump: Counter = new Counter(n + Counter.step)
  def secret: Int = Counter.hidden
}
object Counter {
  private val step: Int = 2
  private def hidden: Int = 41
  def peek(c: Counter): Int = c.owned
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new Counter(1).bump.n)
    println(new Counter(0).secret)
    println(Counter.peek(new Counter(5)))
  }
}
