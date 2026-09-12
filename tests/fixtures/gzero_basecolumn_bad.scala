// The near miss of `gzero_basecolumn`: a `MappedColumnType.base[T, U]` whose
// `U` really has no column type. The rebind must not make every applied
// abstract member fit -- both compilers reject this on the same line.
import slick.jdbc.JdbcProfile

class GzeroNoColumn(val n: Int)

trait GzeroBaseColumnBad {
  val profile: JdbcProfile
  import profile.api._

  implicit val badColumnType: BaseColumnType[java.util.Date] =
    MappedColumnType.base[java.util.Date, GzeroNoColumn](
      d => new GzeroNoColumn(d.getTime.toInt),
      c => new java.util.Date(c.n.toLong)
    )
}
