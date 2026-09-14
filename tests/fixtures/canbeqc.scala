// Minimal Slick query-condition regression: a typed comparison inside a
// Query.filter must infer Rep[Boolean], then summon CanBeQueryCondition for it.
import slick.jdbc.H2Profile.api._

object CanBeQcProbe {
  class Users(tag: Tag) extends Table[(Int, String)](tag, "users") {
    def id = column[Int]("id", O.PrimaryKey)
    def name = column[String]("name")
    def * = (id, name)
  }

  val users = TableQuery[Users](t => new Users(t))
  val filtered = users.filter(_.id === 1)
  val filteredExplicit = users.filter(u => u.id === 1)
}

trait CanBeQcDb {
  val profile: slick.jdbc.H2Profile
  import profile.api._

  class ScopedUsers(tag: Tag) extends Table[(Int, String)](tag, "users") {
    def id = column[Int]("id", O.PrimaryKey)
    def name = column[String]("name")
    def * = (id, name)
  }
  val scopedUsers = TableQuery[ScopedUsers](t => new ScopedUsers(t))
  val scopedFiltered = scopedUsers.filter(_.id === 1)
}
