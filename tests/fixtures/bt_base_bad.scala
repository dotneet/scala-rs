// The rejecting side of `bt_base.scala`: reading the argument through its base
// type must not make anything *fit* that did not.
//
// A view is still not inserted here -- the argument already conforms to
// `Rep[P2]` -- so the implicit clause is asked for the base type's own
// arguments, and there is no witness for either pair. Real scalac 2.13.16
// reports the same two errors.
import scala.language.implicitConversions

class Rep[T](val label: String)
class Lit[T](v: T) extends Rep[T]("lit:" + v)
trait OM[B1, P2, R] { def name: String }

class Col[B1](val col: String) {
  def ===[P2, R](e: Rep[P2])(implicit om: OM[B1, P2, R]): String =
    col + " = " + e.label + " via " + om.name
}

object Main {
  implicit def toRep[T](v: T): Rep[T] = new Lit[T](v)
  implicit val omLong: OM[Long, Long, Boolean] = new OM[Long, Long, Boolean] {
    def name = "omLong"
  }

  def main(args: Array[String]): Unit = {
    val id = new Col[Long]("ID")
    val nm = new Col[String]("NAME")
    // `Lit[String] <: Rep[String]`, so `P2 = String`, and no `OM[Long, String, R]`.
    println(id === new Lit[String]("x"))
    // The receiver's own `B1` is what has no witness here.
    println(nm === new Lit[Long](1L))
  }
}
