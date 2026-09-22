package derivedmodel {
  final case class Record(values: Seq[Int], counts: Map[String, Int])
}

object Main extends App {
  val encoder = io.circe.generic.semiauto.deriveEncoder[derivedmodel.Record]
  val decoder = io.circe.generic.semiauto.deriveDecoder[derivedmodel.Record]
  val value = derivedmodel.Record(Seq(1, 2), Map("total" -> 3))

  println(decoder.decodeJson(encoder(value)) == Right(value))
}
