// `gzero_tupleview` in the place gitbucket writes it: `sortBy` wants a
// `T => slick.lifted.Ordered`, and slick's `Ordered.tuple2Ordered` supplies it
// for a pair whose first half already *is* an `Ordered` (`Predef.$conforms`)
// and whose second half needs the conversion `columnToOrdered`.
//
// Compiled against the slick 3.4.1 jar by both compilers.
import slick.jdbc.JdbcProfile

trait GzeroSortBy {
  val profile: JdbcProfile
  import profile.api._

  class T(tag: Tag) extends Table[(Int, Int, String)](tag, "T") {
    def a = column[Int]("A")
    def b = column[Int]("B")
    def c = column[String]("C")
    def * = (a, b, c)
  }
  val ts = TableQuery[T]

  def one = ts.sortBy(t => t.a.desc)
  def arrow = ts.sortBy(t => t.a.desc -> t.b)
  def pair = ts.sortBy(t => (t.a.desc, t.b))
  def triple = ts.sortBy(t => (t.a.desc, t.b, t.c.asc))
  def bothOrdered = ts.sortBy(t => (t.a.desc, t.b.asc))
  def bothPlain = ts.sortBy(t => (t.a, t.b))
}
