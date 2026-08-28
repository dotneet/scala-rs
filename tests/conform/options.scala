object Main {
  def parse(s: String): Option[Int] = if (s.forall(_.isDigit) && s.nonEmpty) Some(s.toInt) else None
  def main(args: Array[String]): Unit = {
    println(parse("12"))
    println(parse("x"))
    println(parse("12").map(_ * 2))
    println(parse("x").getOrElse(-1))
    println(parse("3").filter(_ > 5))
    println(parse("7").exists(_ > 5))
    println(List("1", "x", "3").flatMap(parse))
    val r = for { a <- parse("2"); b <- parse("3") } yield a + b
    println(r)
    println(parse("4").fold(0)(_ + 1))
    println(Some(1).toList)
  }
}
