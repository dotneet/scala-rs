import java.util.UUID

object Main {
  val selected: () => UUID = UUID.randomUUID

  object Imported {
    import UUID.randomUUID
    val value: () => UUID = randomUUID
  }

  def main(args: Array[String]): Unit =
    println(selected() != null && Imported.value() != null)
}
