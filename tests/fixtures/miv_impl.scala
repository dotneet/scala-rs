// Reverse-RPC fixture for `c.inferImplicitValue`.  The important type is the
// stable path-dependent `MivToken.instance.Type`, the same reflect shape used
// by ZIO's stack tracer (`Tracer.instance.Type`).
import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

trait MivToken { type Type }

object MivToken {
  trait Traced
  val instance: MivToken = new MivToken {}

  // This companion candidate must be invisible to the query below, which
  // asks for `withMacrosDisabled = true`.
  implicit def automatic: instance.Type = macro automaticImpl
  def automaticImpl(c: Context): c.Tree = {
    import c.universe._
    q"null.asInstanceOf[MivToken.instance.Type]"
  }
}

trait MivFlag

object Miv {
  def pathDependent(): String = macro MivImpl.pathDependentImpl
  def ordinary(): String = macro MivImpl.ordinaryImpl
  def traced(): MivToken.instance.Type with MivToken.Traced = macro MivImpl.tracedImpl
}

object MivImpl {
  def pathDependentImpl(c: Context)(): c.Tree = {
    import c.universe._
    val found = c.inferImplicitValue(
      weakTypeOf[MivToken.instance.Type],
      silent = true,
      withMacrosDisabled = true
    )
    Literal(Constant(if (found == EmptyTree) "empty" else "found"))
  }
  def ordinaryImpl(c: Context)(): c.Tree = {
    import c.universe._
    val found = c.inferImplicitValue(weakTypeOf[MivFlag], silent = true)
    Literal(Constant(if (found == EmptyTree) "empty" else found.tpe.toString))
  }
  def tracedImpl(c: Context)(): c.Tree = {
    import c.universe._
    q"null.asInstanceOf[MivToken.instance.Type with MivToken.Traced]"
  }
}
