package smnestedshape

import scala.language.implicitConversions

// Slick's Shape/Profile API reduced to the generic structure that matters for
// its separately compiled `(...).mapTo[CaseClass]` conversion.

class Rep[T](val name: String)
trait ShapeLevel
trait FlatShapeLevel extends ShapeLevel
class Shape[L <: ShapeLevel, -M, U, P](val show: String)

trait TupleShapeImplicits {
  implicit def tuple3Shape[L <: ShapeLevel, M1, M2, M3, U1, U2, U3, P1, P2, P3](implicit
      u1: Shape[_ <: L, M1, U1, P1],
      u2: Shape[_ <: L, M2, U2, P2],
      u3: Shape[_ <: L, M3, U3, P3]
  ): Shape[L, (M1, M2, M3), (U1, U2, U3), (P1, P2, P3)] =
    new Shape("triple")
}

object Shape extends TupleShapeImplicits {
  implicit def repShape[T, L <: ShapeLevel]: Shape[L, Rep[T], T, Rep[T]] =
    new Shape("rep")
}

class ShapedValue[T, U](val value: T, val shape: Shape[_ <: FlatShapeLevel, T, U, _]) {
  def describe: String = shape.show
}

trait BasicProfile {
  trait BasicAPI {
    implicit def anyToShapedValue[T, U](value: T)(implicit
        shape: Shape[? <: FlatShapeLevel, T, U, ?]
    ): ShapedValue[T, U] = new ShapedValue(value, shape)
  }
  val api: BasicAPI
}

object Profile extends BasicProfile {
  object API extends BasicAPI
  val api: BasicAPI = API
}
