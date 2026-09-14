package tablequeryprojected

final case class Repository(id: Int)

trait Profile

trait RepositoryComponent { self: Profile =>
  lazy val Repositories = TableQuery[Repositories]

  class Repositories(tag: Tag) extends Table[Repository](tag) {
    def id: Int = 0
  }
}

trait CoreProfile extends Profile with RepositoryComponent

object Catalog extends CoreProfile
