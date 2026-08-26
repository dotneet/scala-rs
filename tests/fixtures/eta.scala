object Main {
  def inc(x: Int): Int = x + 1
  def add(x: Int)(y: Int): Int = x + y
  def main(args: Array[String]): Unit = {
    println(add(1)(2))
    val xs = 1 :: 2 :: Nil
    val ys = xs.map(add(10))
    for (y <- ys) println(y)
    val zs = xs.map(inc)
    for (z <- zs) println(z)
    val ws = xs.map(inc _)
    for (w <- ws) println(w)
  }
}
