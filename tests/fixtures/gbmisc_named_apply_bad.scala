// Names that no alternative declares, or that no single alternative covers,
// still reject the call once both `apply`s are weighed together.
object Svc {
  case class P(pusher: String, before: String, after: String)
  object P {
    def apply(a: Int, s: String, newId: Long, oldId: Long): P =
      P(pusher = s * a, before = oldId.toString, after = newId.toString)
  }
}
object Main {
  def main(args: Array[String]): Unit = {
    println(Svc.P(2, "y", nope = 3L, oldId = 4L))
    println(Svc.P(2, "y", before = "b", oldId = 4L))
  }
}
