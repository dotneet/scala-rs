// What the arrangements in `Main_1.scala` and `Bad_1.scala` import from.
//
// `agent/nameamb`: SLS 2 gives a definition higher precedence than an import,
// but nsc only lets precedence decide when the two are at the *same* nesting
// level. An import nested more deeply than the definition is neither hidden
// nor preferred -- the reference is ambiguous, and scalac refuses to pick.
package nameamb

object Wild {
  def who: String = "wild"
}

object Exp {
  def who: String = "explicit"
}

object Priv {
  // Not visible outside `Priv` itself, so `import Priv._` offers nothing
  // under this name and cannot make a reference to it ambiguous. This is
  // nsc's `qualifies` filter in `Context.lookupSymbol`.
  private[this] def hidden: String = "private-to-this"
  def keep: String = hidden
}

object Sib {
  object Elsewhere {
    def who: String = "wild-elsewhere"
  }
}

trait Inherited {
  def who: String = "inherited"
}
