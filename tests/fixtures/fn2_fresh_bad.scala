// What is still refused around the fresh-name forms. `docs/macros.md` §7.10.
//
// A `_` only stands for a name when something *binds* it: a lambda's parameter
// list, or the existential an applied type introduces. On its own it binds
// nothing, and real scalac rejects both of these too ("unbound placeholder
// parameter", "unbound wildcard type") -- so refusing them is the same answer,
// not a gap being papered over.
import scala.reflect.runtime.universe._

object Fn2FreshBad {
  // Nothing binds this `_`: there is no section for the parser to close over.
  val bareTerm = q"_"

  // Nothing binds this `_` either: a wildcard type argument is only an
  // existential relative to the application it stands in.
  val bareType = tq"_"
}
