// Settling an overloaded reference in value position must not make the set
// disappear from *application* position, and must not turn a genuine "not a
// member" into silence.

class Req(val id: String)

trait Ctx {
  implicit def request: Req
}

trait Params { self: Ctx =>
  def params(implicit request: Req): Map[String, String] =
    Map("id" -> request.id)
  def params(key: String)(implicit request: Req): String =
    params(request).getOrElse(key, "?")
}

class Bad extends Ctx with Params {
  implicit def request: Req = new Req("x")
  // No alternative takes an `Int`.
  def oops: String = params(42)
  // `Map[String, String]` has no `nope`; the message names the resolved
  // alternative's result, not the overload set.
  def alsoOops: Int = params.nope
}
