package tablequeryprojected

trait CanBeQueryCondition[-T]

object CanBeQueryCondition {
  implicit object booleanCondition extends CanBeQueryCondition[Boolean]
}

abstract class Query[+E, U, C[_]] {
  def filter[T](f: E => T)(implicit wt: CanBeQueryCondition[T]): Query[E, U, C] = this
  def firstOption: Option[U] = None
}

abstract class AbstractTable[T] {
  type TableElementType
}

class Tag

abstract class Table[T](tag: Tag) extends AbstractTable[T] {
  type TableElementType = T
}

class TableQuery[E <: AbstractTable[_]](cons: Tag => E)
    extends Query[E, E#TableElementType, Seq]

object TableQuery {
  def apply[E <: AbstractTable[_]]: TableQuery[E] = null
}
