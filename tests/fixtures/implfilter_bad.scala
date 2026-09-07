// The pre-test only removes candidates the fit would have rejected anyway, so
// a program that has no witness is still an error -- and the two "no"s it is
// allowed to reach on its own have to be the right ones.
//
// `Opaque` is not a `Rep` and not a tuple, so no `Shape` answers it; and a
// `Digits` in scope is a *supertype* of the wanted `Hex`, which is exactly the
// direction the structural test is entitled to reject. Real scalac rejects
// both.
import scala.language.implicitConversions

trait Digits { def digits: String }
class Hex extends Digits { def digits = "0123456789abcdef" }

trait Level
trait Flat extends Level

class Rep[T](val name: String)
class Opaque
class Shape[L <: Level, -M, U, P](val show: String)

object Shape {
  implicit def repShape[T, L <: Level]: Shape[L, Rep[T], T, Rep[T]] =
    new Shape("rep")

  implicit def tuple2Shape[L <: Level, M1, M2, U1, U2, P1, P2](implicit
      u1: Shape[_ <: L, M1, U1, P1],
      u2: Shape[_ <: L, M2, U2, P2]
  ): Shape[L, (M1, M2), (U1, U2), (P1, P2)] =
    new Shape("(" + u1.show + "," + u2.show + ")")
}

object Main {
  implicit val someDigits: Digits = new Hex

  def summon[T](implicit e: T): T = e

  def mapped[F, T, G](f: F)(implicit s: Shape[_ <: Flat, F, T, G]): String =
    s.show

  def main(args: Array[String]): Unit = {
    println(mapped(new Opaque))
    println(summon[Hex].digits)
  }
}
