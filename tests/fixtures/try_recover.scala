import scala.util.Try

object Main {
  def main(args: Array[String]): Unit = {
    val ok: Try[Int] = Try(10 / 2)
    val bad: Try[Int] = Try(1 / 0)
    println(bad.recover { case _: ArithmeticException => -1 })
    println(ok.recover { case _: ArithmeticException => -1 })
    println(bad.recover { case _: NumberFormatException => -2 })
    println(bad.recoverWith { case _: ArithmeticException => Try(42) })
    println(ok.recoverWith { case _: ArithmeticException => Try(42) })
    println(ok.collect { case 5 => "five" })
    val pf: PartialFunction[Throwable, Int] = { case _: ArithmeticException => 99 }
    println(pf.isDefinedAt(new ArithmeticException()))
    println(bad.recover(pf))
  }
}
