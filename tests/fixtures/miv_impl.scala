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
trait MivPlain
trait MivMissing

object MivFlag {
  implicit def automatic: MivFlag = macro MivImpl.automaticFlagImpl
}

object Miv {
  def pathDependent(): String = macro MivImpl.pathDependentImpl
  def ordinary(): String = macro MivImpl.ordinaryImpl
  def traced(): MivToken.instance.Type with MivToken.Traced = macro MivImpl.tracedImpl
  def enabled(): String = macro MivImpl.enabledImpl
  def required(): String = macro MivImpl.requiredImpl
  def nonStatic(token: MivToken): String = macro MivImpl.nonStaticImpl
  def positioned(): String = macro MivImpl.positionedImpl
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
    val found = c.inferImplicitValue(weakTypeOf[MivPlain], silent = true)
    Literal(Constant(if (found == EmptyTree) "empty" else found.tpe.toString))
  }
  def tracedImpl(c: Context)(): c.Tree = {
    import c.universe._
    q"null.asInstanceOf[MivToken.instance.Type with MivToken.Traced]"
  }
  def enabledImpl(c: Context)(): c.Tree = {
    import c.universe._
    val found = c.inferImplicitValue(
      weakTypeOf[MivFlag],
      silent = true,
      withMacrosDisabled = false
    )
    Literal(Constant(if (found == EmptyTree) "empty" else "found"))
  }
  def automaticFlagImpl(c: Context): c.Tree = {
    import c.universe._
    q"null.asInstanceOf[MivFlag]"
  }
  def requiredImpl(c: Context)(): c.Tree = {
    import c.universe._
    c.inferImplicitValue(weakTypeOf[MivMissing], silent = false)
    Literal(Constant("continued"))
  }
  def nonStaticImpl(c: Context)(token: c.Expr[MivToken]): c.Tree = {
    import c.universe._
    val member = token.actualType.member(TypeName("Type"))
    val prefix = c.universe.internal.singleType(NoPrefix, token.tree.symbol)
    val target = c.universe.internal.typeRef(prefix, member, Nil)
    val found = c.inferImplicitValue(target, silent = true)
    Literal(Constant(if (found == EmptyTree) "empty" else "found"))
  }
  def positionedImpl(c: Context)(): c.Tree = {
    import c.universe._
    val found = c.inferImplicitValue(
      weakTypeOf[MivPlain],
      silent = true,
      withMacrosDisabled = false,
      pos = NoPosition
    )
    Literal(Constant(if (found == EmptyTree) "empty" else "found"))
  }
}
