object Main {
  def main(args: Array[String]): Unit = {
    val n = 10
    def add(x: Int): Int = x + n
    println(add(1))
    val xs = 1 :: 2 :: Nil
    val ys = xs.map(add)
    ys.foreach((x: Int) => println(x))
    def fact(x: Int): Int =
      if (x <= 1) 1 else x * fact(x - 1)
    println(fact(5))
    def go(x: Int): Int = {
      if (x <= 0) 0
      else {
        val f = (k: Int) => go(k - 1)
        1 + f(x)
      }
    }
    println(go(3))
  }
}
