// A recursive implicit may grow the result while consuming a smaller
// contravariant input, even when the wanted type is completely concrete.
sealed trait DriverC[-Input, +Output]

object DriverC {
  implicit val base: DriverC[Int, ((String, String), (String, String))] =
    null.asInstanceOf[DriverC[Int, ((String, String), (String, String))]]

  implicit def step[A, B](implicit tail: DriverC[A, (B, B)]): DriverC[List[A], B] =
    null.asInstanceOf[DriverC[List[A], B]]
}

object ConcreteGrowth {
  implicitly[DriverC[List[List[Int]], String]]
}
