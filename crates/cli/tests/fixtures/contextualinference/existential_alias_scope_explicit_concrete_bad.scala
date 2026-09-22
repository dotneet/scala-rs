package existential_alias_scope

class Box[T]

trait Route extends (String => Int)

object ExplicitConcreteWildcardBad {
  type One[T] = Box[Tuple1[T]]

  def reject(value: One[Route]):
      Box[Tuple1[_ <: (String => Int)]] = value
}
