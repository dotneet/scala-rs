import slick.basic.BasicProfile
import slick.relational.RelationalProfile

trait DdlBaseDB {
  type Profile <: BasicProfile
  val profile: Profile
}

trait DdlDB extends DdlBaseDB {
  type Profile <: RelationalProfile
}

abstract class DdlGeneric[T >: Null <: DdlDB] {
  protected[this] var _tdb: T = null
  final lazy val tdb: T = _tdb
}

abstract class DdlAsync[T >: Null <: DdlDB] extends DdlGeneric[T]

class DdlUse extends DdlAsync[DdlDB] {
  import tdb.profile.api._

  class Users(tag: Tag) extends Table[Int](tag, "users") {
    def id = column[Int]("id", O.PrimaryKey)
    def * = id
  }

  val users = TableQuery[Users](t => new Users(t))
  val schema = users.schema
  val createAction = schema.create
  val macroUsers = TableQuery[Users]
  val macroSchema = macroUsers.schema
  val macroCreateAction = macroSchema.create
}
