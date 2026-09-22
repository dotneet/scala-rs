object Main {
  case class Entry(name: String, count: Int)
  val generic = shapeless.Generic[Entry]
  def main(args: Array[String]): Unit = {
    val original = Entry("record", 7)
    println(generic.from(generic.to(original)))
  }
}
