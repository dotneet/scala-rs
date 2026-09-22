package existential_alias_scope

object Main {
  final class RouteImpl extends Route {
    def apply(value: String): Int = value.length
  }

  def main(args: Array[String]): Unit = {
    val route = new Box(Tuple1(new RouteImpl))
    println(
      AliasApi.run(route) +
        AliasApi.explicit(route) +
        AliasApi.forward(route)
    )
  }
}
