import io.circe.{Decoder, Encoder}
import io.circe.generic.extras.Configuration
import io.circe.generic.extras.semiauto._

object Main {
  implicit val config: Configuration = Configuration.default
  case class Message(f0: String, f1: String, f2: String)
  implicit val encoder: Encoder[Message] = deriveConfiguredEncoder[Message]
  implicit val decoder: Decoder[Message] = deriveConfiguredDecoder[Message]

  def main(args: Array[String]): Unit = {
    val json = Encoder[Message].apply(Message("a", "b", "c"))
    println(json.noSpaces)
    println(Decoder[Message].decodeJson(json))
  }
}
