// The negative half of `hkself_member.scala`: reducing `self.T` through the
// profile it is read at must not become an unconditional widening.
//
// Two different profiles' `api.ColumnType` are two different types, and a
// profile that leaves `ColumnType` deferred settles nothing at all.

trait TypedType[T] { def name: String }

trait TypesComponent { self: Profile =>
  type ColumnType[T] <: TypedType[T]
}

trait Profile extends TypesComponent { self: Profile =>
  trait API {
    type ColumnType[T] = self.ColumnType[T]
  }
}

class JdbcType[T](val name: String) extends TypedType[T]
class MemType[T](val name: String) extends TypedType[T]

trait JdbcProfile extends Profile { type ColumnType[T] = JdbcType[T] }
trait MemProfile extends Profile { type ColumnType[T] = MemType[T] }

object Jdbc extends JdbcProfile {
  object api extends API
  val mine: api.ColumnType[Int] = new JdbcType[Int]("INTEGER")
  // `Mem`'s profile settles `ColumnType` as `MemType`, not `JdbcType`.
  val stolen: Mem.api.ColumnType[Int] = new JdbcType[Int]("INTEGER")
}

object Mem extends MemProfile {
  object api extends API
  val mine: api.ColumnType[Int] = new MemType[Int]("MEM_INT")
}

// `ColumnType` is still deferred in this profile, so `api.ColumnType[Int]` is
// an abstract member and nothing concrete conforms to it.
trait OpenProfile extends Profile {
  object api extends API
  val open: api.ColumnType[Int] = new JdbcType[Int]("OPEN")
}
