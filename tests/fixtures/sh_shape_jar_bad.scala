// The two relaxations in `sh_shape_jar.scala` must not turn into accepting
// what nsc rejects.
//
//  * a projection slick has no `Shape` for is still a missing implicit, not a
//    witness picked because the wanted type's first argument is a wildcard;
//  * an operator called at an arity *no* conversion offers is still "is not a
//    member" -- narrowing the conversions by the argument count never widens
//    the set.
import slick.jdbc.H2Profile.api._

class NoShape(val x: Int)

trait RichColumnsBad {
  implicit class RichColumn(c1: Rep[Boolean]) {
    def &&(c2: => Rep[Boolean], guard: => Boolean): Rep[Boolean] = if (guard) c1 && c2 else c1
  }
}

class BadIssues(tag: Tag) extends Table[(Long, String)](tag, "ISSUES") {
  def id = column[Long]("ID")
  def title = column[String]("TITLE")
  def * = (id, title)
}

object ShapeJarBad extends RichColumnsBad {
  val q = TableQuery[BadIssues](t => new BadIssues(t))

  val nope = q.map(t => new NoShape(1))

  val badArity =
    (q.baseTableRow.id === 1L).&&(q.baseTableRow.title === "x", true, false)
}
