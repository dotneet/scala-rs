package existential_alias_scope

class Box[T](val value: T)

trait Route extends (String => Int)

object AliasApi {
  type One[T] = Box[Tuple1[T]]

  def accept(value: One[_ <: (String => Int)]): Int = {
    val run = value.value._1
    run("ok")
  }

  def run(value: One[_ <: Function1[String, Int]]): Int = accept(value)

  def explicit(value: Box[Tuple1[T]] forSome { type T <: (String => Int) }): Int =
    value.value._1("ok")

  def forward(value: One[_ <: (String => Int)]): Int = accept(value)
}
