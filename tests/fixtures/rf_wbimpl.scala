// Whitebox macro implementations, compiled first; `rf_wbuse.scala` is compiled
// against this one, so the expansions really run through the JVM bridge.
//
// A whitebox macro's expansion type *replaces* the declared result type
// (nsc `macroExpandApply`'s `WhiteboxExpansion`); a blackbox macro's does not,
// and the declaration stands however precise the expansion is. Both kinds are
// declared here from the same expansion so the two rules are told apart by the
// call site and not by the tree.
import scala.language.experimental.macros
import scala.reflect.macros.whitebox.Context

object RfWb {
  // Declared `Any`, expands to a tree whose type is `Option[Int]`.
  def someTwo: Any = macro someTwoImpl
  def someTwoImpl(c: Context) = {
    import c.universe._
    q"if (true) Some(2) else None"
  }

  // The same expansion behind a *blackbox* declaration: the call site sees
  // `Any`, which is what `rf_wbbad.scala` checks.
  def anyTwo: Any = macro anyTwoImpl
  def anyTwoImpl(c: scala.reflect.macros.blackbox.Context) = {
    import c.universe._
    q"if (true) Some(2) else None"
  }

  // A whitebox expansion whose type is a primitive, and one that is a literal
  // string: enough to see that the replacement is the expansion's own type and
  // not just "something more specific than Any".
  def five: Any = macro fiveImpl
  def fiveImpl(c: Context) = {
    import c.universe._
    q"2 + 3"
  }

  def hello: Any = macro helloImpl
  def helloImpl(c: Context) = {
    import c.universe._
    q""""hello""""
  }
}
