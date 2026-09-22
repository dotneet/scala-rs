package existential_alias_scope

class Box[T]

trait Route extends (String => Int)

object BoundBad {
  type One[T] = Box[Tuple1[T]]

  def route(value: One[_ <: Function1[String, Int]]): Unit = ()

  def reject(): Unit = route(new Box(Tuple1("not a route")))
}
