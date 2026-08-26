object Main {
  var n: Int = 0
  def twice(x: => Int): Int = x + x
  def main(args: Array[String]): Unit = {
    val r = twice({ n = n + 1; 3 })
    println(r)
    println(n)
  }
}
