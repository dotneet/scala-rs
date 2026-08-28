// Parameterized type members and aliases across a trait hierarchy, in the shape
// slick's profile cake uses: an abstract `type C[T] <: Bound[T]` declared in one
// trait and implemented by `type C[T] = Impl[T]` in another, plus a
// path-dependent alias `type C[T] = self.C[T]` reached through a self-type.

trait TypedType[T] { def name: String }
trait BaseTypedType[T] extends TypedType[T]

trait TypesComponent { self: Profile =>
  type ColumnType[T] <: TypedType[T]
  type BaseColumnType[T] <: ColumnType[T]

  trait Factory {
    // A context bound whose bound is a *parameterized type member*.
    def base[U: BaseColumnType](u: U): String = implicitly[BaseColumnType[U]].name
  }
}

trait Profile extends TypesComponent { self: Profile =>
  trait API {
    // Path-dependent parameterized alias through the self-type.
    type ColumnType[T] = self.ColumnType[T]
  }
}

class JdbcType[T](val name: String) extends BaseTypedType[T]

trait JdbcProfile extends Profile {
  // Implements the abstract parameterized member two levels up: `JdbcType[T]`
  // conforms to `TypedType[T]` only after the parent's `T` is aligned with ours.
  type ColumnType[T] = JdbcType[T]
  type BaseColumnType[T] = JdbcType[T]

  def describe[T](c: ColumnType[T]): String = c.name
}

object Main extends JdbcProfile {
  object api extends API
  object factory extends Factory

  implicit val intType: JdbcType[Int] = new JdbcType[Int]("INTEGER")

  def main(args: Array[String]): Unit = {
    println(describe(intType))
    val viaApi: api.ColumnType[Int] = intType
    println(viaApi.name)
    println(factory.base(7))
  }
}
