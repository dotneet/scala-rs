package tracerfixture

import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

trait Trace {
  type Type <: AnyRef
  trait Traced
}

object Trace {
  val instance: Trace = new Trace {
    type Type = String
    class Traced
  }
  implicit def auto: instance.Type with instance.Traced = macro TraceImpl.auto
}

object TraceImpl {
  def auto(c: Context): c.Tree = {
    import c.universe._
    q"???"
  }
}
