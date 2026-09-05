// gitbucket's `model/Profile.scala` and one of its table components, against
// the published `slick_2.13-3.4.1.jar`; see `crates/cli/tests/slickimplicit.rs`.
//
// slick's profile cake declares its column types as *parameterised abstract
// type members*:
//
//   trait RelationalTypesComponent {
//     type ColumnType[T] <: TypedType[T]
//     type BaseColumnType[T] <: ColumnType[T] with BaseTypedType[T]
//   }
//
// and `RelationalProfile.API` re-exports them as
// `type BaseColumnType[T] = RelationalTypesComponent.this.BaseColumnType[T]`.
// Both spellings are what `import profile.api._` hands a program that writes
// its own `MappedColumnType`, and neither used to resolve: the pickle reader
// dropped an abstract member's own type parameters, so `BaseColumnType[T]`
// was "does not take type parameters" and every declaration written at one
// was an unmappable result type. That is why `column[Event]` could
// not find a `TypedType[Event]` even though the profile declares one a few
// lines up.
import slick.jdbc.JdbcProfile
import slick.ast.{BaseTypedType, TypedType}

class Event(val name: String)

trait Profile {
  val profile: JdbcProfile
  import profile.api._

  // gitbucket writes two of these; this is the `WebHook.Event` one, mapped
  // through `String`.
  implicit val eventColumnType: BaseColumnType[Event] =
    MappedColumnType.base[Event, String](_.name, new Event(_))
}

trait Component { self: Profile =>
  import profile.api._
  import self._

  // The column type the profile declared has to be found through the self
  // type, and it has to be a `TypedType[Event]` for `column` to take it.
  class Events(tag: Tag) extends Table[Event](tag, "EVENTS") {
    val at = column[Event]("AT")
    val name = column[String]("NAME")
    def * = at
  }

  // The same witness, asked for directly, by each of the names it has.
  val asTyped: TypedType[Event] = eventColumnType
  val asBase: BaseTypedType[Event] = eventColumnType
  val summonedTyped = implicitly[TypedType[Event]]
  val summonedBase = implicitly[BaseTypedType[Event]]
}
