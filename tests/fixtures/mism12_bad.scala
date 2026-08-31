// None of the four changes makes the typer more permissive. nsc 2.13.16
// rejects every one of these, with the same four messages.

trait Box[+X] {
  def head: X
}
final class One[+X](val head: X) extends Box[X]

trait Bx[X]
case class SV[T, U](a: T, b: Bx[U]) {
  // The class's `U` is fixed here; `apply`'s is not the same parameter, and a
  // `Bx[Int]` is still not a `Bx[U]`.
  def wrong: SV[T, U] = SV(a, new Bx[Int] {})
}

trait Lvl
trait Flat extends Lvl
abstract class Shp[L <: Lvl, -Mixed, Unpacked, Packed]
class Rp[T]
class ConstCol[T] extends Rp[T]

trait RepShapes {
  implicit def repShape[T, L <: Lvl]: Shp[L, Rp[T], T, Rp[T]] = ???
}
object Shp extends RepShapes

object Main {
  // The bound really is `Box[A]`, so the element is an `Int` and `.length` is
  // not one of its members.
  def bad1[A, M[+X] <: Box[X]](m: M[Int]): Int = m.head.length

  // An inherited companion implicit is still only found for a type it fits:
  // `String` is not a `Rp[_]`.
  def bad2: Shp[Flat, String, String, Rp[String]] = implicitly

  // The eta-expansion has no `(Boolean)BigDecimal` alternative.
  def bad3: Boolean => BigDecimal = BigDecimal.apply
}
