import scala.util.{Try, Success, Failure}
object Main {
  def main(a: Array[String]): Unit = {
    println(Try(1 / 0) match { case Success(v) => "ok" + v; case Failure(e) => "err " + e.getClass.getSimpleName })
    println(Try("5".toInt).getOrElse(0))
    println(Either.cond(true, 1, "no"))
    println(List(1,2,3).find(_ > 1))
    println(Right(1).map(_ + 1))
    println(scala.util.Using(new java.io.StringReader("x"))(r => r.read()).get)
  }
}
