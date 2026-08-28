import scala.util.Try

object Main {
  def main(args: Array[String]): Unit = {
    val t: Try[Int] = Try(1)
    println(t.noSuchTryMember)
  }
}
