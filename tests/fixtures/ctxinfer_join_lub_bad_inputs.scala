trait Join[-L, -R, +Out]

object Join {
  implicit def widen[A]: Join[A, A, A] = new Join[A, A, A] {}
}

object Main {
  val invalid = implicitly[Join[String, Int, Option[String]]]
}
