object data {
  implicit val api: API = new API {}
  import api._

  val table = new TableQuery[String]("data")
  val schema = table.schema
}
