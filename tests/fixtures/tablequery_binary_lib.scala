package tablequerybinary

abstract class Query[+E, U, C[_]] {
  def filter(predicate: E => Boolean): Query[E, U, C] = this
  def firstOption: Option[U] = None
  def insert(value: U): Int = 1
}

abstract class AbstractTable[T] {
  type TableElementType
}

class User
class Account

trait RelationalTableComponent {
  abstract class Table[T] extends AbstractTable[T] {
    type TableElementType = T
  }
}

trait Profile extends RelationalTableComponent

trait UserComponent { self: Profile =>
  class Accounts extends Table[Account]
  class Users extends Table[User]
  val accounts: TableQuery[Accounts] = null
  val users: TableQuery[Users] = null
}

class TableQuery[E <: AbstractTable[_]]
    extends Query[E, E#TableElementType, Seq]

object Catalog extends Profile with UserComponent
