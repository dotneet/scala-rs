package existential_alias_scope

class Box[T](val value: T)

trait Route extends (String => Int)

object AliasApi {
  type One[T] = Box[Tuple1[T]]

  def run(value: One[_ <: Function1[String, Int]]): Int =
    value.value._1("ok")
}

object Main {
  final class RouteImpl extends Route {
    def apply(value: String): Int = value.length
  }

  def main(args: Array[String]): Unit =
    println(AliasApi.run(new Box(Tuple1(new RouteImpl))))
}
