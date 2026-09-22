package existential_alias_scope

class Box[T]

object ExplicitWildcardBad {
  type One[T] = Box[Tuple1[T]]

  def alias(value: One[_ <: Function1[String, Int]]): Unit = ()

  def reject(value: One[_ <: Function1[String, Int]]):
      Box[Tuple1[_ <: Function1[String, Int]]] = value
}
