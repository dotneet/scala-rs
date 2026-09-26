import io.circe._
import io.circe.generic.semiauto._

package macroderive {
  final case class Point(x: Int, y: Int, tags: List[String])
  object Point {
    implicit val encoder: Encoder[Point] = deriveEncoder[Point]
    implicit val decoder: Decoder[Point] = deriveDecoder[Point]
  }

  sealed trait Shape
  object Shape {
    final case class Circle(radius: Int, center: Point) extends Shape
    final case class Square(side: Int) extends Shape
    implicit val encoder: Encoder[Shape] = deriveEncoder[Shape]
    // The coproduct decoder's `CNil` case passes `c.history` by name to the
    // overloaded `DecodingFailure.apply`, inside a shapeless `Lazy` instance.
    implicit val decoder: Decoder[Shape] = deriveDecoder[Shape]
  }
}

object Main {
  import macroderive._
  def main(args: Array[String]): Unit = {
    val shapes: List[Shape] = List(Shape.Circle(2, Point(1, 2, List("a"))), Shape.Square(3))
    for (shape <- shapes) {
      val json = Encoder[Shape].apply(shape)
      println(json.noSpaces)
      println(Decoder[Shape].decodeJson(json) == Right(shape))
    }
    println(Decoder[Shape].decodeJson(Json.obj("Triangle" -> Json.obj())).isLeft)
  }
}
