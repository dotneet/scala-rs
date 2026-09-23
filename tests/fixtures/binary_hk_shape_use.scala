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
