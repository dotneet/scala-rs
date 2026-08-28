object Main {
  def divide(a: Int, b: Int): Either[String, Int] =
    if (b == 0) Left("div0") else Right(a / b)

  def main(args: Array[String]): Unit = {
    val r: Either[String, Int] = divide(10, 2)
    val l: Either[String, Int] = divide(1, 0)
    println(r.isRight)
    println(r.isLeft)
    println(l.isRight)
    println(r.getOrElse(0))
    println(l.getOrElse(-1))
    println(r.map((x: Int) => x * 2))
    println(l.map((x: Int) => x * 2))
    println(r.flatMap((x: Int) => divide(x, 5)))
    println(r.fold((s: String) => s.length, (x: Int) => x))
    println(l.fold((s: String) => s.length, (x: Int) => x))
    println(r.swap)
    println(r.toOption)
    println(l.toOption)
    println(r.toSeq)
    println(r.contains(5))
    println(r.exists((x: Int) => x > 3))
    println(r.forall((x: Int) => x > 9))
    r.foreach((x: Int) => println(x))
    println(r.filterOrElse((x: Int) => x > 9, "small"))
    println(r.orElse(Right(0)))
    println(l.orElse(Right(0)))
  }
}
