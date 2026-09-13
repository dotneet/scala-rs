// Context.literal calls used by ScalaTest's BooleanMacro. Keeping both
// overloads here makes the bridge contract independently testable without an
// external framework jar.
import scala.reflect.macros.blackbox.Context

object ContextLiteralImpl {
  def boolImpl(c: Context)(): c.Expr[Boolean] = c.literal(true)
  def stringImpl(c: Context)(): c.Expr[String] = c.literal("ok")
}
