trait Shape[A] {
  type Out
  def depth: Int
}

object Shape {
  type Aux[A, O] = Shape[A] { type Out = O }

  implicit val number: Aux[Int, String] = new Shape[Int] {
    type Out = String
    val depth = 1
  }

  implicit def sequence[A](implicit element: Shape[A]): Aux[List[A], List[element.Out]] =
    new Shape[List[A]] {
      type Out = List[element.Out]
      val depth = element.depth + 1
    }
}

object Main {
  val wrong = implicitly[Shape.Aux[List[Int], List[Char]]]
}
