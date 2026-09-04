// Expanding an abstract type member through `this` must not make it *anything*:
// where the mixin narrows it, a wider value is still rejected, and where no
// mixin refines it at all the abstract member's bounds still hold.

sealed trait BadRps
object BadRps {
  case object All extends BadRps
  case object One extends BadRps
}

trait BadComp {
  type Rows >: BadRps.One.type <: BadRps
  abstract class Impl[U] {
    def insertAll(values: Iterable[U], rows: Rows): String = "" + values + rows
  }
}

trait BadSingle extends BadComp {
  override type Rows = BadRps.One.type
}

trait BadNarrowProfile extends BadComp with BadSingle {
  private trait Insert[U] extends Impl[U] {
    // `All` is a `BadRps`, but here `Rows` is `One.type`.
    override def insertAll(values: Iterable[U], rows: Rows): String =
      super.insertAll(values, BadRps.All)
  }
}

trait BadOpenProfile extends BadComp {
  private trait Insert[U] extends Impl[U] {
    // Nothing refines `Rows` here, so only its lower bound `One.type` conforms.
    override def insertAll(values: Iterable[U], rows: Rows): String =
      super.insertAll(values, BadRps.All)
  }
}
