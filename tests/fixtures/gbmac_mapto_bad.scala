// `mapTo` calls real scalac 2.13.16 rejects, and scala-rs must reject too
// (`crates/cli/tests/gbmac.rs`). A mirror that answered approximately would
// let some of these through: each one's rejection depends on the macro
// seeing the row class the way nsc's typer does.
package gbmacbad

import slick.jdbc.H2Profile.api._

// The field types do not line up with the columns: `(Int, String)` columns
// mapped to a class whose fields are `(String, String)`.
case class Swapped(id: String, name: String)
// One column too few.
case class Wide(id: Int, name: String, extra: Boolean)
// Not a case class at all: `mapToImpl` aborts with its own message.
class NotCase(val id: Int, val name: String)

class Rows(tag: Tag) extends Table[Swapped](tag, "ROWS") {
  val id = column[Int]("ID")
  val name = column[String]("NAME")
  def * = (id, name).mapTo[Swapped]
  def wide = (id, name).mapTo[Wide]
  def plain = (id, name).mapTo[NotCase]
}
