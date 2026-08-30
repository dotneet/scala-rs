import scala.util.{Try, Success, Failure}
object Main {
  def parse(s: String): Try[Int] = Try(s.toInt)
  def main(a: Array[String]): Unit = {
    println(parse("42"))
    println(parse("x").isFailure)
    println(parse("1").flatMap(x => parse("2").map(y => x + y)))
    println(List("1","2","z").map(parse).collect { case Success(v) => v })
    println(parse("z").recover { case _: NumberFormatException => -1 }.get)
    println(parse("5").toOption, parse("z").toOption)
    println(parse("5").getOrElse(0), parse("z").getOrElse(0))
    parse("z") match { case Success(v) => println(v); case Failure(e) => println(e.getClass.getSimpleName) }
    println(parse("7").fold(_ => 0, identity))
  }
}
