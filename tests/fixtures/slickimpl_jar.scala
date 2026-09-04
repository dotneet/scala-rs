// Using slick's *published* jar, which is a different measurement from
// compiling slick's sources: the DSL is made of implicits, and every one of
// them has to survive the trip through the pickle.
import slick.jdbc.H2Profile.api._

object SlickJarTypes {
  // `LongJdbcType`'s class file says its superclass is
  // `JdbcTypesComponent$DriverJdbcType$mcJ$sp`, the `@specialized` variant,
  // whose own superclass fixes the parameter to `Object`. The pickle -- which
  // is what nsc reads -- says `DriverJdbcType[Long]`.
  val a: slick.ast.BaseTypedType[Long] = longColumnType
  val b: slick.jdbc.JdbcType[Int] = intColumnType
  val c: slick.ast.TypedType[Boolean] = booleanColumnType
}

// Naming `Table[…]` as a parent reads the profile's class files, which is the
// order in which `fill_java_members` used to overwrite the `implicit` flag the
// pickle had just put on all 24 column types.
class JarUsers(tag: Tag) extends Table[(Long, String)](tag, "USERS") {
  def id = column[Long]("ID")
  def name = column[String]("NAME")
  def * = (id, name)
}

object SlickJarUse {
  val tt = implicitly[slick.ast.TypedType[String]]
  val users = TableQuery[JarUsers](t => new JarUsers(t))
}

// gitbucket's shape: the profile is an abstract `val` and the whole API
// arrives through a path, in the signatures of the members that follow.
trait JarProf { val profile: slick.jdbc.JdbcProfile }

trait JarComponent { self: JarProf =>
  import profile.api._

  implicit val dateColumnType: BaseColumnType[java.util.Date] = ???
  def byName(r: Rep[String]): Rep[String] = r
  def table(t: Table[String]): Int = 1
}
