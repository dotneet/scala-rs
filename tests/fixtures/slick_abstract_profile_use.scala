class SlickAbstractProfileUse(val tdb: SlickAbstractProfileDb) {
  import tdb.profile.api._

  class Users(tag: Tag) extends Table[Int](tag, "users") {
    def id = column[Int]("id", O.PrimaryKey)
    def * = id
  }

  val users: TableQuery[Users] = TableQuery(new Users(_))
  val filtered = users.filter(_.id === 1)

  val explicitUsers = TableQuery[Users](new Users(_))
  val explicitFiltered = explicitUsers.filter(_.id === 4)

  val inferredUsers = TableQuery(new Users(_))
  val inferredFiltered = inferredUsers.filter(_.id === 2)

  object userRows extends TableQuery(new Users(_))
  val objectFiltered = userRows.filter(_.id === 3)

  val anonymousRows = new TableQuery(new Users(_)) {}
  val anonymousFiltered = anonymousRows.filter(_.id === 5)
}
