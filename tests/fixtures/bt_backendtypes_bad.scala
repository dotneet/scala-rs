// The negative half of `bt_backendtypes.scala`. Preferring the definition
// that fixes a type member must not make *every* deferred member resolve to
// something: a member nothing fixes stays abstract, and a member that
// genuinely is not there is still diagnosed.

class SessionDef(val conn: String) {
  def label: String = "session(" + conn + ")"
}

trait BaseBackend {
  type Session
  type Handle
  def handle(): Handle
}

trait ConcreteBackend extends BaseBackend {
  type Session = SessionDef
}

object Main {
  // `Handle` is deferred all the way down, so it has no members at all.
  def opaque(b: ConcreteBackend): Int = b.handle().length

  // `Session` is fixed at `SessionDef`, which has no `rollback`.
  def absent(s: ConcreteBackend#Session): Unit = s.rollback()

  def main(args: Array[String]): Unit = ()
}
