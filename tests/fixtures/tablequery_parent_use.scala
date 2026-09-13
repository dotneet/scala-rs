import tablequeryparent._

object TableQueryParentUse {
  val tables = new TableQuery[User](tag => new User(tag))
  val filtered = tables.filter(_.id == 1)
  val explicit = tables.filter((user: User) => user.id == 1)
  val typed: Query[User, Int, Seq] = tables.filter(_.id == 1)
  val inheritedLazy = UserTableCatalog.users.filter(_.id == 1)
}
