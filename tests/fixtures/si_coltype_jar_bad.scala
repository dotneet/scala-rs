// Reading a cake's abstract type member with its own type parameters is not
// the same as accepting anything under its name. Real scalac 2.13.16 rejects
// all three of these too; see `crates/cli/tests/slickimplicit.rs`.
import slick.jdbc.JdbcProfile

class Unmapped(val n: Int)

trait BadProfile {
  val profile: JdbcProfile
  import profile.api._

  // A `BaseColumnType[T]` is not satisfied by an arbitrary value.
  val wrong: BaseColumnType[String] = "not a column type"

  // The wrong number of arguments is still an arity error.
  val arity: BaseColumnType[String, Int] = null
}

trait BadComponent { self: BadProfile =>
  import profile.api._

  // Nothing declares a column type for `Unmapped`.
  class Uns(tag: Tag) extends Table[Unmapped](tag, "UNS") {
    val u = column[Unmapped]("U")
    def * = u
  }
}
