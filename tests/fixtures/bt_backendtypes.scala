// A cake in the shape of slick's backends: a base trait leaves type members
// deferred and a derived trait fixes some of them. Every place that resolves
// one of those names has to prefer the definition that fixes it over the
// declaration it overrides -- nsc's "a concrete definition overrides a
// deferred one" -- while a member nothing fixes stays abstract.
//
// The library case this stands for is `slick.basic.BasicBackend`'s
// `type Session` against `slick.jdbc.JdbcBackend`'s `type Session =
// SessionDef`; see `bt_backendtypes_bad.scala` for the negative half.

class SessionDef(val conn: String) {
  def label: String = "session(" + conn + ")"
}

trait BaseBackend {
  type Session
  /** Nothing below fixes this one; it has to stay abstract. */
  type Handle
  def open(): Session
  def handle(): Handle
}

trait ConcreteBackend extends BaseBackend {
  type Session = SessionDef
  def open(): Session = new SessionDef("jdbc:mem")
}

object TheBackend extends ConcreteBackend {
  type Handle = String
  def handle(): Handle = "handle"
}

object Main {
  // Through the type projection: `ConcreteBackend` fixes `Session`, so this
  // is a `SessionDef` and `label` is one of its members.
  def viaProjection(s: ConcreteBackend#Session): String = s.label

  // Through the path: the same member reached off the object.
  def viaPath(s: TheBackend.Session): String = s.conn

  // A member nothing fixes stays opaque -- it can be named and passed
  // around, and nothing is a member of it. `bt_backendtypes_bad` checks that
  // half; naming it here is enough to show it is still a type.
  def viaAbstract(h: BaseBackend#Handle): Int = 1

  def main(args: Array[String]): Unit = {
    val backend: ConcreteBackend = TheBackend
    println(viaProjection(backend.open()))
    println(viaPath(TheBackend.open()))
    // `TheBackend` is the class that fixes `Handle`, so this is a `String`.
    println(TheBackend.handle().length)
  }
}
