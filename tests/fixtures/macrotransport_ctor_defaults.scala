import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

object ConstructorDefaultMacro {
  def make(): Any = macro makeImpl

  def makeImpl(c: Context)(): c.Expr[Any] = {
    import c.universe._
    c.Expr[Any](c.typecheck(q"new SameRunDefaultRow()"))
  }
}
