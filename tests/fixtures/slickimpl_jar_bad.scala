// The neighbouring rejections. Reading the pickled parent has to *narrow*
// what the class file said, not widen it.
import slick.jdbc.H2Profile.api._

object SlickJarBad {
  // `BaseTypedType` is invariant. The specialized class file parent
  // (`DriverJdbcType<Object>`) would make this compile.
  val wide: slick.ast.BaseTypedType[Any] = longColumnType
  // No column type for a class of our own is in scope.
  val none = implicitly[slick.ast.TypedType[SlickJarBad.type]]
}
