// The three shapes scalatra puts a gitbucket controller in, with the jar's
// declarations rewritten as source so the fixture needs nothing but
// scala-library.
//
//   1. an overload set whose alternatives are `(implicit r: Req)M` and
//      `(key: String)(implicit r: Req)S`. In value position -- as the
//      qualifier of `params.get(...)` -- nsc's `isAsSpecific` looks through
//      the implicit clause, so the first alternative is strictly more
//      specific and is the one the reference means;
//   2. a *declaration* of an implicit in one trait and its *definition* in
//      another, where neither trait is a base of the other, reached through a
//      `self:` annotation rather than through `extends`;
//   3. the same pair reached from inside an anonymous class nested in the
//      class that mixes them in.

class Req(val id: String)

// Declares `request`; `Scope` below defines it, and neither is a base of the
// other -- exactly `org.scalatra.ScalatraContext` and `DynamicScope`.
trait Ctx {
  implicit def request: Req
}

trait Scope {
  private var cur: Req = new Req("none")
  implicit def request: Req = cur
  def enter(r: Req): Unit = { cur = r }
}

trait Params { self: Ctx =>
  def params(implicit request: Req): Map[String, String] =
    Map("id" -> request.id, "kind" -> "map")
  def params(key: String)(implicit request: Req): String =
    params(request).getOrElse(key, "?")
}

trait Base extends Ctx with Scope with Params

class Direct extends Base {
  def viaSelect: Option[String] = params.get("id")
  def viaGetOrElse: String = params.getOrElse("kind", "?")
  def viaApply: String = params("id")
  // The set is still a set where an argument list picks from it.
  def viaSize: Int = params.size
}

// The self-type route: `Base`'s members are visible unqualified here, but
// `ViaSelfType`'s own linearization holds neither `Ctx` nor `Scope`.
trait ViaSelfType { self: Base =>
  def fromSelf: String = params("id")
  def fromSelfGet: String = params.getOrElse("kind", "?")
}

class WithSelf extends Base with ViaSelfType

abstract class Constraint {
  def validate(name: String): String
}

class Nested extends Base {
  // Inside the anonymous class, `params` and its implicit `request` are the
  // enclosing class's, and the anonymous class's linearization is just
  // `Constraint`.
  def constraint: Constraint = new Constraint {
    def validate(name: String): String = params(name)
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val d = new Direct
    d.enter(new Req("d1"))
    println(d.viaSelect)
    println(d.viaGetOrElse)
    println(d.viaApply)
    println(d.viaSize)
    val w = new WithSelf
    w.enter(new Req("w1"))
    println(w.fromSelf)
    println(w.fromSelfGet)
    val n = new Nested
    n.enter(new Req("n1"))
    println(n.constraint.validate("id"))
    println(n.constraint.validate("missing"))
  }
}
