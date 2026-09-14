// A minimal Slick mapTo call whose receiver is itself a generated mapping.
// Keep the row classes and table local, like JdbcMapperTest's wide mapping:
// nsc then emits the same anonymous converter/ExistentialTypeTree shape while
// avoiding a top-level local-class specialization diagnostic.
import slick.jdbc.H2Profile.api._

object Main {
  def mapped(tag: Tag) = {
    case class PrefixPart(a: Int, b: Int)
    case class PrefixWhole(id: Int, p1: PrefixPart, p2: PrefixPart)

    class PrefixTable(tag: Tag) extends Table[PrefixWhole](tag, "PREFIX_ROW") {
      val id = column[Int]("ID")
      val p1a = column[Int]("P1A")
      val p1b = column[Int]("P1B")
      val p2a = column[Int]("P2A")
      val p2b = column[Int]("P2B")
      def * = (
        id,
        (p1a, p1b).mapTo[PrefixPart],
        (p2a, p2b).mapTo[PrefixPart]
      ).mapTo[PrefixWhole]
    }

    new PrefixTable(tag).*
  }

  def main(args: Array[String]): Unit = println("prefix")
}
