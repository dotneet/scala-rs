final class Scope {
  sealed trait Driver[-Input, +Output]
  object Driver {
    implicit val base: Driver[Int, ((String, String), (String, String))] =
      null.asInstanceOf[Driver[Int, ((String, String), (String, String))]]

    implicit def step[A, B](implicit tail: Driver[A, (B, B)]): Driver[List[A], B] =
      null.asInstanceOf[Driver[List[A], B]]
  }
}

object PathDependentDriver {
  val scope = new Scope
  import scope.Driver._
  implicitly[scope.Driver[List[List[Int]], String]]
}
