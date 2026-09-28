package probe

import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object ClasspathProbe {
  def available: Boolean = macro ClasspathProbeImpl.available
}

object ClasspathProbeImpl {
  def available(c: blackbox.Context): c.Expr[Boolean] = {
    import c.universe._
    val found = try {
      c.mirror.staticClass("probe.Added")
      true
    } catch {
      case _: scala.ScalaReflectionException => false
    }
    c.Expr[Boolean](Literal(Constant(found)))
  }
}
