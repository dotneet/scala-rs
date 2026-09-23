import slick.jdbc.H2Profile.api._

class OptionRow(tag: Tag) extends Table[Int](tag, "option_row") {
  def id = column[Int]("id")
  def * = id
}

object OptionFlatMap {
  // The result type is inferred from the view on the lambda body. Supplying
  // an explicit result type would decide B before the lambda is checked.
  def selected(row: Rep[Option[OptionRow]]) = row.flatMap(_.id)
}
