import slick.jdbc.H2Profile.api._

object MatchView {
  def predicate(col: Rep[Option[Int]], enabled: Boolean, selected: Boolean) =
    col.nonEmpty && ((enabled, selected) match {
      case (true, true) => true
      case (true, false) => col.isEmpty
      case (false, true) => col.nonEmpty
      case (false, false) => false
    })
}
