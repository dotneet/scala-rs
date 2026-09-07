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

  // Supplying a case class's *synthetic* companion `apply` from a pickle must
  // not make an instance of the class applicable. `scala/scala`'s
  // `neg/t4196` is exactly this: a `Some[String]` applied to an argument, for
  // which scalac says "Some[String] does not take parameters". Supplying the
  // pickled `Some$.apply` here compiled it as `Some$.apply("spurious")` with
  // the receiver dropped.
  val d: Any = Some("first")("spurious")
}
