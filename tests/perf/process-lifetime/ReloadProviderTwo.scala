package probe

import scala.language.experimental.macros
import scala.reflect.macros.blackbox

object ReloadProvider {
  def value: Int = macro ReloadProviderImpl.value
}

object ReloadProviderImpl {
  def value(c: blackbox.Context): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](q"2")
  }
}
