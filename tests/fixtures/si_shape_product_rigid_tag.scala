// A rigid, unrelated type parameter must not be treated as an inference hole
// when checking the recursive input measure. The derivation still terminates
// after two structurally decreasing input steps.
sealed trait DriverT[-Input, +Output, Tag]

object DriverT {
  implicit def base[T]: DriverT[Int, ((String, String), (String, String)), T] =
    null.asInstanceOf[DriverT[Int, ((String, String), (String, String)), T]]

  implicit def step[A, B, T](implicit tail: DriverT[A, (B, B), T]): DriverT[List[A], B, T] =
    null.asInstanceOf[DriverT[List[A], B, T]]
}

object RigidTagChangesDivergence {
  def evidence[T]: DriverT[List[List[Int]], String, T] = implicitly
}
