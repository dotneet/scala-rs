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

  var sequenceBuilds = 0

  implicit def sequence[A](implicit element: Shape[A]): Aux[List[A], List[element.Out]] = {
    sequenceBuilds += 1
    new Shape[List[A]] {
      type Out = List[element.Out]
      val depth = element.depth + 1
    }
  }
}

object Main {
  val oneStep: Shape.Aux[List[Int], List[String]] =
    implicitly[Shape.Aux[List[Int], List[String]]]
  val twoSteps: Shape.Aux[List[List[Int]], List[List[String]]] =
    implicitly[Shape.Aux[List[List[Int]], List[List[String]]]]

  def main(args: Array[String]): Unit =
    println(s"${oneStep.depth}:${twoSteps.depth}:${Shape.sequenceBuilds}")
}
