import scala.language.implicitConversions
import io.circe.Encoder
import io.circe.generic.auto._

abstract class Box[A]
object Box {
  implicit def boxApply(x: Box[Int]): ((Int, Int) => Int) => Int = f => f(2, 3)
}
trait Low {
  implicit def response[A: Encoder](a: A): String => String = identity
}
object Main extends Low {
  def main(args: Array[String]): Unit = {
    val box = new Box[Int] {}
    println(box { (x: Int, y: Int) => x + y })
  }
}
