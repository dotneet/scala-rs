import scala.util.Try

object Main {
  def main(args: Array[String]): Unit = {
    val t: Try[Int] = Try(1)
    // `recover` takes a PartialFunction[Throwable, U], not a total Int => Int.
    println(t.recover((x: Int) => x + 1))
  }
}
