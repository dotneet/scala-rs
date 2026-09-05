// The rejecting side of `tq_openview.scala`: relaxing which of the callee's
// type parameters a view has to settle must not make an inapplicable call
// applicable.
//
// Real scalac 2.13.16 rejects this too -- there is no `OM[Long, Long, R]` in
// scope, so no `R` exists for the result, and the view that makes the argument
// fit cannot supply one.
import scala.language.implicitConversions

class Rep[T](val label: String)
class Lit[T](v: T) extends Rep[T]("lit:" + v)
trait OM[B1, P2, R]

class Col[B1](val col: String) {
  def ===[P2, R](e: Rep[P2])(implicit om: OM[B1, P2, R]): String = col
}

object Main {
  implicit def toRep[T](v: T): Rep[T] = new Lit[T](v)
  // Deliberately for a different `B1`, so nothing answers `OM[Long, Long, R]`.
  implicit val omStr: OM[String, String, Int] = new OM[String, String, Int] {}

  def main(args: Array[String]): Unit = {
    val id = new Col[Long]("ID")
    println(id === 1L)
  }
}
