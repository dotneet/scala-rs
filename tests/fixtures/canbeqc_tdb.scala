import slick.basic.BasicProfile
import slick.relational.RelationalProfile

// This mirrors the TestKit's GenericTest[TDB] -> tdb.profile.api._ path:
// the profile is an abstract, narrowed type member rather than a concrete
// H2Profile imported directly at the use site.
trait CanBeQcBase {
  type Profile <: BasicProfile
  val profile: Profile
}

trait CanBeQcRelational extends CanBeQcBase {
  type Profile <: RelationalProfile
}

abstract class CanBeQcGeneric[TDB >: Null <: CanBeQcBase] {
  protected[this] var _tdb: TDB = null
  final lazy val tdb: TDB = _tdb
}

abstract class CanBeQcTdb extends CanBeQcGeneric[CanBeQcRelational] {
  import tdb.profile.api._

  class Users(tag: Tag) extends Table[Int](tag, "users") {
    def id = column[Int]("id")
    def * = id
  }

  val users = TableQuery[Users](t => new Users(t))
  val filtered = users.filter(_.id === 1)
}
