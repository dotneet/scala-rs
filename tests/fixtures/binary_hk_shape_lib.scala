package binaryshape

package object identity {
  type Id[A] = A
}

import slick.lifted.{CaseClassShape, FlatShapeLevel, Rep, Shape}
import scala.reflect.ClassTag

trait TupledApply[Tuple[_[_]], Record[_[_]]] {
  def tupledApply[F[_]](tuple: Tuple[F]): Record[F]
}

abstract class RecordShape[RecordTuple[_[_]], Record[_[_]] <: Product](
  apply: TupledApply[RecordTuple, Record]
)(implicit
  tupleShape: Shape[
    FlatShapeLevel,
    RecordTuple[Rep],
    RecordTuple[identity.Id],
    RecordTuple[Rep]
  ],
  classTag: ClassTag[Record[identity.Id]]
) extends CaseClassShape[
  Product,
  RecordTuple[Rep],
  Record[Rep],
  RecordTuple[identity.Id],
  Record[identity.Id]
](
  apply.tupledApply[Rep],
  apply.tupledApply[identity.Id]
)
