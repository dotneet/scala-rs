trait ValueShape { type Value; type Element <: ValueShape }
class NumberShape extends ValueShape { type Value = Int; type Element = NumberShape }
class TextShape extends ValueShape { type Value = String; type Element = TextShape }
class ValueHolder[T <: ValueShape]
object AssociatedValues {
  def bind[T <: ValueShape, V](holder: ValueHolder[T], value: V)(implicit ev: V <:< T#Value): V = value
  def element[T <: ValueShape](holder: ValueHolder[T])(implicit ev: T#Element =:= NumberShape): Int = 42
}
object Main {
  def main(args: Array[String]): Unit = {
    println(AssociatedValues.bind(new ValueHolder[NumberShape], 42))
    println(AssociatedValues.bind(new ValueHolder[TextShape], "ok"))
    println(AssociatedValues.element(new ValueHolder[NumberShape]))
  }
}
