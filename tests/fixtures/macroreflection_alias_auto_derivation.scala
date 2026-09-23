object Host {
  final case class Request(courseCode: String)
}

class DecoderHolder {
  type Request = Host.Request
  import io.circe.generic.auto._

  val decoder: io.circe.Decoder[Request] = implicitly
}

object Main extends App {
  val json = io.circe.Json.obj("courseCode" -> io.circe.Json.fromString("A"))
  println(new DecoderHolder().decoder.decodeJson(json) == Right(Host.Request("A")))
}
