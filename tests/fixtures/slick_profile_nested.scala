// Probe the path through a generic database value used by Slick TestKit.
// `Table` is deliberately also imported from slick.model in the outer scope.
import slick.jdbc.JdbcProfile
import slick.model._

trait ProbeDB {
  type Profile = JdbcProfile
  val profile: Profile
}

class SlickProfileNested(val tdb: ProbeDB) {
  import tdb.profile.api._

  class Users(tag: Tag) extends Table[Int](tag, "users") {
    def id = column[Int]("id", O.PrimaryKey)
    def * = id
  }
}
