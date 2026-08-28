object Main {
  def divide(a: Int, b: Int): Either[String, Int] =
    if (b == 0) Left("div0") else Right(a / b)

  def main(args: Array[String]): Unit = {
    val ok: Either[String, Int] =
      for { x <- divide(10, 2); y <- divide(x, 1) } yield x + y
    println(ok)
    val bad: Either[String, Int] =
      for { x <- divide(10, 0); y <- divide(x, 1) } yield x + y
    println(bad)
    val single: Either[String, Int] = for (x <- divide(9, 3)) yield x * 3
    println(single)
    val three: Either[String, Int] =
      for { a <- divide(100, 5); b <- divide(a, 2); c <- divide(b, 5) } yield a + b + c
    println(three)
  }
}
