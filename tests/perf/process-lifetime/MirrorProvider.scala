package probe

import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object MirrorProvider {
  def source: Int = macro MirrorProviderImpl.source
  def imported[A]: Int = macro MirrorProviderImpl.imported[A]
}

object MirrorProviderImpl {
  def source(c: blackbox.Context): c.Expr[Int] = {
    c.internal.enclosingOwner
    import c.universe._
    c.Expr[Int](q"1")
  }

  def imported[A: c.WeakTypeTag](c: blackbox.Context): c.Expr[Int] = {
    import c.universe._
    weakTypeOf[A].typeSymbol.info
    c.Expr[Int](q"2")
  }
}
