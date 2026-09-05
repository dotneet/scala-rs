// Letting a candidate's own implicit clause settle a type parameter the
// wanted type leaves open only ever *adds* a way to solve one. A derivation
// whose clause has no witness is still not a candidate, and a wanted type no
// rule answers is still "could not find implicit value". Real scalac 2.13.16
// rejects both of these too; see `crates/cli/tests/slickimplicit.rs`.
class Rep[T](val name: String)

trait ShapeLevel
trait FlatShapeLevel extends ShapeLevel

class Shape[Level <: ShapeLevel, -M, U, P](val show: String)

object Shape {
  implicit def repShape[T, L <: ShapeLevel]: Shape[L, Rep[T], T, Rep[T]] =
    new Shape("rep")

  implicit def tuple2Shape[L <: ShapeLevel, M1, M2, U1, U2, P1, P2](implicit
      u1: Shape[_ <: L, M1, U1, P1],
      u2: Shape[_ <: L, M2, U2, P2]
  ): Shape[L, (M1, M2), (U1, U2), (P1, P2)] =
    new Shape("(" + u1.show + "," + u2.show + ")")
}

object Bad {
  def summon[T](implicit e: T): T = e

  // `String` is not a `Rep`, so `tuple2Shape`'s second clause has no witness
  // and the derivation is not usable.
  val one: Shape[_ <: FlatShapeLevel, (Rep[String], String), (String, String), _] = summon

  // Nothing at all answers a bare `Rep` triple: there is no `tuple3Shape`.
  val two: Shape[_ <: FlatShapeLevel, (Rep[String], Rep[Int], Rep[Int]), (String, Int, Int), _] =
    summon

  // The unpacked type has to match what the witness says it is.
  val three: Shape[_ <: FlatShapeLevel, (Rep[String], Rep[Int]), (Int, String), _] = summon
}
