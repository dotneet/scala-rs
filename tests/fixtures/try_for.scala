import scala.util.Try

object Main {
  def main(args: Array[String]): Unit = {
    val ok: Try[Int] = Try(10 / 2)
    val sum = for { a <- ok; b <- Try(a + 1) } yield a + b
    println(sum)
    val fail = for { a <- ok; b <- Try(a / 0) } yield a + b
    println(fail)
    val keep = for { x <- ok if x > 1 } yield x * 2
    println(keep)
    val drop = for { x <- ok if x > 9 } yield x * 2
    println(drop)
    val guarded = for { a <- ok; b <- Try(a + 1) if b > 3 } yield a * b
    println(guarded)
    ok.withFilter((x: Int) => x > 1).foreach((x: Int) => println(x))
  }
}
