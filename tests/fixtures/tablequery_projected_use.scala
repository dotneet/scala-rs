package tablequeryprojected

object TableQueryProjectedUse {
  val repository: Option[Repository] =
    Catalog.Repositories.filter(_.id == 1).firstOption
}
