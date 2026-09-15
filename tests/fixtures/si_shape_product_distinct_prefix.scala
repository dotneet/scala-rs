final class Scope {
  sealed trait Driver[-Input, +Output]
  object Driver {
    implicit def step[A, B](implicit tail: Driver[A, (B, B)]): Driver[List[A], B] =
      null.asInstanceOf[Driver[List[A], B]]
  }
}

object DistinctPathPrefix {
  val left = new Scope
  val right = new Scope
  implicit val onlyLeft: left.Driver[Int, ((String, String), (String, String))] =
    null.asInstanceOf[left.Driver[Int, ((String, String), (String, String))]]
  implicitly[right.Driver[List[List[Int]], String]]
}
