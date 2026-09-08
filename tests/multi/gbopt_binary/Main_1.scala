import scala.concurrent.ExecutionContext.Implicits.global
import gboptlib.Entry._
import gboptlib.Box

object Main {
  def want[T](implicit b: Box[T]): String = b.name

  def main(args: Array[String]): Unit = {
    // Root 1.
    println(want[String])
    println(want[Option[String]])
    println(want[Option[Option[String]]])
    // Root 2. The second one also pins the *result*: `R` is decided by the
    // witness the second clause finds, so a wrong answer here would be a
    // wrong type with no diagnostic.
    println(7.tagIn(List(1, 2, 3)).text)
    val r: gboptlib.Res[Boolean] = 7.tagIn(List(4))
    println(r.text)
    // Root 3.
    println(Uses.twice)
    println(gboptlib.Slickish.later(40))
  }
}
