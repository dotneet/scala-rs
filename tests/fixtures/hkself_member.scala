// A type member read through the *self alias* of the class that declares it,
// from a class two levels below the declaration: slick's profile cake.
//
//   trait Profile extends TypesComponent { self: Profile =>
//     trait API { type ColumnType[T] = self.ColumnType[T] }
//   }
//
// `API` is not itself a `Profile`, so `self` is the *enclosing* profile
// instance, and the concrete profile that mixes `API` in is what settles
// `ColumnType`. Two profiles settle it differently, and the reduction has to
// follow the one it is read at -- not the declaration, and not `API`'s own
// alias folding back onto itself.

trait TypedType[T] { def name: String }

trait TypesComponent { self: Profile =>
  type ColumnType[T] <: TypedType[T]
}

trait Profile extends TypesComponent { self: Profile =>
  trait API {
    type ColumnType[T] = self.ColumnType[T]
  }
  def describe[T](c: ColumnType[T]): String = c.name
}

class JdbcType[T](val name: String) extends TypedType[T]
class MemType[T](val name: String) extends TypedType[T]

trait JdbcProfile extends Profile { type ColumnType[T] = JdbcType[T] }
trait MemProfile extends Profile { type ColumnType[T] = MemType[T] }

object Jdbc extends JdbcProfile {
  object api extends API
  val intType: JdbcType[Int] = new JdbcType[Int]("INTEGER")
  // `api.ColumnType[Int]` is `JdbcType[Int]` here, so a `JdbcType[Int]` goes
  // in and `describe`, declared against the abstract `ColumnType`, takes it.
  val viaApi: api.ColumnType[Int] = intType
  def show: String = describe(viaApi)
}

object Mem extends MemProfile {
  object api extends API
  val intType: MemType[Int] = new MemType[Int]("MEM_INT")
  val viaApi: api.ColumnType[Int] = intType
  def show: String = describe(viaApi)
}

// A profile that leaves `ColumnType` deferred: `api.ColumnType[T]` must stay
// abstract rather than collapse to anything, and still be the same type as
// the declaration `describe` is written against.
trait OpenProfile extends Profile {
  object api extends API
  def relay[T](c: api.ColumnType[T]): String = describe(c)
}

object Open extends OpenProfile {
  type ColumnType[T] = JdbcType[T]
  def show: String = relay(new JdbcType[Int]("OPEN"))
}

// The other half of the same rule: a `self` named from an *anonymous*
// subclass of the very trait that binds it is still the outer instance, so
// this alias must not resolve its own right-hand side back to itself.
trait Cell { self =>
  type R
  def make: R
  def render(r: R): String
  // `self.R` and `other.R` are two different members, and neither is this
  // anonymous class's own `R` -- which is the alias being defined.
  def zip(other: Cell): Cell =
    new Cell {
      type R = (self.R, other.R)
      def make: R = (self.make, other.make)
      def render(r: R): String = self.render(r._1) + "+" + other.render(r._2)
    }
  def show: String = render(make)
}

object IntCell extends Cell {
  type R = Int
  def make: Int = 1
  def render(r: Int): String = "i" + r
}

object StrCell extends Cell {
  type R = String
  def make: String = "s"
  def render(r: String): String = "s:" + r
}

object Main {
  // Read from outside either profile: the *prefix* says which profile settles
  // `ColumnType`, and `Main` is not one itself.
  val outsideJdbc: Jdbc.api.ColumnType[Int] = Jdbc.intType
  val outsideMem: Mem.api.ColumnType[Int] = Mem.intType

  def main(args: Array[String]): Unit = {
    println(Jdbc.show)
    println(Mem.show)
    println(Open.show)
    println(IntCell.zip(StrCell).show)
    println(outsideJdbc.name + "/" + outsideMem.name)
  }
}
