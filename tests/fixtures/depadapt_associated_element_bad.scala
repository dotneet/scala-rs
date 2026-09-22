trait ValueShape { type Element <: ValueShape }
class NumberShape extends ValueShape { type Element = NumberShape }
class TextShape extends ValueShape { type Element = TextShape }
class ValueHolder[T <: ValueShape]
object Main {
  def element[T <: ValueShape](holder: ValueHolder[T])(implicit ev: T#Element =:= NumberShape): Int = 42
  val invalid = element(new ValueHolder[TextShape])
}
