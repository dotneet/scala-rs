// The profile owner is declared in a later compilation unit. During the first
// signature round its inherited `profile` member is not complete yet, so the
// nested table's parent must be retried after all unit signatures are known.
import slick.model._

class SlickProfileNestedLate(val tdb: LateProbeDB) {
  import tdb.profile.api._

  class Users(tag: Tag) extends Table[Int](tag, "users") {
    def id = column[Int]("id", O.PrimaryKey)
    def * = id
  }
}
