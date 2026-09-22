import scala.language.implicitConversions
trait ValueKind
class NumberKind extends ValueKind
class TextKind extends ValueKind
trait Combine[R <: ValueKind, A <: ValueKind, B <: ValueKind]
class ConvertedValue[A <: ValueKind] {
  def minus[B <: ValueKind, R <: ValueKind](other: ConvertedValue[B])(
    implicit combine: Combine[R, A, B]
  ): ConvertedValue[R] = new ConvertedValue[R]
}
object Main {
  implicit def fromString(value: String): ConvertedValue[TextKind] = new ConvertedValue[TextKind]
  implicit val numbers: Combine[NumberKind, NumberKind, NumberKind] =
    new Combine[NumberKind, NumberKind, NumberKind] {}
  val invalid: ConvertedValue[NumberKind] = new ConvertedValue[NumberKind].minus("wrong")
}
