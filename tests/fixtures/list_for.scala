object Main {
  def main(args: Array[String]): Unit = {
    val xs = 1 :: 2 :: 3 :: Nil
    val ys = for (x <- xs) yield x + 1
    for (y <- ys) println(y)
    val zs = for (x <- xs if x > 1) yield x * 10
    for (z <- zs) println(z)
  }
}
