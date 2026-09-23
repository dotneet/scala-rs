import io.circe.Encoder

object Main {
  def encode(s: String): io.circe.Json = {
    import io.circe.Json.*
    obj(
      "id" -> fromInt(s.length),
      "value" -> fromString(s),
      "items" -> fromValues(Seq.empty),
      "missing" -> Null
    )
  }
}
