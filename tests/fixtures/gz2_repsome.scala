// gitbucket `Repository.scala:77` without gitbucket: inside a slick `Table`
// -- a subclass of `AbstractTable`, which is a subclass of `slick.lifted.Rep`
// -- the `Some(...)` of `.shaped.<>(…, r => Some(…))` must be `scala.Some`.
// scalac never offers `object Rep`'s members unqualified here: its static
// forwarders on `Rep.class` belong to the companion object, and static members
// are not inherited.
import slick.jdbc.H2Profile.api._

case class Gz2Row(name: String, size: Int, note: Option[String])

class Gz2Rows(tag: Tag) extends Table[Gz2Row](tag, "GZ2") {
  def name = column[String]("NAME")
  def size = column[Int]("SIZE")
  def note = column[String]("NOTE")
  def * = (name, size, note.?).shaped.<>(
    { case (n, s, o) => Gz2Row(n, s, o) },
    { (r: Gz2Row) => Some((r.name, r.size, r.note)) }
  )
}

object Gz2RepSome {
  val rows = TableQuery[Gz2Rows]
}
