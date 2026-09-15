// Multiple contravariant arguments use a coordinate-wise progress relation:
// one consumed input may shrink while another grows as part of the result.
sealed trait Multi[-Left, -Right]

object Multi {
  implicit val base: Multi[Int, List[List[Int]]] =
    null.asInstanceOf[Multi[Int, List[List[Int]]]]

  implicit def step[A, B](implicit tail: Multi[A, List[B]]): Multi[List[A], B] =
    null.asInstanceOf[Multi[List[A], B]]
}

object MultipleContravariantArguments {
  implicitly[Multi[List[List[Int]], Int]]
}
