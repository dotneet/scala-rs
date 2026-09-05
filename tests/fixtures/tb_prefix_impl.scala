// A macro implementation that reaches `c.prefix` through a *named import off
// a value* -- `import c.{prefix => prefix}`, which is how scala/scala's own
// `macro-term-declared-in-*` tests are written.
//
// `c.prefix` written out has always resolved, because a member selection asks
// the pickle for a name the class file declares. The import selector did not
// ask, so the same member was "value prefix is not a member of
// scala.reflect.macros.blackbox.Context" in one spelling and fine in the
// other. `Check::import_named` now asks too.
//
// The other two selectors are ordinary members that resolve either way, so a
// regression that broke them would be visible here as well.
import scala.reflect.macros.blackbox.Context

object TbPrefixImpls {
  def show(c: Context): c.Expr[String] = {
    import c.{prefix => prefix}
    import c.{universe => universe}
    import universe._
    // `prefix` is an `Expr[Nothing]`; its `toString` is what the enclosing
    // program prints, so the receiver the macro was called on is visible in
    // the expansion's output.
    c.Expr[String](Literal(Constant("prefix = " + prefix)))
  }
}
