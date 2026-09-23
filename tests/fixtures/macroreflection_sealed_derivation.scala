import io.circe.Encoder
import io.circe.generic.auto._
import io.circe.generic.semiauto._
import sealedmodel.Shape

object Main extends App {
  val encoder: Encoder[Shape] = deriveEncoder[Shape]
  println(encoder(Shape.Circle(2)).noSpaces.contains("radius"))
}
