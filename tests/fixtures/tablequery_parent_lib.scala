package tablequeryparent

trait CanBeQueryCondition[-T]

object CanBeQueryCondition {
  implicit object booleanCondition extends CanBeQueryCondition[Boolean]
}

abstract class Query[+E, U, C[_]] {
  def filter[T](f: E => T)(implicit wt: CanBeQueryCondition[T]): Query[E, U, C] = this
}

abstract class AbstractTable[T] {
  type TableElementType
  def id: T
}

class Tag

class User(tag: Tag) extends AbstractTable[Int] {
  type TableElementType = Int
  def id: Int = 0
}

class TableQuery[E <: AbstractTable[?]](cons: Tag => E)
    extends Query[E, E#TableElementType, Seq]

trait UserTables {
  lazy val users: TableQuery[User] = new TableQuery[User](tag => new User(tag))
}

object UserTableCatalog extends UserTables
