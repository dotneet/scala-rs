import scala.util.{Try, Success, Failure}

object Main {
  def main(args: Array[String]): Unit = {
    val ok: Try[Int] = Try(10 / 2)
    val bad: Try[Int] = Try(1 / 0)
    println(ok.isSuccess)
    println(bad.isSuccess)
    println(ok.isFailure)
    println(bad.isFailure)
    println(ok.get)
    println(ok.getOrElse(0))
    println(bad.getOrElse(-1))
    println(ok.map((x: Int) => x + 1))
    println(bad.map((x: Int) => x + 1))
    println(ok.flatMap((x: Int) => Try(x * 2)))
    println(ok.filter((x: Int) => x > 1))
    println(ok.toOption)
    println(bad.toOption)
    println(ok.toEither)
    println(ok.orElse(Try(0)))
    println(bad.orElse(Try(7)))
    println(ok.fold((e: Throwable) => -1, (x: Int) => x))
    println(bad.fold((e: Throwable) => -1, (x: Int) => x))
    ok.foreach((x: Int) => println(x))
    println(bad.failed.isSuccess)
    println(ok.transform((x: Int) => Try(x * 3), (e: Throwable) => Try(0)))
    println(Success(2).getOrElse(0))
    println(Failure(new RuntimeException()).getOrElse(0))
  }
}
