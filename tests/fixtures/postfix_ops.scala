import scala.language.implicitConversions
import scala.language.postfixOps
object Main {
  implicit class Bang(n: Int) {
    def bang: Int = n + 1
  }
  def main(args: Array[String]): Unit = {
    println(42 bang)
  }
}
