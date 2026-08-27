import scala.annotation.implicitNotFound

@implicitNotFound("no show for ${A}")
trait Show[A]

object Main {
  def show[A](implicit s: Show[A]): Int = 1
  def main(args: Array[String]): Unit = {
    val n: Int = show[Int]
  }
}
