object Main {
  def main(a: Array[String]): Unit = {
    val e1: Either[String, Int] = Right(2)
    val r = for { x <- e1; y <- (Left("no"): Either[String, Int]).orElse(Right(3)) } yield x + y
    println(r)
    val o = for { x <- Option(1); if x > 0; y <- Option(2) } yield x + y
    println(o)
    val l = for { x <- List(1,2); if x % 2 == 0; y <- List(10,20) } yield x * y
    println(l)
    val t = for { (k, v) <- Map("a" -> 1) } yield s"$k=$v"
    println(t)
    val z = for { x <- 1 to 3; y <- x to 3 } yield (x, y)
    println(z.toList)
    val n = for { Some(x) <- List(Some(1), None, Some(3)) } yield x
    println(n)
  }
}
