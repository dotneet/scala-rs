import slick.jdbc.MySQLProfile.api._
import slick.lifted.MappedProjection
case class Row(id: Int)
class Rows(tag: Tag) extends Table[Row](tag, "rows") {
  def id = column[Int]("id")
  def name = column[String]("name", O.Length(32, varying = true))
  def * = id <> (Row.apply, Row.unapply)
  def optional: MappedProjection[Option[Row], Option[Int]] = Rep.Some(id).shaped.<>(r => r.map(i => Row(i)), (_: Any) => throw new Exception("unused"))
}
object Main {
  def main(args: Array[String]): Unit = {
    val t = TableQuery[Rows](tag => new Rows(tag)).baseTableRow
    println(t.optional.toNode.nodeType)
  }
}
