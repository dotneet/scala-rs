// An abstract type member of a jar's cake, reached through the nested `API`
// trait's alias `type BaseColumnType[T] = self.BaseColumnType[T]`.
//
// The alias's right-hand side is written against the *enclosing* class's
// `this`, which a pickle records beside the member and not in the type, so it
// arrives as the bare deferred `RelationalTypesComponent.BaseColumnType`.
// nsc rebinds it to the definition the prefix's profile has
// (`JdbcTypesComponent`'s `JdbcType[T] with BaseTypedType[T]`); without that,
// `timestampColumnType` could not be shown to fit and gitbucket's
// `Profile.scala` reported "could not find implicit value of type
// RelationalTypesComponent.BaseColumnType[Timestamp]".
//
// Compiled against the slick 3.4.1 jar by both compilers; not run, because a
// `JdbcProfile` instance is not what this is about.
import slick.jdbc.JdbcProfile

trait GzeroBaseColumn {
  val profile: JdbcProfile
  import profile.api._

  // gitbucket's `gitbucket.core.model.Profile`, verbatim apart from
  // blocking-slick's profile type.
  implicit val dateColumnType: BaseColumnType[java.util.Date] =
    MappedColumnType.base[java.util.Date, java.sql.Timestamp](
      d => new java.sql.Timestamp(d.getTime),
      t => new java.util.Date(t.getTime)
    )

  // The name the import offers is the profile's own member, which is the
  // intersection `JdbcTypesComponent` fixes it to -- not the abstract
  // declaration `RelationalTypesComponent` leaves.
  val viaImport: BaseColumnType[java.sql.Timestamp] =
    implicitly[BaseColumnType[java.sql.Timestamp]]
  val viaProfile: profile.BaseColumnType[java.sql.Timestamp] = viaImport
  val expanded: slick.jdbc.JdbcType[java.sql.Timestamp]
    with slick.ast.BaseTypedType[java.sql.Timestamp] = viaImport
}
