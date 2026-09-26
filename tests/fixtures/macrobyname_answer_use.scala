import bynameanswer._
object Main {
  def main(args: Array[String]): Unit =
    println(Untyped[Deferred[Int]].value)
}
