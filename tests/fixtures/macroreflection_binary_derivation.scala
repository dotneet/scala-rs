object Main extends App {
  val encoder = io.circe.generic.semiauto.deriveEncoder[binaryderived.Model.Record]
  val decoder = io.circe.generic.semiauto.deriveDecoder[binaryderived.Model.Record]
  val value = binaryderived.Model.Record("record", Seq(1, 2))

  println(decoder.decodeJson(encoder(value)) == Right(value))
}
