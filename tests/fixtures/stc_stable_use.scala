import stablemember.Api

object Main {
  val entry = new Api.types.Entry {
    override def label: String = "ok"
  }

  def main(args: Array[String]): Unit = println(entry.label)
}
