// scalac: ambiguous implicit values: both value a and value b match
// expected type Main.Tag.
object Main {
  case class Tag(s: String)
  implicit val a: Tag = Tag("a")
  implicit val b: Tag = Tag("b")
  def main(args: Array[String]): Unit = println(implicitly[Tag])
}
