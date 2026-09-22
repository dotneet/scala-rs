import slick.jdbc.MySQLProfile.api._
object Main {
  implicit val ec:scala.concurrent.ExecutionContext = scala.concurrent.ExecutionContext.global
  class Rows(tag:Tag) extends Table[Int](tag, "rows") {
    def id = column[Int]("id")
    def * = id
  }
  val warm = TableQuery(new Rows(_)).result.headOption
  def action(db:Database) = db.run((for {
    _ <- sqlu"UPDATE rows SET id=1"
    id <- sql"SELECT id FROM rows".as[Int].head
  } yield id).transactionally)
}
