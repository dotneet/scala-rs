object Main {
  def fib(n: Int): Int = if (n < 2) n else fib(n - 1) + fib(n - 2)
  def main(args: Array[String]): Unit = {
    println(fib(10))
    println(1 + 2 * 3 - 4 / 2)
    println(1 + 2.5)
    println(2L + 1)
    println("a" + 1 + true)
    var i = 0
    var acc = 0
    while (i < 5) { val x = i * i; acc += x; i += 1 }
    println(acc)
    println(if (acc > 10) "big" else "small")
    println((1 to 5).map(_ * 2).mkString(","))
  }
}
