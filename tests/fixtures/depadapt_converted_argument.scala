import scala.language.implicitConversions
trait ValueKind
class NumberKind extends ValueKind
trait Combine[R <: ValueKind, A <: ValueKind, B <: ValueKind]
class ConvertedValue[A <: ValueKind](val value: Int) {
  def minus[B <: ValueKind, R <: ValueKind](other: ConvertedValue[B])(
    implicit combine: Combine[R, A, B]
  ): ConvertedValue[R] = new ConvertedValue[R](value - other.value)
}
object Main {
  private var conversions = 0
  implicit def fromInt(value: Int): ConvertedValue[NumberKind] = {
    conversions += 1
    new ConvertedValue[NumberKind](value)
  }
  implicit val numbers: Combine[NumberKind, NumberKind, NumberKind] =
    new Combine[NumberKind, NumberKind, NumberKind] {}
  def main(args: Array[String]): Unit = {
    val result: ConvertedValue[NumberKind] = new ConvertedValue[NumberKind](42).minus(1)
    println(result.value)
    println(conversions)
  }
}
