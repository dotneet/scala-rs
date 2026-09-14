// Slick's Option-valued Shape derivations must resolve recursively. The
// outer Shape leaves its level, unpacked and packed positions open; the
// optionShape witness settles the latter two from its own Shape clause and
// chooses the declared ShapeLevel upper bound for the remaining level.
import slick.jdbc.H2Profile.api._

object OptionShapeProbe {
  implicitly[Shape[?, Rep[Option[Int]], ?, ?]]
  implicitly[Shape[?, Rep[Option[Option[Int]]], ?, ?]]
  implicitly[Shape[?, Rep[Option[(Rep[Int], Rep[String])]], ?, ?]]
}
