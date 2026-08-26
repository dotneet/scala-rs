class Counter(start: Int) {
  var n: Int = start
  def inc(): Unit = { n = n + 1 }
  def get(): Int = n
}

object Main {
  def main(args: Array[String]): Unit = {
    val c = new Counter(10)
    c.inc()
    c.inc()
    println(c.get())
  }
}
