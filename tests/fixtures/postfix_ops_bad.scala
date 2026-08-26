import scala.language.implicitConversions
object Main {
  implicit class Bang(n: Int) {
    def bang: Int = n + 1
  }
  def main(args: Array[String]): Unit = {
    val n: Int = 42 bang
  }
}
