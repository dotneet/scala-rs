// What filling defaults must still refuse.
package da

object helpers2 {
  def avatar(name: String, size: Int, tooltip: Boolean = false): String =
    name + size + tooltip
}

trait Svc2 { def get(name: String, all: Boolean = false): String = name + all }

object BadMain extends Svc2 {
  import helpers2._

  // `size` has no default, so it cannot be omitted even though `tooltip` has
  // one and the call reaches `avatar` through a wildcard import.
  val a: String = avatar("octocat")

  // A default does not make the parameter untyped.
  val b: String = get("x", all = "yes")

  // Nor does it invent a parameter name.
  val c: String = get("x", removed = true)
}
