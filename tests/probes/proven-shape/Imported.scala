import slick.lifted.ProvenShape.proveShapeOf
import slick.jdbc.H2Profile.api._
class Rows(t: Tag) extends Table[Int](t, "ROWS") {
 def id = column[Int]("ID")
 def * : slick.lifted.ProvenShape[Int] = id
}
object Main {
 val rows = TableQuery[Rows]((t: Tag) => new Rows(t))
 val q = rows.joinLeft(rows).on((a,b) => a.id === b.id)
 def main(args: Array[String]): Unit = println(q.result.statements.head)
}
