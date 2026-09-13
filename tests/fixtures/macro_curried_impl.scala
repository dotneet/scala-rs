// Implementation for macro_curried_use.scala. The third parameter belongs to
// the macro's second, implicit source clause; it is intentionally a raw Tree
// so this fixture isolates argument-clause transport from Context.literal.
import scala.reflect.macros.blackbox.Context

object CurriedImplicitImpl {
  def identityImpl(c: Context)(value: c.Tree)(clue: c.Tree): c.Tree = value
}
