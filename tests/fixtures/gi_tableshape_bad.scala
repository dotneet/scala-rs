// The fallback that settles a candidate's own type parameter from its
// evidence clause is still a *search*: a receiver with no witness at all is
// still rejected. `Loose` is not an `AbstractTable`, so no `Ev[Loose,
// AbstractTable[T]]` exists and `tableShape` cannot apply.
import scala.language.implicitConversions

trait ShapeLevel
trait FlatShapeLevel extends ShapeLevel

class Shape[Level <: ShapeLevel, -M, U, P](val show: String)

class Ev[-From, +To]
object Ev {
  implicit def refl[A]: Ev[A, A] = new Ev
}

abstract class AbstractTable[T]
class Loose

object Shape {
  implicit def tableShape[L <: ShapeLevel, T, C <: AbstractTable[_]](implicit
      ev: Ev[C, AbstractTable[T]]
  ): Shape[L, C, T, C] = new Shape("table")
}

object Main {
  def mapped[F, T, G](f: F)(implicit s: Shape[_ <: FlatShapeLevel, F, T, G]): String =
    s.show

  def main(args: Array[String]): Unit = println(mapped(new Loose))
}
