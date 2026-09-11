// A case class companion that declares its own `apply` overload keeps the
// synthetic one (nsc suppresses it only for a written `apply` of the *same*
// signature), and named arguments choose among both by name -- in any
// order, and through a qualified reference (`Svc.P(...)`), which is how
// gitbucket calls `WebHookService.WebHookPushPayload(..., oldId = ...,
// newId = ...)`.
trait Payload
object Svc {
  case class P(pusher: String, before: String, after: String) extends Payload
  object P {
    def apply(a: Int, s: String, newId: Long, oldId: Long): P =
      P(pusher = s * a, before = oldId.toString, after = newId.toString)
  }
}
case class Q(a: String, b: Int)
object Q {
  def apply(a: String, b: Int): Q = new Q(a + "!", b * 2)
}
object Main {
  def main(args: Array[String]): Unit = {
    val r: Option[Payload] = for (x <- Some(2)) yield {
      Svc.P(x, "x", oldId = 1L, newId = 9L)
    }
    println(r)
    println(Svc.P(2, "y", newId = 3L, oldId = 4L))
    println(Svc.P("p", after = "a", before = "b"))
    println(Svc.P(pusher = "q", before = "b", after = "a"))
    println(Q("x", 1))
    println(Q(b = 3, a = "y"))
  }
}
