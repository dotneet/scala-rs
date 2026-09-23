trait RouteProvider {
  type Route = Int => String
  def route: Route
}

class Service extends RouteProvider {
  private def getProfile: Route = _.toString
  private def getProfile(prefix: String, offset: Int)(implicit factor: Int): Route =
    n => prefix + (n + offset * factor).toString

  override def route: Route = getProfile
}

object Main {
  def main(args: Array[String]): Unit = println(new Service().route(3))
}
