object Main {
  case class Entry(name: String, count: Int)
  val generic = shapeless.LabelledGeneric[Entry]
  def main(args: Array[String]): Unit = {
    val original = Entry("record", 7)
    println(generic.from(generic.to(original)))
  }
}
