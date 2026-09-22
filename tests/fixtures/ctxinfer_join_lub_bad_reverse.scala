trait Join[-L, -R, +Out]

object Join {
  implicit def widen[A]: Join[A, A, A] = new Join[A, A, A] {}
}

trait FixedJoin[-L, -R, Out]

object FixedJoin {
  implicit def same[A]: FixedJoin[A, A, A] = new FixedJoin[A, A, A] {}
}

object Main {
  val invalid = implicitly[Join[Option[String], Option[String], None.type]]
  val invalidFixed = implicitly[FixedJoin[Option[String], Option[String], None.type]]
}
