package compiledquery

trait ShapeLevel

abstract class Shape[Level <: ShapeLevel, -Mixed_, Unpacked_, Packed_] {
  type Packed = Packed_
}

final class Parameters[PU, PP](pShape: Shape[ShapeLevel, PU, PU, ?]) {
  def withFilter(f: PP => Boolean): Parameters[PU, PP] = this
}

final class Selected[T](val value: T)

object ownerone {
  abstract class Shape { type Packed = String }
}

object ownertwo {
  abstract class Shape { type Packed = Int }
}

object OwnerProbe {
  def first(p1: ownerone.Shape, p2: ownertwo.Shape): Selected[p1.Packed] =
    new Selected[p1.Packed](null.asInstanceOf[p1.Packed])
}

object inherited {
  abstract class Base { type Packed }
  final class Fixed extends Base { type Packed = String }
  def select(p: Fixed): Selected[p.Packed] =
    new Selected[p.Packed](null.asInstanceOf[p.Packed])
}

object recursive {
  trait Loop[A]
  object Loop {
    implicit def loop[A](implicit ev: Loop[A]): Loop[A] = ???
  }
}

object Shape {
  implicit def tuple2Shape[A, B](implicit
      a: Shape[ShapeLevel, A, A, A],
      b: Shape[ShapeLevel, B, B, B]
  ): Shape[ShapeLevel, (A, B), (A, B), (A, B)] =
    new Shape[ShapeLevel, (A, B), (A, B), (A, B)] {}
}

object Parameters {
  def apply[U](implicit pShape: Shape[ShapeLevel, U, U, ?]): Parameters[U, pShape.Packed] =
    new Parameters[U, pShape.Packed](pShape)

  // Keep both formal paths in the classfile. A reader that indexes these by
  // the shared Shape class (or by HashMap iteration) can silently return the
  // second parameter's projection for firstPacked.
  def firstPacked[A](p1: Shape[ShapeLevel, A, A, ?], p2: Shape[ShapeLevel, A, A, ?]): Selected[p1.Packed] =
    new Selected[p1.Packed](null.asInstanceOf[p1.Packed])
  def secondPacked[A](p1: Shape[ShapeLevel, A, A, ?], p2: Shape[ShapeLevel, A, A, ?]): Selected[p2.Packed] =
    new Selected[p2.Packed](null.asInstanceOf[p2.Packed])
}
