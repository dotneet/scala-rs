trait ValueShape { type Value }
class NumberShape extends ValueShape { type Value = Int }
class ValueHolder[T <: ValueShape]
object Main {
  def bind[T <: ValueShape, V](holder: ValueHolder[T], value: V)(implicit ev: V <:< T#Value): V = value
  val invalid = bind(new ValueHolder[NumberShape], "wrong")
}
