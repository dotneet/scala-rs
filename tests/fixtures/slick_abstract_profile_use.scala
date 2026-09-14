import slick.lifted.TableQuery

class SlickAbstractProfileUse(val tdb: SlickAbstractProfileDb) {
  import tdb.profile.api._

  class Users(tag: Tag) extends Table[Int](tag, "users") {
    def id = column[Int]("id", O.PrimaryKey)
    def * = id
  }

  val users: TableQuery[Users] = TableQuery(new Users(_))
  val filtered = users.filter(_.id === 1)
}
