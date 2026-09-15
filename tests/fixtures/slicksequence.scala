import slick.basic.BasicProfile
import slick.jdbc.JdbcProfile
import slick.relational.RelationalProfile
import slick.sql.SqlProfile

trait SequenceBaseDB {
  type Profile <: BasicProfile
  val profile: Profile
}

trait SequenceRelDB extends SequenceBaseDB {
  type Profile <: RelationalProfile
}

trait SequenceSqlDB extends SequenceRelDB {
  type Profile <: SqlProfile
}

trait SequenceJdbcDB extends SequenceSqlDB {
  type Profile = JdbcProfile
}

abstract class SequenceGeneric[T >: Null <: SequenceBaseDB] {
  protected[this] var _tdb: T = null
  final lazy val tdb: T = _tdb
}

abstract class SequenceAsync[T >: Null <: SequenceBaseDB] extends SequenceGeneric[T]

class SequenceUse extends SequenceAsync[SequenceJdbcDB] {
  import tdb.profile.api._

  val sequence = Sequence[Int]("sequence") start 200 inc 10
  val ddl = sequence.schema
  val statements = ddl.createStatements
}
