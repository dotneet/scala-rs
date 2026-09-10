import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context
object InferredMacro {
  // No wildcard universe import and no declared implementation result type.
  def impl(c: Context)(a: c.Expr[Any]) = c.Expr[Any](c.untypecheck(a.tree))
  def identity(a: Any): Any = macro impl
}
