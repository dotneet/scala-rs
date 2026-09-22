package existential_alias_scope

class Box[T]

object CorrelationBad {
  type Same[T] = Box[(T, T)]

  def same(value: Same[_]): Unit = ()

  def reject(): Unit = same(new Box(("left", 1)))
}
