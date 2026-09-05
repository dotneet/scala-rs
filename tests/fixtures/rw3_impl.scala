// A macro implementation that reads `c.prefix` and nothing else, so what the
// call site prints is exactly the tree the bridge handed over.
import scala.reflect.macros.blackbox.Context

object Rw3Impls {
  def showPrefix(c: Context): c.Expr[Unit] = {
    import c.universe._
    val text = "prefix = " + c.prefix
    c.Expr[Unit](
      Apply(
        Select(Ident(definitions.PredefModule), TermName("println")),
        List(Literal(Constant(text)))
      )
    )
  }
}
