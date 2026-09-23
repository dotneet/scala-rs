import io.circe.Encoder
import io.circe.generic.semiauto.deriveEncoder

final case class Wide(
  field0: Int,
  field1: Int,
  field2: Int,
  field3: Int,
  field4: Int,
  field5: Int,
  field6: Int,
  field7: Int,
  field8: Int,
  field9: Int,
  field10: Int,
  field11: Int,
  field12: Int,
  field13: Int,
  field14: Int,
  field15: Int,
  field16: Int,
  field17: Int,
  field18: Int,
  field19: Int,
  field20: Int,
  field21: Int,
  field22: Int
)

object Wide {
  implicit val encoder: Encoder[Wide] = deriveEncoder[Wide]
}

object Main {
  def main(args: Array[String]): Unit = {
    val value = Wide(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22)
    println(Wide.encoder(value).noSpaces.contains("\"field22\":22"))
  }
}
