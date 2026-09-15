// Slick's Option-valued Shape derivations must resolve recursively. The
// outer Shape leaves its level, unpacked and packed positions open; the
// optionShape witness settles the latter two from its own Shape clause and
// chooses the declared ShapeLevel upper bound for the remaining level.
import slick.jdbc.H2Profile.api._

object OptionShapeProbe {
  implicitly[Shape[?, Rep[Option[Int]], ?, ?]]
  implicitly[Shape[?, Rep[Option[Option[Int]]], ?, ?]]
  implicitly[Shape[?, Rep[Option[(Rep[Int], Rep[String])]], ?, ?]]

  // Query.map must carry the recursive optionShape through its result Shape;
  // the packed projection grows while the scalar input is lifted twice.
  final class X(tag: Tag) extends Table[(Int, String, Option[Int])](tag, "X_OPT") {
    def a = column[Int]("A")
    def b = column[String]("B")
    def c = column[Option[Int]]("C")
    def * = (a, b, c)
  }
  val source: Query[X, (Int, String, Option[Int]), Seq] = ???
  val nested = source.map(value => Rep.Some(Rep.Some(value.a)))
  val nestedTyped: Query[Rep[Option[Option[Int]]], ?, Seq] = nested
  val nestedColumn = source.map(value => Rep.Some(value.c))
  val nestedColumnTyped: Query[Rep[Option[Option[Int]]], ?, Seq] = nestedColumn
}
