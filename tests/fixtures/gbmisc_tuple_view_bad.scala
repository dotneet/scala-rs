// A view whose result class merely has the tuple's arity is no view to a
// tuple: `SV[T, U]` unified with `(String, String)` by `T := String,
// U := String` in scala-rs's implicit unifier, so both lines below compiled
// (to an `SV` where a `Tuple2` was promised). scalac rejects both.
import scala.language.implicitConversions
trait Ev[T, U]
final case class SV[T, U](value: T, ev: Ev[T, U])
object Main {
  implicit def evSame[T]: Ev[T, T] = new Ev[T, T] {}
  implicit def toSV[T, U](v: T)(implicit ev: Ev[T, U]): SV[T, U] = SV(v, ev)
  def one(v: (String, String)): Int = 1
  def main(args: Array[String]): Unit = {
    val t: (String, String) = "Reopen"
    println(one("x"))
  }
}
