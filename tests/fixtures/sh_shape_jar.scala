// slick's `Shape` implicits, and an operator that two conversions offer.
// Compiled against the published `slick_2.13-3.4.1.jar`; see
// `crates/cli/tests/slickshape.rs`.
//
// `Query.map` is
//
//   def map[F, G, T](f: E => F)(implicit shape: Shape[_ <: FlatShapeLevel, F, T, G]): Query[G, T, C]
//
// so `T` and `G` are undetermined and only the witness can say what they are.
// Two things had to be true before `repColumnShape` could answer:
//
//  1. the existential's *bound* has to survive the pickle. Read as a bare `_`
//     it left the candidate's own `Level <: ShapeLevel` with nothing opposite
//     it, and a candidate with an unsolved type parameter is dropped.
//  2. `FlatShapeLevel`'s parents have to have been read before
//     `candidate_bounds_hold` asks whether the solution is a `ShapeLevel`.
//     Nothing in this file names `FlatShapeLevel`, so nothing had warmed it.
//
// `q.sortBy(_.id)` has no implicit clause at all and worked throughout, which
// is what said the receiver was not the problem.
//
// `RichColumn` is gitbucket's, and is why `&&` is here: its two-argument `&&`
// ties with slick's one-argument one on every comparison in the project.
import slick.jdbc.H2Profile.api._

class Issues(tag: Tag) extends Table[(Long, String)](tag, "ISSUES") {
  def id = column[Long]("ID")
  def title = column[String]("TITLE")
  def * = (id, title)
}

trait RichColumns {
  implicit class RichColumn(c1: Rep[Boolean]) {
    def &&(c2: => Rep[Boolean], guard: => Boolean): Rep[Boolean] = if (guard) c1 && c2 else c1
  }
}

object ShapeJar extends RichColumns {
  val q = TableQuery[Issues](t => new Issues(t))

  // Not annotated: what a `map` off a `TableQuery` reports as its own type is
  // a separate gap (it answers `TableQuery[G]` where nsc says
  // `Query[G, T, Seq]`), and this fixture is about the witness being found at
  // all.
  val sorted = q.sortBy(_.id)
  val titles = q.map(_.title)
  val ids = q.map(_.id)
  val pairs = q.map(t => (t.id, t.title))

  val filtered = q.filter(t => (t.id === 1L) && (t.title === "x"))

  // The two-argument alternative is still reachable; narrowing by the
  // argument count must pick it here.
  val guarded: Rep[Boolean] = (q.baseTableRow.id === 1L).&&(q.baseTableRow.title === "x", true)
}
