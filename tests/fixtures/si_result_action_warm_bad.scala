import slick.jdbc.MySQLProfile.api._
object Main {
  class Rows(tag:Tag) extends Table[Int](tag, "rows") {
    def id = column[Int]("id")
    def * = id
  }
  val warm = TableQuery(new Rows(_)).result.headOption
  val invalid:slick.sql.SqlAction[String, slick.dbio.NoStream, slick.dbio.Effect] =
    sqlu"UPDATE rows SET id=1"
}
