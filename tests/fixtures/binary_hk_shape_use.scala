import binaryshape.identity.Id
import slick.jdbc.H2Profile.api._
import slick.lifted.CaseClassShape

case class Record[F[_]](id: F[Int], label: F[String])

object Record {
  type Tuple[F[_]] = (F[Int], F[String])
}

object BinaryShapeUse {
  type Parent = CaseClassShape[
    Product,
    Record.Tuple[Rep],
    Record[Rep],
    Record.Tuple[Id],
    Record[Id]
  ]

  def parent(shape: binaryshape.RecordShape[Record.Tuple, Record]): Parent = shape
}

object RecordShapeInstance {
  implicit object Shape extends binaryshape.RecordShape(
    new binaryshape.TupledApply[Record.Tuple, Record] {
      override def tupledApply[F[_]](tuple: Record.Tuple[F]): Record[F] =
        (Record.apply[F] _).tupled(tuple)
    }
  )
}

class NestedShape {
  case class Entry[F[_]](id: F[Int], label: F[String])

  object Entry {
    type Tuple[F[_]] = (F[Int], F[String])

    implicit object Shape extends binaryshape.RecordShape(
      new binaryshape.TupledApply[Tuple, Entry] {
        override def tupledApply[F[_]](tuple: Tuple[F]): Entry[F] =
          (Entry.apply[F] _).tupled(tuple)
      }
    )
  }

  val direct: slick.lifted.Shape[
    slick.lifted.FlatShapeLevel, Entry[Rep], Entry[Id], Entry[Rep]
  ] = Entry.Shape

  val inferred: slick.lifted.Shape[
    slick.lifted.FlatShapeLevel, Entry[Rep], Entry[Id], Entry[Rep]
  ] = implicitly
}
